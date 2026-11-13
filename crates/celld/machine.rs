// Copyright 2026 Deno Land Inc. Apache-2.0 license.

//! What the process can learn about where it is running.
//!
//! Two kinds of fact, both read once at startup or sampled on a timer:
//! the environment the operator set, and the machine underneath — memory,
//! CPU ticks, page size — which is per-platform and therefore duplicated
//! behind `cfg` for each one.
use celld_logic::OwnershipOnEvict;
use rand::RngCore as _;

/// Validate one node name before it enters a storage key, a local path, or a
/// peer message. All three surfaces use this same restricted identity.
pub(crate) fn validate_node_name(node: &str) -> anyhow::Result<()> {
    if celld_logic::peer::valid_identity(node) {
        Ok(())
    } else {
        anyhow::bail!(
            "must be a safe node name of 1 to 128 ASCII letters, numbers, dots, dashes, or underscores"
        )
    }
}

/// The default bound on concurrent cold activations for a machine with this
/// many hardware threads: eight per thread, at least 16, at most 128.
///
/// A cold activation is object-store latency, not compute. Measured on GCS
/// from a 4-vCPU node (2026-09-01), the route was ~100 ms of round trips
/// (owner read 25 ms, ownership CAS 60 ms, restore 10 ms) and ~10 ms of CPU,
/// so a bound equal to the thread count admitted 35-40 activations a second
/// per node whatever the client offered, and 1,000 hibernated alarms due in
/// one minute were 29 s late at the tail. The same ceiling appeared on
/// two-core nodes. Eight per thread keeps the CPU share of the route
/// below one core per thread at full duty (measured 30 % of four vCPUs at
/// 16 slots, 43 % at 64); the floor keeps a one- or two-thread node from
/// serializing an I/O-bound path; the cap is the point past which more
/// concurrent cold I/O can itself become an object-store storm, and it is
/// the value the previous default already capped at.
pub fn default_max_activations(available_parallelism: usize) -> usize {
    const PER_THREAD: usize = 8;
    const FLOOR: usize = 16;
    const CAP: usize = 128;
    available_parallelism
        .saturating_mul(PER_THREAD)
        .clamp(FLOOR, CAP)
}

pub fn random_node_session_id() -> String {
    let mut bytes = [0_u8; 16];
    crate::asyncrt::rng("session").fill_bytes(&mut bytes);
    let suffix = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("node_{suffix}")
}

pub fn random_peer_key() -> [u8; 32] {
    let mut key = [0_u8; 32];
    crate::asyncrt::rng("peer_key").fill_bytes(&mut key);
    key
}

pub fn random_process_generation() -> String {
    let mut generation = [0_u8; 32];
    crate::asyncrt::rng("generation").fill_bytes(&mut generation);
    generation
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Watermarks from celld's public environment contract.
/// `CELLD_PRESSURE_OWNERSHIP`: `release` (the default) or `sticky`.
/// The lease lifetime, as the capacity listing needs it to decide which node
/// records are stale enough to skip reading. Same variable and default the
/// lease itself uses.
/// Concurrent outbound WebSockets one cell may hold
/// (`CELLD_MAX_OUTBOUND_WEBSOCKETS`).
pub const DEFAULT_MAX_OUTBOUND_WEBSOCKETS: usize = 32;

/// How long a revoked residency hold may take to wind down before the node
/// drops it (`CELLD_HOLD_GRACE_MS`). Bounds how long a held cell can delay a
/// drain, a forced generation swap or a clean reload.
pub const DEFAULT_HOLD_GRACE_MS: u64 = 5_000;

/// Live residency holds one cell may carry (`CELLD_MAX_HOLDS_PER_CELL`).
pub const DEFAULT_MAX_HOLDS_PER_CELL: usize = 16;

/// Backoff between dead-owner log recovery retries. One second keeps a
/// waiting takeover responsive to the sweep finishing without hammering
/// the recovery lock.
pub const DEFAULT_OWNER_LOG_RECOVERY_BACKOFF_MS: u64 = 1_000;

/// Recovery retry cycles before a takeover request fails. 240 cycles at
/// the default backoff covers the ~155 s dead-leader sweep measured for
/// a thousand-cell owner with a wide margin;
/// clients bound their own waits far earlier, so the cap only fires on
/// a recovery that is truly wedged.
pub const DEFAULT_OWNER_LOG_RECOVERY_ATTEMPTS: u32 = 240;

pub const DEFAULT_LOCAL_CACHE_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Matches the fleet and ownership-store clients. Long enough to survive a
/// slow but live peer, short enough that a stale address does not hold a
/// request for the kernel's own connect timeout.
pub const PEER_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// The node lease: how long it lives and how often it is renewed.
///
/// The lease is the safety lane of the fleet. A node serves a cell only while
/// its lease is live, and it fences itself (stops serving and exits) when it
/// has not managed to renew in `ttl_ms`; a peer takes a dead node's cells only
/// after the lease has lapsed. So the TTL is also the failover time, and the
/// renewal interval is what decides how many failed or slow attempts one lease
/// survives.
///
/// | Setting | Flag | Variable | Default |
/// | --- | --- | --- | --- |
/// | `ttl_ms` | `--lease-ttl-ms` | `CELLD_TTL_MS` | 10000 |
/// | `renew_ms` | `--lease-renew-ms` | `CELLD_LEASE_RENEW_MS` | `ttl_ms / 3` |
///
/// Constraints, all refused at startup rather than defaulted:
///
/// * `MIN_LEASE_TTL_MS` (1 s) `<= ttl_ms <= MAX_LEASE_TTL_MS` (1 h). Below a
///   second a single slow write fences the node; above an hour a dead node's
///   cells are unavailable for as long.
/// * `MIN_LEASE_RENEW_MS` (100 ms) `<= renew_ms <= ttl_ms / 3`. The renewal
///   must leave at least two more attempts inside the lease after the first,
///   which is what lets one slow or failed write pass without a fence. A
///   cadence below 100 ms writes the bucket ten times a second for nothing.
///
/// A host that is overloaded, or a developer machine that is suspended and
/// loaded, raises `ttl_ms` and leaves `renew_ms` to follow it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaseTiming {
    pub ttl_ms: u64,
    pub renew_ms: u64,
}

pub const DEFAULT_LEASE_TTL_MS: u64 = 10_000;
pub const MIN_LEASE_TTL_MS: u64 = 1_000;
pub const MAX_LEASE_TTL_MS: u64 = 3_600_000;
pub const MIN_LEASE_RENEW_MS: u64 = 100;

impl LeaseTiming {
    /// Check a TTL and a renewal interval against each other. `None` selects
    /// the default: 10 s, and a third of the TTL.
    pub fn resolve(ttl_ms: Option<u64>, renew_ms: Option<u64>) -> anyhow::Result<Self> {
        let ttl_ms = ttl_ms.unwrap_or(DEFAULT_LEASE_TTL_MS);
        anyhow::ensure!(
            (MIN_LEASE_TTL_MS..=MAX_LEASE_TTL_MS).contains(&ttl_ms),
            "the node lease TTL (CELLD_TTL_MS, --lease-ttl-ms) must be between \
             {MIN_LEASE_TTL_MS} and {MAX_LEASE_TTL_MS} ms, not {ttl_ms}"
        );
        let longest = ttl_ms / 3;
        let renew_ms = renew_ms.unwrap_or(longest);
        anyhow::ensure!(
            (MIN_LEASE_RENEW_MS..=longest).contains(&renew_ms),
            "the node lease renewal interval (CELLD_LEASE_RENEW_MS, --lease-renew-ms) must be \
             between {MIN_LEASE_RENEW_MS} ms and a third of the lease TTL ({longest} ms for a \
             TTL of {ttl_ms} ms), not {renew_ms}: a lease must get at least three renewal \
             attempts before it lapses"
        );
        Ok(Self { ttl_ms, renew_ms })
    }
}

/// What `--lease-ttl-ms` and `--lease-renew-ms` set. They win over the
/// variables, as every other flag does.
static LEASE_FLAGS: std::sync::Mutex<(Option<u64>, Option<u64>)> = std::sync::Mutex::new((None, None));

/// Record the lease flags and refuse an invalid combination. Called while the
/// command line is parsed, before anything reads the timing.
pub fn set_lease_flags(ttl_ms: Option<u64>, renew_ms: Option<u64>) -> anyhow::Result<()> {
    {
        let mut flags = LEASE_FLAGS.lock().unwrap();
        if ttl_ms.is_some() {
            flags.0 = ttl_ms;
        }
        if renew_ms.is_some() {
            flags.1 = renew_ms;
        }
    }
    lease_timing_from_environment().map(|_| ())
}

/// The lease timing this process runs with: flags, then the environment, then
/// the defaults. Fallible so startup can report a bad value; the readers below
/// run after `env_vars::validate` and unwrap it.
pub fn lease_timing_from_environment() -> anyhow::Result<LeaseTiming> {
    let (flag_ttl, flag_renew) = *LEASE_FLAGS.lock().unwrap();
    let ttl_ms = match flag_ttl {
        Some(ttl_ms) => Some(ttl_ms),
        None => crate::env_vars::optional::<u64>("CELLD_TTL_MS")?,
    };
    let renew_ms = match flag_renew {
        Some(renew_ms) => Some(renew_ms),
        None => crate::env_vars::optional::<u64>("CELLD_LEASE_RENEW_MS")?,
    };
    LeaseTiming::resolve(ttl_ms, renew_ms)
}

pub fn lease_ttl_ms_from_environment() -> u64 {
    lease_timing_from_environment()
        .expect("validated node lease timing")
        .ttl_ms
}

pub fn ownership_on_evict_from_environment() -> anyhow::Result<OwnershipOnEvict> {
    match std::env::var("CELLD_PRESSURE_OWNERSHIP") {
        Err(_) => Ok(OwnershipOnEvict::Release),
        Ok(value) => match value.trim() {
            "release" => Ok(OwnershipOnEvict::Release),
            "sticky" => Ok(OwnershipOnEvict::Sticky),
            other => Err(anyhow::anyhow!(
                "CELLD_PRESSURE_OWNERSHIP must be `release` or `sticky`, not `{other}`"
            )),
        },
    }
}

/// Total memory this process may use: the cgroup limit when one applies
/// (containers), the machine otherwise.
#[cfg(target_os = "linux")]
#[allow(clippy::disallowed_methods)] // cgroup and `/proc` files describe the host.
fn total_memory_bytes() -> Option<u64> {
    if let Some(limit) = crate::memory::cgroup_limit_bytes() {
        return Some(limit);
    }
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb = meminfo
        .lines()
        .find(|line| line.starts_with("MemTotal:"))?
        .split_whitespace()
        .nth(1)?
        .parse::<u64>()
        .ok()?;
    Some(kb.saturating_mul(1024))
}

#[cfg(target_os = "macos")]
fn total_memory_bytes() -> Option<u64> {
    let mut size: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    let ok = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&raw mut size).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (ok == 0 && size > 0).then_some(size)
}

#[cfg(windows)]
fn total_memory_bytes() -> Option<u64> {
    crate::winos::total_physical_memory_bytes()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn total_memory_bytes() -> Option<u64> {
    None
}

pub fn pressure_config_from_environment() -> anyhow::Result<celld_logic::pressure::PressureConfig> {
    // Residency is a hard cap (`CELLD_MAX_RESIDENT_CELLS` -> `Config::max_resident`),
    // enforced at admission -- not a pressure watermark, so it is not built here.
    // Memory shedding is on by default: a node that runs into its memory ceiling
    // must give cells back, not be killed. The arithmetic lives in the core;
    // the shell supplies only the two facts it can read.
    let config = celld_logic::pressure::PressureConfig::from_limits(
        total_memory_bytes(),
        crate::env_vars::optional::<u64>("CELLD_MAX_RSS_MB")?,
    );
    if config.ceiling_above_cap() {
        tracing::warn!(
            high_bytes = config.high_bytes,
            rss_hard_bytes = config.rss_hard_bytes,
            "CELLD_MAX_RSS_MB is at or above the absolute cap, so the node \
             decides on its complete cgroup charge or RSS fallback"
        );
    }
    Ok(config)
}

pub fn local_cache_max_bytes_from_environment() -> anyhow::Result<Option<u64>> {
    let bytes = crate::env_vars::with_default(
        "CELLD_LOCAL_CACHE_MAX_BYTES",
        DEFAULT_LOCAL_CACHE_MAX_BYTES,
    )?;
    Ok((bytes > 0).then_some(bytes))
}

#[cfg(all(test, celld_internal_tests))]
mod internal_tests {
    include!(env!("CELLD_INTERNAL_MACHINE_TESTS"));
}

#[cfg(test)]
mod lease_timing_tests {
    use super::*;

    #[test]
    fn the_defaults_are_ten_seconds_renewed_every_third() {
        let timing = LeaseTiming::resolve(None, None).unwrap();
        assert_eq!(timing.ttl_ms, 10_000);
        assert_eq!(timing.renew_ms, 3_333);
    }

    #[test]
    fn the_cadence_follows_a_raised_ttl() {
        let timing = LeaseTiming::resolve(Some(60_000), None).unwrap();
        assert_eq!((timing.ttl_ms, timing.renew_ms), (60_000, 20_000));
    }

    #[test]
    fn a_cadence_up_to_a_third_of_the_ttl_is_accepted() {
        assert!(LeaseTiming::resolve(Some(30_000), Some(10_000)).is_ok());
        assert!(LeaseTiming::resolve(Some(30_000), Some(100)).is_ok());
        assert!(LeaseTiming::resolve(None, Some(1_000)).is_ok());
    }

    #[test]
    fn a_cadence_that_leaves_fewer_than_three_attempts_is_refused() {
        let error = LeaseTiming::resolve(Some(30_000), Some(10_001)).unwrap_err();
        assert!(
            error.to_string().contains("a third of the lease TTL"),
            "{error}"
        );
        // Half the lease is a cadence of two attempts.
        assert!(LeaseTiming::resolve(Some(10_000), Some(5_000)).is_err());
        // The default TTL's third is 3333, so 3334 is over.
        assert!(LeaseTiming::resolve(None, Some(3_334)).is_err());
    }

    #[test]
    fn a_cadence_that_hammers_the_bucket_is_refused() {
        assert!(LeaseTiming::resolve(None, Some(99)).is_err());
        assert!(LeaseTiming::resolve(None, Some(0)).is_err());
    }

    #[test]
    fn the_ttl_has_bounds() {
        assert!(LeaseTiming::resolve(Some(999), None).is_err());
        assert!(LeaseTiming::resolve(Some(0), None).is_err());
        assert!(LeaseTiming::resolve(Some(1_000), None).is_ok());
        assert!(LeaseTiming::resolve(Some(MAX_LEASE_TTL_MS), None).is_ok());
        assert!(LeaseTiming::resolve(Some(MAX_LEASE_TTL_MS + 1), None).is_err());
        // A cadence valid for the default TTL is not valid for a shorter one.
        assert!(LeaseTiming::resolve(Some(1_000), Some(1_000)).is_err());
    }
}
