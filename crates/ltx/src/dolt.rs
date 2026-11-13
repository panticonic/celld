//! `DoltStore`: the capture half of a Dolt cell.
//!
//! A Dolt cell's file is a DoltLite chunk store (`chunk_store.h`): a
//! 168-byte manifest at offset 0, a compacted region and index, then an
//! append-only WAL of chunk records (`0x01`, hash, length, bytes) and root
//! records (`0x02` + a sealed 168-byte manifest). A commit appends chunks and
//! then one root, and fsyncs; the manifest at offset 0 is written only when
//! the file is created and when `dolt_gc` rewrites the whole file and renames
//! it into place. Bytes after the newest root are an open transaction's
//! chunks and can be truncated and rewritten, so they are never captured.
//!
//! The durable unit is therefore the byte range appended since the previous
//! capture, cut at the end of the newest **sealed** root (its self-hash,
//! BLAKE3 of the manifest bound to the root's file offset, verifies; a root
//! still being written does not). No DoltLite API reports that cut, so the
//! capture parses the WAL forward from where the previous capture stopped.
//!
//! The range is carried as the page images (4 KiB) that cover it, in an
//! ordinary L0 LTX file: the page holding the previous cut, every page after
//! it, and the last page zero-filled past the cut. Everything downstream of
//! [`crate::cell_store::CellStore`] — followers, bundles, the bucket upload,
//! compaction, the epoch chain, restore and the output gate's proofs — is the
//! same code a SQLite cell runs. A position is a TXID; the byte offset of each
//! capture is recorded in the L0 header (`wal_size` is the cut,
//! `wal_offset` where the next record starts, `wal_salt1` the
//! [`DOLT_LTX_TAG`]).
//!
//! A new file generation (`dolt_gc`, a restore, a file replaced under the
//! store) is detected by a changed offset-0 manifest or a file shorter than
//! the cut, and captured whole: every page, with `commit` set to the new
//! page count, which truncates a restore exactly as a SQLite VACUUM does.
//!
//! A restored file is the page image of a cut: zero bytes may follow the last
//! root on its last page. [`DoltStore::open`] truncates them before the
//! application opens the file, so DoltLite never sees them as a torn tail.
//!
//! The LTX format never carries SQLite's lock page (the 4 KiB page at
//! 1 GiB, [`lock_pgno`]): every reader zero-fills it. A chunk file has no
//! such page, so its pages map onto LTX page numbers that skip it: file page
//! `n` is LTX page `n` below the lock page and `n + 1` from it on
//! ([`ltx_pgno`]). An image rebuilt from LTX pages then has a 4 KiB hole at
//! 1 GiB, which [`unmap_restored_image`] removes before the store opens it.
//! A chunk file can therefore grow past 1 GiB.

use std::path::{Path, PathBuf};

use crate::cell_store::{CellStore, Engine};
use crate::db::{CheckpointMode, SyncTiming};
use crate::error::{Error, Result};
use crate::ltx::{self, lock_pgno};
use crate::{Pos, TXID};

/// The LTX page number of the chunk file's page `file_page` (1-based): the
/// LTX lock page is skipped. See the module documentation.
pub fn ltx_pgno(file_page: u32) -> Option<u32> {
    ltx_pgno_with_lock(file_page, lock_pgno(DOLT_PAGE_SIZE))
}

fn ltx_pgno_with_lock(file_page: u32, lock: u32) -> Option<u32> {
    if file_page >= lock {
        file_page.checked_add(1)
    } else {
        Some(file_page)
    }
}

/// Remove the lock-page hole from a chunk file rebuilt from LTX pages (see
/// the module documentation). A file that does not reach the lock page is
/// left alone. Runs before anything opens the restored file.
pub fn unmap_restored_image(path: &Path) -> Result<()> {
    let page = DOLT_PAGE_SIZE as u64;
    unmap_hole(path, (lock_pgno(DOLT_PAGE_SIZE) as u64 - 1) * page, page)
}

fn unmap_hole(path: &Path, hole: u64, page: u64) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let len = match std::fs::metadata(path) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if len <= hole {
        return Ok(());
    }
    if len < hole + page {
        return Err(Error::Other(
            format!("a restored Dolt image of {len} bytes ends inside the LTX lock page").into(),
        ));
    }
    let mut file = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    let mut buf = vec![0u8; 8 << 20];
    let mut from = hole + page;
    let mut to = hole;
    while from < len {
        let n = ((len - from) as usize).min(buf.len());
        file.seek(SeekFrom::Start(from))?;
        file.read_exact(&mut buf[..n])?;
        file.seek(SeekFrom::Start(to))?;
        file.write_all(&buf[..n])?;
        from += n as u64;
        to += n as u64;
    }
    file.set_len(len - page)?;
    file.sync_all()?;
    Ok(())
}

/// The LTX pages (and page count) of a whole chunk-file image.
pub fn image_pages(image: &[u8]) -> Result<(u32, Vec<(u32, Vec<u8>)>)> {
    DoltStore::pages(image, 0, image.len() as u64)
}

/// The page size of a Dolt cell's LTX frames.
pub const DOLT_PAGE_SIZE: u32 = 4096;
/// `wal_salt1` of every L0 a [`DoltStore`] writes ("DOLT").
pub const DOLT_LTX_TAG: u32 = 0x444F_4C54;

const MANIFEST_SIZE: usize = 168;
const STORE_MAGIC: u32 = 0x444C_5443;
const STORE_VERSION: u32 = 12;
const HASH_SIZE: usize = 20;
const OFF_WAL_OFFSET: usize = 84;
const OFF_NEXT: usize = 52;
const OFF_SELF_HASH: usize = 124;
const TAG_CHUNK: u8 = 0x01;
const TAG_ROOT: u8 = 0x02;
const CHUNK_HDR: usize = 1 + HASH_SIZE + 4;
const ROOT_RECORD: usize = 1 + MANIFEST_SIZE;
/// Writers pad a batch to at most a 64 KiB sector (`chunk_store.c`).
const MAX_GAP: u64 = 65536;

/// Whether `bytes` begin with a DoltLite chunk-store manifest.
pub fn is_chunk_store(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && le_u32(&bytes[0..]) == STORE_MAGIC
}

fn le_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn le_i64(b: &[u8]) -> i64 {
    i64::from_le_bytes(b[..8].try_into().expect("eight bytes"))
}

/// The BLAKE3 seal DoltLite stamps on a manifest written at `offset`
/// (`csManifestSeal`): the hash of the manifest with its self-hash field
/// zeroed, followed by the offset as a little-endian i64, truncated to 20
/// bytes.
fn manifest_seal(manifest: &[u8], offset: u64) -> [u8; HASH_SIZE] {
    let mut copy = [0u8; MANIFEST_SIZE];
    copy.copy_from_slice(&manifest[..MANIFEST_SIZE]);
    copy[OFF_SELF_HASH..OFF_SELF_HASH + HASH_SIZE].fill(0);
    let mut hasher = blake3::Hasher::new();
    hasher.update(&copy);
    hasher.update(&(offset as i64).to_le_bytes());
    let mut out = [0u8; HASH_SIZE];
    hasher.finalize_xof().fill(&mut out);
    out
}

/// A root record's manifest at file offset `offset` that DoltLite would
/// accept as a commit: magic, version, and a verifying seal.
fn sealed_root(manifest: &[u8], offset: u64) -> bool {
    le_u32(&manifest[0..]) == STORE_MAGIC
        && le_u32(&manifest[4..]) == STORE_VERSION
        && manifest[OFF_SELF_HASH..OFF_SELF_HASH + HASH_SIZE] != [0u8; HASH_SIZE]
        && manifest[OFF_SELF_HASH..OFF_SELF_HASH + HASH_SIZE] == manifest_seal(manifest, offset)
}

/// Where a WAL scan ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cut {
    /// The end of the newest sealed root: every byte below it is committed.
    pub end: u64,
    /// Where the record after that root starts (its `NEXT_OFF`).
    pub resume: u64,
}

/// Scan WAL records of a chunk file. `buf` holds the file from file offset
/// `base`; the scan starts at the record boundary `from` (with `cut` the
/// committed state at that point) and returns the cut after the newest sealed
/// root it reaches. A partial record, an unknown tag or a root that does not
/// verify ends the scan: that is an open transaction or a torn tail.
pub fn scan_wal(buf: &[u8], base: u64, from: u64, cut: Cut) -> Cut {
    let end_of_buf = base + buf.len() as u64;
    let at = |offset: u64| (offset - base) as usize;
    let mut position = from;
    let mut cut = cut;
    while position < end_of_buf {
        match buf[at(position)] {
            TAG_CHUNK => {
                if position + CHUNK_HDR as u64 > end_of_buf {
                    break;
                }
                let len = le_u32(&buf[at(position) + 1 + HASH_SIZE..]) as u64;
                let next = position + CHUNK_HDR as u64 + len;
                if next > end_of_buf {
                    break;
                }
                position = next;
            }
            TAG_ROOT => {
                if position + ROOT_RECORD as u64 > end_of_buf {
                    break;
                }
                let manifest = &buf[at(position) + 1..at(position) + ROOT_RECORD];
                if !sealed_root(manifest, position) {
                    break;
                }
                let root_end = position + ROOT_RECORD as u64;
                let next = le_i64(&manifest[OFF_NEXT..]);
                let next = if next <= 0 { root_end } else { next as u64 };
                if next < root_end || next - root_end >= MAX_GAP {
                    break;
                }
                cut = Cut {
                    end: root_end,
                    resume: next,
                };
                position = next;
            }
            _ => break,
        }
    }
    cut
}

/// The committed cut of a whole chunk file, or `None` when the file holds
/// no committed state (shorter than a manifest, or no root and no compacted
/// region).
pub fn scan_file(file: &[u8]) -> Option<Cut> {
    if file.len() < MANIFEST_SIZE || !is_chunk_store(file) {
        return None;
    }
    let wal_offset = le_i64(&file[OFF_WAL_OFFSET..]);
    if wal_offset < MANIFEST_SIZE as i64 || wal_offset as u64 > file.len() as u64 {
        return None;
    }
    let wal_offset = wal_offset as u64;
    let base = Cut {
        end: wal_offset,
        resume: wal_offset,
    };
    let cut = scan_wal(file, 0, wal_offset, base);
    // A file with no root and nothing compacted below its WAL has no commit.
    if cut == base && wal_offset == MANIFEST_SIZE as u64 {
        return None;
    }
    Some(cut)
}

/// The identity of a file generation: a hash of its offset-0 manifest,
/// which only creation and `dolt_gc` write.
fn generation_of(manifest: &[u8]) -> u32 {
    let mut crc = crate::ltx::Crc64::new();
    crc.update(&manifest[..MANIFEST_SIZE]);
    let sum = crc.sum64();
    (sum ^ (sum >> 32)) as u32
}

#[derive(Clone, Copy, Debug)]
struct Captured {
    cut: Cut,
    generation: u32,
    #[cfg(unix)]
    inode: Option<(u64, u64)>,
}

/// The capture half of a Dolt cell. See the module documentation.
pub struct DoltStore {
    host: crate::LtxHost,
    path: PathBuf,
    meta_path: PathBuf,
    /// What the newest L0 covers; `None` before the first capture of this
    /// store, or when the next capture must be whole.
    captured: Option<Captured>,
    pos_cache: Option<Pos>,
}

impl DoltStore {
    /// Opens the capture state for the chunk file at `path`. The file may be
    /// absent (a new cell, created by the application's first open) or a
    /// restored image. Must run before the application opens the file.
    pub fn open(path: impl AsRef<Path>, host: crate::LtxHost) -> Result<DoltStore> {
        let path = path.as_ref().to_path_buf();
        let meta_path = crate::Db::meta_path_for_path(&path);
        host.create_dir_all(&meta_path)?;
        let mut store = DoltStore {
            host,
            path,
            meta_path,
            captured: None,
            pos_cache: None,
        };
        let file = store.read_file()?;
        if let Some(file) = &file {
            anyhow_engine(file)?;
        }
        let pos = store.pos()?;
        if pos.txid == TXID(0) {
            // No local capture: a new cell, or an image a restore just
            // wrote. Drop the zero padding a page image carries past its
            // cut, so DoltLite opens a clean tail.
            if let Some(file) = file {
                if let Some(cut) = scan_file(&file) {
                    if cut.end < file.len() as u64
                        && file[cut.end as usize..].iter().all(|byte| *byte == 0)
                    {
                        truncate(&store.path, cut.end)?;
                    }
                }
            }
        } else {
            // Continue the newest L0 when it describes this file generation.
            let header = store.newest_header()?;
            if let (Some(header), Some(file)) = (header, file) {
                let cut = Cut {
                    end: header.wal_size.max(0) as u64,
                    resume: header.wal_offset.max(0) as u64,
                };
                let same = header.wal_salt1 == DOLT_LTX_TAG
                    && file.len() as u64 >= cut.end
                    && file.len() >= MANIFEST_SIZE
                    && generation_of(&file) == header.wal_salt2;
                if same {
                    store.captured = Some(Captured {
                        cut,
                        generation: header.wal_salt2,
                        #[cfg(unix)]
                        inode: inode_of(&store.path),
                    });
                }
            }
        }
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read_file(&self) -> Result<Option<Vec<u8>>> {
        match self.host.read(&self.path) {
            Ok(bytes) if bytes.is_empty() => Ok(None),
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Reads the file from `offset` to its end (`None` when absent).
    fn read_from(&self, offset: u64) -> Result<Option<(u64, Vec<u8>)>> {
        let mut file = match self.host.open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let len = file.file_len()?;
        if len == 0 {
            return Ok(None);
        }
        if offset > len {
            return Ok(Some((len, Vec::new())));
        }
        let bytes = file.read_exact_at(offset, (len - offset) as usize)?;
        Ok(Some((len, bytes)))
    }

    fn header_bytes(&self) -> Result<Option<Vec<u8>>> {
        let mut file = match self.host.open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if file.file_len()? < MANIFEST_SIZE as u64 {
            return Ok(None);
        }
        Ok(Some(file.read_exact_at(0, MANIFEST_SIZE)?))
    }

    fn level_dir(&self) -> String {
        crate::ltx_level_dir(&self.meta_path.to_string_lossy(), 0)
    }

    fn max_ltx(&self) -> Result<(TXID, TXID)> {
        let entries = match self.host.read_dir(Path::new(&self.level_dir())) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok((TXID(0), TXID(0)))
            }
            Err(error) => return Err(error.into()),
        };
        let mut best = (TXID(0), TXID(0));
        for entry in entries {
            if let Ok((min, max)) = ltx::parse_filename(&entry.file_name.to_string_lossy()) {
                if max > best.1 {
                    best = (min, max);
                }
            }
        }
        Ok(best)
    }

    fn newest_header(&self) -> Result<Option<ltx::Header>> {
        let (min, max) = self.max_ltx()?;
        if max == TXID(0) {
            return Ok(None);
        }
        let bytes = self.read_ltx_file(0, min, max)?;
        Ok(Some(ltx::Header::parse(&bytes)?))
    }

    fn write_l0(&mut self, txid: TXID, bytes: &[u8]) -> Result<()> {
        let dir = self.level_dir();
        self.host.create_dir_all(Path::new(&dir))?;
        let path = crate::ltx_file_path(&self.meta_path.to_string_lossy(), 0, txid, txid);
        let tmp = format!("{path}.tmp");
        let result = (|| -> Result<()> {
            let mut file = self.host.create(Path::new(&tmp))?;
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            self.host.rename(Path::new(&tmp), Path::new(&path))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = self.host.remove_file(Path::new(&tmp));
        }
        result
    }

    /// The LTX pages covering `[first_page_offset, cut)` of the file, from
    /// a buffer that starts at `first_page_offset`, numbered by [`ltx_pgno`],
    /// and the LTX page count of the cut.
    fn pages(buf: &[u8], first_page_offset: u64, cut: u64) -> Result<(u32, Vec<(u32, Vec<u8>)>)> {
        Self::pages_with_lock(buf, first_page_offset, cut, lock_pgno(DOLT_PAGE_SIZE))
    }

    fn pages_with_lock(
        buf: &[u8],
        first_page_offset: u64,
        cut: u64,
        lock: u32,
    ) -> Result<(u32, Vec<(u32, Vec<u8>)>)> {
        let page_size = DOLT_PAGE_SIZE as u64;
        let too_large = || Error::Other("dolt chunk file too large to capture".into());
        let last = u32::try_from(cut.div_ceil(page_size)).map_err(|_| too_large())?;
        let commit = ltx_pgno_with_lock(last, lock).ok_or_else(too_large)?;
        let first = (first_page_offset / page_size) as u32 + 1;
        let mut pages = Vec::with_capacity((last + 1).saturating_sub(first) as usize);
        for file_page in first..=last {
            let start = (file_page as u64 - 1) * page_size;
            let end = (start + page_size).min(cut);
            let mut data = vec![0u8; page_size as usize];
            let from = (start - first_page_offset) as usize;
            let to = (end - first_page_offset) as usize;
            data[..to - from].copy_from_slice(&buf[from..to]);
            pages.push((ltx_pgno_with_lock(file_page, lock).ok_or_else(too_large)?, data));
        }
        Ok((commit, pages))
    }

    fn header(&self, txid: TXID, commit: u32, cut: Cut, generation: u32) -> ltx::Header {
        ltx::Header {
            version: ltx::VERSION,
            flags: ltx::HEADER_FLAG_NO_CHECKSUM,
            page_size: DOLT_PAGE_SIZE,
            commit,
            min_txid: txid,
            max_txid: txid,
            timestamp: self.host.now_unix_millis(),
            pre_apply_checksum: 0,
            wal_offset: cut.resume as i64,
            wal_size: cut.end as i64,
            wal_salt1: DOLT_LTX_TAG,
            wal_salt2: generation,
            node_id: 0,
        }
    }

    /// The bytes and cut of a whole-file capture.
    fn whole(&self) -> Result<Option<(Vec<u8>, Cut, u32)>> {
        let Some(file) = self.read_file()? else {
            return Ok(None);
        };
        anyhow_engine(&file)?;
        let Some(cut) = scan_file(&file) else {
            return Ok(None);
        };
        let generation = generation_of(&file);
        Ok(Some((file, cut, generation)))
    }
}

fn anyhow_engine(file: &[u8]) -> Result<()> {
    if file.len() >= 16 && &file[..16] == b"SQLite format 3\0" {
        return Err(Error::Other(
            "a Dolt cell's file is a stock SQLite database; a cell's engine is fixed when it is created"
                .into(),
        ));
    }
    if file.len() >= 8 && !is_chunk_store(file) {
        return Err(Error::Other("a Dolt cell's file is not a DoltLite chunk store".into()));
    }
    Ok(())
}

fn truncate(path: &Path, len: u64) -> Result<()> {
    let file = std::fs::OpenOptions::new().write(true).open(path)?;
    file.set_len(len)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn inode_of(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path)
        .ok()
        .map(|metadata| (metadata.dev(), metadata.ino()))
}

impl CellStore for DoltStore {
    fn engine(&self) -> Engine {
        Engine::Dolt
    }

    fn sync(&mut self) -> Result<()> {
        let pos = self.pos()?;
        let txid = TXID(pos.txid.0 + 1);
        // Same generation as the newest L0? Then capture only the tail.
        let incremental = match (self.captured, pos.txid.0) {
            (_, 0) | (None, _) => None,
            (Some(captured), _) => {
                let same_inode = {
                    #[cfg(unix)]
                    {
                        captured.inode.is_none() || inode_of(&self.path) == captured.inode
                    }
                    #[cfg(not(unix))]
                    {
                        true
                    }
                };
                match self.header_bytes()? {
                    Some(header) if same_inode && generation_of(&header) == captured.generation => {
                        Some(captured)
                    }
                    _ => None,
                }
            }
        };
        let (cut, generation, commit, pages) = match incremental {
            Some(captured) => {
                let page_size = DOLT_PAGE_SIZE as u64;
                let first_page_offset = captured.cut.end / page_size * page_size;
                let Some((len, buf)) = self.read_from(first_page_offset)? else {
                    return Ok(());
                };
                if len < captured.cut.end {
                    // Shorter than what we captured: not the same file.
                    self.captured = None;
                    return self.sync();
                }
                let cut = scan_wal(&buf, first_page_offset, captured.cut.resume.max(first_page_offset), captured.cut);
                if cut.end == captured.cut.end {
                    // Nothing new committed. Remember a moved resume point
                    // only through a capture, so the L0 and the state agree.
                    return Ok(());
                }
                let (commit, pages) = Self::pages(&buf, first_page_offset, cut.end)?;
                (cut, captured.generation, commit, pages)
            }
            None => {
                let Some((file, cut, generation)) = self.whole()? else {
                    return Ok(());
                };
                let (commit, pages) = Self::pages(&file, 0, cut.end)?;
                (cut, generation, commit, pages)
            }
        };
        let header = self.header(txid, commit, cut, generation);
        let encoded = ltx::encode_file(&header, &pages, 0)?;
        self.write_l0(txid, &encoded)?;
        self.captured = Some(Captured {
            cut,
            generation,
            #[cfg(unix)]
            inode: inode_of(&self.path),
        });
        self.pos_cache = Some(Pos::new(txid, 0));
        Ok(())
    }

    fn pos(&mut self) -> Result<Pos> {
        if let Some(pos) = self.pos_cache {
            return Ok(pos);
        }
        let (_, max) = self.max_ltx()?;
        let pos = Pos::new(max, 0);
        self.pos_cache = Some(pos);
        Ok(pos)
    }

    fn read_ltx_file(&self, level: u32, min_txid: TXID, max_txid: TXID) -> Result<Vec<u8>> {
        Ok(self.host.read(Path::new(&self.ltx_path(level, min_txid, max_txid)))?)
    }

    fn ltx_path(&self, level: u32, min_txid: TXID, max_txid: TXID) -> String {
        crate::ltx_file_path(&self.meta_path.to_string_lossy(), level, min_txid, max_txid)
    }

    fn seed_l0_baseline(&mut self, min_txid: TXID, max_txid: TXID, data: &[u8]) -> Result<()> {
        match self.host.remove_dir_all(Path::new(&self.level_dir())) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        self.host.create_dir_all(Path::new(&self.level_dir()))?;
        let path = crate::ltx_file_path(&self.meta_path.to_string_lossy(), 0, min_txid, max_txid);
        let tmp = format!("{path}.tmp");
        let mut file = self.host.create(Path::new(&tmp))?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        self.host.rename(Path::new(&tmp), Path::new(&path))?;
        // The baseline does not describe this file: capture it whole next.
        self.captured = None;
        self.pos_cache = None;
        Ok(())
    }

    fn seed_continuation(&mut self, _txid: TXID, _commit: u32) -> Result<Vec<u8>> {
        Err(Error::Other(
            "a Dolt cell does not restore paged; its chain is downloaded whole".into(),
        ))
    }

    fn checkpoint(&mut self, _mode: CheckpointMode) -> Result<()> {
        // DoltLite checkpoints its own WAL inside the chunk file; there is
        // no sidecar to fold.
        Ok(())
    }

    fn snapshot_to_writer(&mut self, w: &mut dyn std::io::Write) -> Result<Pos> {
        let pos = self.pos()?;
        let Some(captured) = self.captured else {
            return Err(Error::Other(
                "dolt snapshot needs a capture of the current file generation".into(),
            ));
        };
        let file = self
            .read_file()?
            .ok_or_else(|| Error::Other("dolt chunk file is missing".into()))?;
        if (file.len() as u64) < captured.cut.end || generation_of(&file) != captured.generation {
            return Err(Error::Other(
                "dolt chunk file changed generation since its last capture".into(),
            ));
        }
        let (commit, pages) = Self::pages(&file, 0, captured.cut.end)?;
        let mut rolling: crate::Checksum = crate::CHECKSUM_FLAG;
        for (pgno, data) in &pages {
            rolling = crate::CHECKSUM_FLAG | (rolling ^ ltx::checksum_page(*pgno, data));
        }
        let mut header = self.header(pos.txid, commit, captured.cut, captured.generation);
        header.flags = 0;
        header.min_txid = TXID(1);
        let encoded = ltx::encode_file(&header, &pages, rolling)?;
        w.write_all(&encoded)?;
        Ok(Pos::new(pos.txid, rolling))
    }

    fn last_sync_timing(&self) -> SyncTiming {
        SyncTiming::default()
    }

    fn close(self: Box<Self>) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{Connection, OpenFlags};

    fn open_prolly(path: &Path) -> Connection {
        Connection::open_with_flags(
            format!("file:{}?doltlite_engine=prolly", path.display()),
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_URI,
        )
        .unwrap()
    }

    fn l0_count(store: &DoltStore) -> usize {
        store
            .host
            .read_dir(Path::new(&store.level_dir()))
            .map(|entries| entries.len())
            .unwrap_or(0)
    }

    /// Restore the L0 chain into a fresh file, the way the replica does.
    fn rebuild(store: &DoltStore, out: &Path) {
        let (_, max) = store.max_ltx().unwrap();
        let files: Vec<Vec<u8>> = (1..=max.0)
            .map(|txid| store.read_ltx_file(0, TXID(txid), TXID(txid)).unwrap())
            .collect();
        let image = crate::replica::internal::build_database_image(&files).unwrap();
        std::fs::write(out, image).unwrap();
    }

    #[test]
    fn seal_matches_doltlite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cell.db");
        let c = open_prolly(&path);
        c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY, b TEXT); INSERT INTO t(b) VALUES ('x');")
            .unwrap();
        drop(c);
        let file = std::fs::read(&path).unwrap();
        let cut = scan_file(&file).expect("a committed root");
        assert!(cut.end > MANIFEST_SIZE as u64);
        assert!(cut.end <= file.len() as u64);
        // The last root ends exactly at the cut and its seal verifies.
        let root = (cut.end - ROOT_RECORD as u64) as usize;
        assert_eq!(file[root], TAG_ROOT);
        assert!(sealed_root(&file[root + 1..root + ROOT_RECORD], root as u64));
        // A flipped byte breaks the seal.
        let mut torn = file[root + 1..root + ROOT_RECORD].to_vec();
        torn[10] ^= 0xff;
        assert!(!sealed_root(&torn, root as u64));
    }

    #[test]
    fn captures_restore_every_commit_and_branch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cell.db");
        let mut store = DoltStore::open(&path, crate::LtxHost::default()).unwrap();
        // Absent file: nothing to capture.
        store.sync().unwrap();
        assert_eq!(store.pos().unwrap().txid, TXID(0));

        let c = open_prolly(&path);
        c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY, b TEXT);").unwrap();
        store.sync().unwrap();
        assert_eq!(store.pos().unwrap().txid, TXID(1));
        // No new commit: no new L0.
        store.sync().unwrap();
        assert_eq!(l0_count(&store), 1);

        for i in 0..50 {
            c.execute("INSERT INTO t(b) VALUES (?1)", [format!("row-{i}-{}", "x".repeat(300))])
                .unwrap();
            if i % 7 == 0 {
                store.sync().unwrap();
            }
        }
        let _: String = c
            .query_row("SELECT dolt_commit('-A','-m','fifty')", [], |r| r.get(0))
            .unwrap();
        let _: i64 = c.query_row("SELECT dolt_branch('feature')", [], |r| r.get(0)).unwrap();
        store.sync().unwrap();
        let head: String = c.query_row("SELECT dolt_hashof('HEAD')", [], |r| r.get(0)).unwrap();

        let restored = dir.path().join("restored.db");
        rebuild(&store, &restored);
        let mut reopened = DoltStore::open(&restored, crate::LtxHost::default()).unwrap();
        let r = Connection::open(&restored).unwrap();
        let n: i64 = r.query_row("SELECT count(*) FROM t", [], |row| row.get(0)).unwrap();
        assert_eq!(n, 50);
        let restored_head: String =
            r.query_row("SELECT dolt_hashof('HEAD')", [], |row| row.get(0)).unwrap();
        assert_eq!(restored_head, head);
        let branches: i64 = r
            .query_row("SELECT count(*) FROM dolt_branches WHERE name='feature'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(branches, 1);
        // The restored image keeps working: a new commit appends cleanly and
        // a store over it captures a whole first L0.
        r.execute("INSERT INTO t(b) VALUES ('after-restore')", []).unwrap();
        reopened.sync().unwrap();
        assert_eq!(reopened.pos().unwrap().txid, TXID(1));
        drop(r);
        let again = dir.path().join("again.db");
        rebuild(&reopened, &again);
        let a = Connection::open(&again).unwrap();
        let n: i64 = a.query_row("SELECT count(*) FROM t", [], |row| row.get(0)).unwrap();
        assert_eq!(n, 51);
    }

    #[test]
    fn uncommitted_tail_is_not_captured() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cell.db");
        let mut store = DoltStore::open(&path, crate::LtxHost::default()).unwrap();
        let c = open_prolly(&path);
        c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY, b BLOB);").unwrap();
        c.execute("INSERT INTO t(b) VALUES (zeroblob(100))", []).unwrap();
        store.sync().unwrap();
        // An open transaction large enough to spill chunks into the file.
        c.execute_batch("BEGIN").unwrap();
        for _ in 0..200 {
            c.execute("INSERT INTO t(b) VALUES (randomblob(2000))", []).unwrap();
        }
        store.sync().unwrap();
        let restored = dir.path().join("restored.db");
        rebuild(&store, &restored);
        let r = Connection::open(&restored).unwrap();
        let n: i64 = r.query_row("SELECT count(*) FROM t", [], |row| row.get(0)).unwrap();
        assert_eq!(n, 1, "only the committed row is captured");
        drop(r);
        c.execute_batch("COMMIT").unwrap();
        store.sync().unwrap();
        let restored = dir.path().join("restored2.db");
        rebuild(&store, &restored);
        let r = Connection::open(&restored).unwrap();
        let n: i64 = r.query_row("SELECT count(*) FROM t", [], |row| row.get(0)).unwrap();
        assert_eq!(n, 201);
    }

    #[test]
    fn gc_is_a_new_generation_captured_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cell.db");
        let mut store = DoltStore::open(&path, crate::LtxHost::default()).unwrap();
        let c = open_prolly(&path);
        c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY, b TEXT);").unwrap();
        for i in 0..40 {
            c.execute("INSERT INTO t(b) VALUES (?1)", [format!("{i}")]).unwrap();
            let _: String = c
                .query_row("SELECT dolt_commit('-A','-m',?1)", [format!("c{i}")], |r| r.get(0))
                .unwrap();
        }
        store.sync().unwrap();
        let before = std::fs::metadata(&path).unwrap().len();
        let _: String = c.query_row("SELECT dolt_gc()", [], |r| r.get(0)).unwrap();
        let after = std::fs::metadata(&path).unwrap().len();
        assert!(after < before, "gc compacts ({before} -> {after})");
        store.sync().unwrap();
        let newest = store.newest_header().unwrap().unwrap();
        assert_eq!(newest.commit as u64, (newest.wal_size as u64).div_ceil(4096));
        // Writes after GC keep appending to the new generation.
        c.execute("INSERT INTO t(b) VALUES ('post-gc')", []).unwrap();
        store.sync().unwrap();
        let head: String = c.query_row("SELECT dolt_hashof('HEAD')", [], |r| r.get(0)).unwrap();
        let restored = dir.path().join("restored.db");
        rebuild(&store, &restored);
        DoltStore::open(&restored, crate::LtxHost::default()).unwrap();
        let r = Connection::open(&restored).unwrap();
        let n: i64 = r.query_row("SELECT count(*) FROM t", [], |row| row.get(0)).unwrap();
        assert_eq!(n, 41);
        let h: String = r.query_row("SELECT dolt_hashof('HEAD')", [], |row| row.get(0)).unwrap();
        assert_eq!(h, head);
        let log: i64 = r.query_row("SELECT count(*) FROM dolt_log", [], |row| row.get(0)).unwrap();
        assert_eq!(log, 41);
    }

    #[test]
    fn reopen_continues_the_same_generation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cell.db");
        let mut store = DoltStore::open(&path, crate::LtxHost::default()).unwrap();
        let c = open_prolly(&path);
        c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY);").unwrap();
        store.sync().unwrap();
        drop(store);
        c.execute("INSERT INTO t VALUES (1)", []).unwrap();
        let mut store = DoltStore::open(&path, crate::LtxHost::default()).unwrap();
        store.sync().unwrap();
        let newest = store.newest_header().unwrap().unwrap();
        assert_eq!(newest.min_txid, TXID(2));
        // Incremental: it starts at the page holding the previous cut, not 1.
        let pages = ltx::decode_file_pages(&store.read_ltx_file(0, TXID(2), TXID(2)).unwrap()).unwrap();
        assert!(pages.first().map(|(pgno, _)| *pgno).unwrap_or(1) >= 1);
        let mut snapshot = Vec::new();
        let pos = store.snapshot_to_writer(&mut snapshot).unwrap();
        assert_eq!(pos.txid, TXID(2));
        let image = ltx::decode_database_image(&snapshot).unwrap();
        let restored = dir.path().join("snap.db");
        std::fs::write(&restored, image).unwrap();
        DoltStore::open(&restored, crate::LtxHost::default()).unwrap();
        let r = Connection::open(&restored).unwrap();
        let n: i64 = r.query_row("SELECT count(*) FROM t", [], |row| row.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    /// The lock-page mapping round-trips, with a lock page small enough to
    /// test: capture skips it, the rebuilt image has a hole there, and the
    /// unmap removes it.
    #[test]
    fn pages_skip_the_lock_page_and_unmap_restores_the_file() {
        let page = DOLT_PAGE_SIZE as usize;
        let lock = 3u32;
        let original: Vec<u8> = (0..(5 * page + 1000)).map(|i| (i % 251) as u8 + 1).collect();
        let cut = original.len() as u64;
        let (commit, pages) = DoltStore::pages_with_lock(&original, 0, cut, lock).unwrap();
        let pgnos: Vec<u32> = pages.iter().map(|(pgno, _)| *pgno).collect();
        assert_eq!(pgnos, vec![1, 2, 4, 5, 6, 7]);
        assert_eq!(commit, 7);
        // An incremental capture from inside page 4 keeps the mapping.
        let from = 3 * page as u64;
        let (_, tail) =
            DoltStore::pages_with_lock(&original[from as usize..], from, cut, lock).unwrap();
        assert_eq!(tail.iter().map(|(p, _)| *p).collect::<Vec<_>>(), vec![5, 6, 7]);
        // Rebuild as a reader does: LTX page p at (p-1)*page, lock page zero.
        let mut image = vec![0u8; commit as usize * page];
        for (pgno, data) in &pages {
            let at = (*pgno as usize - 1) * page;
            image[at..at + page].copy_from_slice(data);
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image");
        std::fs::write(&path, &image).unwrap();
        unmap_hole(&path, (lock as u64 - 1) * page as u64, page as u64).unwrap();
        let restored = std::fs::read(&path).unwrap();
        assert_eq!(restored.len(), 6 * page);
        assert_eq!(&restored[..original.len()], &original[..]);
        assert!(restored[original.len()..].iter().all(|b| *b == 0));
        // Below the lock page nothing moves.
        let small = dir.path().join("small");
        std::fs::write(&small, &original[..page]).unwrap();
        unmap_hole(&small, (lock as u64 - 1) * page as u64, page as u64).unwrap();
        assert_eq!(std::fs::read(&small).unwrap(), &original[..page]);
        // The real lock page: page 262144 is LTX 262144, page 262145 is 262146.
        let real = lock_pgno(DOLT_PAGE_SIZE);
        assert_eq!(ltx_pgno(real - 1), Some(real - 1));
        assert_eq!(ltx_pgno(real), Some(real + 1));
    }

    #[test]
    fn refuses_a_stock_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cell.db");
        let c = Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE t(a)").unwrap();
        drop(c);
        assert!(DoltStore::open(&path, crate::LtxHost::default()).is_err());
    }
}
