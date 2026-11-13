//! Windows host primitives.
//!
//! Everything here replaces a Unix call site elsewhere in the crate; see
//! `docs/fork/windows.md` for the site-by-site table. The module is compiled
//! on Windows only. Each function names the Unix call it stands in for.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::Foundation::{
    CloseHandle, FALSE, FILETIME, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{SetNamedSecurityInfoW, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{
    AddAccessAllowedAce, GetLengthSid, GetTokenInformation, InitializeAcl, TokenUser, ACL,
    ACL_REVISION, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, TOKEN_QUERY,
    TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;
use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::ProcessStatus::{
    K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThreadId, GetProcessHandleCount, GetProcessTimes, GetThreadTimes,
    OpenProcessToken, OpenThread, THREAD_QUERY_LIMITED_INFORMATION,
};

fn filetime_100ns(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

fn zero_filetime() -> FILETIME {
    FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    }
}

/// The process working set, the counterpart of Linux `statm` resident pages
/// and macOS `pti_resident_size`. Zero when the query fails.
pub fn working_set_bytes() -> u64 {
    // SAFETY: the counters struct is plain data sized by `cb`, and the
    // pseudo-handle for the current process needs no close.
    unsafe {
        let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) == 0 {
            return 0;
        }
        counters.WorkingSetSize as u64
    }
}

/// Physical memory of the machine, the counterpart of `/proc/meminfo`
/// `MemTotal` and macOS `hw.memsize`.
pub fn total_physical_memory_bytes() -> Option<u64> {
    // SAFETY: `dwLength` is set as the API requires.
    unsafe {
        let mut status: MEMORYSTATUSEX = std::mem::zeroed();
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        (GlobalMemoryStatusEx(&mut status) != 0 && status.ullTotalPhys > 0)
            .then_some(status.ullTotalPhys)
    }
}

/// User plus kernel CPU time of the process in 100 ns units, the counterpart
/// of `/proc/self/stat` utime + stime.
pub fn process_cpu_100ns() -> Option<u64> {
    // SAFETY: all four out-pointers are valid FILETIMEs.
    unsafe {
        let (mut created, mut exited, mut kernel, mut user) =
            (zero_filetime(), zero_filetime(), zero_filetime(), zero_filetime());
        if GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        ) == 0
        {
            return None;
        }
        Some(filetime_100ns(kernel).saturating_add(filetime_100ns(user)))
    }
}

/// Open kernel handles, the counterpart of counting `/proc/self/fd`.
pub fn open_handle_count() -> u64 {
    let mut count = 0_u32;
    // SAFETY: the out-pointer is valid for the call.
    if unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) } == 0 {
        return 0;
    }
    u64::from(count)
}

/// Windows has no soft `RLIMIT_NOFILE`. A process may hold up to 2^24
/// handles (the documented kernel limit), which is what `fd_limit` reports.
pub const HANDLE_LIMIT: u64 = 1 << 24;

/// An identifier for the calling thread that another thread can query.
pub fn current_thread_id() -> u32 {
    // SAFETY: no arguments, no side effects.
    unsafe { GetCurrentThreadId() }
}

/// CPU time of thread `id` in nanoseconds, the counterpart of
/// `clock_gettime(CLOCK_THREAD_CPUTIME_ID)` / `pthread_getcpuclockid`.
/// `GetThreadTimes` reports user and kernel time in 100 ns units.
pub fn thread_cpu_nanos(id: u32) -> Option<u64> {
    // SAFETY: the handle is opened for query only and closed on every path;
    // the FILETIME out-pointers are valid for the call.
    unsafe {
        let handle = OpenThread(THREAD_QUERY_LIMITED_INFORMATION, FALSE, id);
        if handle.is_null() {
            return None;
        }
        let (mut created, mut exited, mut kernel, mut user) =
            (zero_filetime(), zero_filetime(), zero_filetime(), zero_filetime());
        let ok = GetThreadTimes(handle, &mut created, &mut exited, &mut kernel, &mut user);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        filetime_100ns(kernel)
            .checked_add(filetime_100ns(user))?
            .checked_mul(100)
    }
}

/// `CPU time of the calling thread in nanoseconds.
pub fn current_thread_cpu_nanos() -> Option<u64> {
    thread_cpu_nanos(current_thread_id())
}

/// Tie the lifetime of `child` to this process: when this process exits for
/// any reason, including being killed, the kernel kills the child. This is
/// the counterpart of Linux `PR_SET_PDEATHSIG`. The job object handle is
/// intentionally kept open for the life of the process; closing it (which
/// process exit does) is what triggers the kill.
pub fn kill_on_parent_exit(child: std::os::windows::io::RawHandle) -> io::Result<()> {
    use std::sync::OnceLock;
    struct Job(HANDLE);
    // SAFETY: a job handle is a kernel object reference usable from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}
    static JOB: OnceLock<Result<Job, i32>> = OnceLock::new();
    let job = JOB.get_or_init(|| {
        // SAFETY: standard job object creation; the info struct is plain data.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(io::Error::last_os_error().raw_os_error().unwrap_or(0));
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                let error = io::Error::last_os_error().raw_os_error().unwrap_or(0);
                CloseHandle(job);
                return Err(error);
            }
            Ok(Job(job))
        }
    });
    let job = job
        .as_ref()
        .map_err(|code| io::Error::from_raw_os_error(*code))?;
    // SAFETY: both handles are valid; the child handle is borrowed.
    if unsafe { AssignProcessToJobObject(job.0, child as HANDLE) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Ask `process_group` to shut down gracefully: the counterpart of
/// `kill(pid, SIGTERM)`. The target must have been created with
/// `CREATE_NEW_PROCESS_GROUP` (so its pid is its group id) and share this
/// process's console; it receives `CTRL_BREAK_EVENT`, which `main.rs` and
/// `dev.rs` treat as the graceful-stop request. Unlike `TerminateProcess`,
/// this lets the node run its durability handoff.
pub fn send_ctrl_break(process_group: u32) -> io::Result<()> {
    // SAFETY: plain call with integer arguments.
    if unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, process_group) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Replace the DACL of `path` with a single entry granting the current user
/// full access, with inheritance from the parent turned off. This is the
/// counterpart of `chmod 0600`: after it, no other user (and no group)
/// can read the file, which matters for key and credential files.
pub fn restrict_to_current_user(path: &Path) -> io::Result<()> {
    // SAFETY: every buffer outlives the call that uses it; the token handle
    // is closed on every path; the ACL buffer is u32-aligned as ACLs require.
    unsafe {
        let mut token: HANDLE = INVALID_HANDLE_VALUE;
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            let mut needed = 0_u32;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
            if needed == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut user = vec![0_u64; (needed as usize).div_ceil(8)];
            if GetTokenInformation(token, TokenUser, user.as_mut_ptr().cast(), needed, &mut needed)
                == 0
            {
                return Err(io::Error::last_os_error());
            }
            let sid = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
            let sid_length = GetLengthSid(sid);
            // ACL header + one ACCESS_ALLOWED_ACE (its SidStart overlaps the
            // first SID DWORD, hence the - 4) + the SID, rounded to DWORDs.
            let acl_size = (std::mem::size_of::<ACL>() + 8 + 4 + sid_length as usize + 3) & !3;
            let mut acl = vec![0_u32; acl_size / 4];
            let acl_ptr = acl.as_mut_ptr().cast::<ACL>();
            if InitializeAcl(acl_ptr, acl_size as u32, ACL_REVISION) == 0 {
                return Err(io::Error::last_os_error());
            }
            if AddAccessAllowedAce(acl_ptr, ACL_REVISION, FILE_ALL_ACCESS, sid) == 0 {
                return Err(io::Error::last_os_error());
            }
            let wide: Vec<u16> = OsStr::new(path)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let status = SetNamedSecurityInfoW(
                wide.as_ptr().cast_mut(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                acl_ptr,
                std::ptr::null_mut(),
            );
            if status != 0 {
                return Err(io::Error::from_raw_os_error(status as i32));
            }
            Ok(())
        })();
        CloseHandle(token);
        result
    }
}

/// The longest file name component the volume holding `path` supports, in
/// UTF-16 units (255 on NTFS and ReFS). The counterpart of
/// `fpathconf(_PC_NAME_MAX)`.
pub fn volume_max_component_length(path: &Path) -> io::Result<u32> {
    use windows_sys::Win32::Storage::FileSystem::{GetVolumeInformationW, GetVolumePathNameW};
    let absolute = std::path::absolute(path)?;
    let wide: Vec<u16> = absolute
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut root = vec![0_u16; 32_768];
    // SAFETY: the input is NUL-terminated; the output buffer length is passed.
    if unsafe { GetVolumePathNameW(wide.as_ptr(), root.as_mut_ptr(), root.len() as u32) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut max_component = 0_u32;
    // SAFETY: `root` is NUL-terminated by the call above; unused outputs are null.
    let ok = unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut max_component,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(max_component)
}
