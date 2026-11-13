//! The capture seam between a cell's storage engine and the replication
//! pipeline.
//!
//! Everything above this trait — the L0 directory, the upload, the follower
//! ship, node bundles, compaction, the epoch chain, restore, the handoff
//! snapshot and every durability proof — consumes one thing: a sequence of
//! L0 LTX files with contiguous TXIDs, each a set of fixed-size pages of the
//! cell's file. A [`CellStore`] produces that sequence from one engine's
//! file:
//!
//! - [`crate::Db`] (the "LTX store"): a stock SQLite file in WAL mode. A
//!   capture reads committed WAL frames.
//! - [`crate::dolt::DoltStore`]: a DoltLite prolly chunk file. A capture
//!   reads the bytes appended since the last capture, cut at the end of the
//!   last sealed root record, as the page images that cover them.
//!
//! Because both engines emit the same unit, positions, proofs and the bucket
//! layout are shared, and the output gate cannot tell the engines apart.

use crate::db::{CheckpointMode, SyncTiming};
use crate::{Pos, Result, TXID};

/// The storage engine a cell's file was created with. Fixed for the life of
/// the cell: the file's own magic records it, and a capture refuses a file of
/// the other engine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Engine {
    /// A stock SQLite file (celld's original cell).
    #[default]
    Sqlite,
    /// A DoltLite prolly-tree chunk store (a Dolt cell).
    Dolt,
}

impl Engine {
    pub fn as_str(self) -> &'static str {
        match self {
            Engine::Sqlite => "sqlite",
            Engine::Dolt => "dolt",
        }
    }
}

/// One engine's capture half. Object safe: a [`crate::Replica`] holds a
/// `Box<dyn CellStore>`.
///
/// The contract every implementation keeps:
///
/// - `sync` writes at most one new L0 file, at `pos().txid + 1`, holding
///   every page that changed since the previous L0, and `commit` set to the
///   file's page count. A store with no L0 yet writes a snapshot (`TXID 1`,
///   every page). It never captures a write that has not committed.
/// - A write that committed before `sync` began is in the L0 that `sync`
///   writes (or an earlier one).
/// - `pos` is the newest local L0 (`Pos::ZERO` when there is none).
pub trait CellStore: Send {
    fn engine(&self) -> Engine;

    fn sync(&mut self) -> Result<()>;

    fn pos(&mut self) -> Result<Pos>;

    fn read_ltx_file(&self, level: u32, min_txid: TXID, max_txid: TXID) -> Result<Vec<u8>>;

    /// The local path of one LTX file (for error messages).
    fn ltx_path(&self, level: u32, min_txid: TXID, max_txid: TXID) -> String;

    /// Replace the local L0 directory with one remote L0 file.
    fn seed_l0_baseline(&mut self, min_txid: TXID, max_txid: TXID, data: &[u8]) -> Result<()>;

    /// Continue a paged chain at `txid` (see [`crate::Db::seed_continuation`]).
    fn seed_continuation(&mut self, txid: TXID, commit: u32) -> Result<Vec<u8>>;

    fn checkpoint(&mut self, mode: CheckpointMode) -> Result<()>;

    /// A whole-file snapshot (`MinTXID 1 ..= pos`) for the handoff.
    fn snapshot_to_writer(&mut self, w: &mut dyn std::io::Write) -> Result<Pos>;

    fn last_sync_timing(&self) -> SyncTiming;

    /// Change the TRUNCATE checkpoint threshold; engines without one ignore
    /// it.
    fn set_truncate_page_n(&mut self, _pages: u32) {}

    fn close(self: Box<Self>) -> Result<()>;
}

impl CellStore for crate::Db {
    fn engine(&self) -> Engine {
        Engine::Sqlite
    }

    fn sync(&mut self) -> Result<()> {
        crate::Db::sync(self)
    }

    fn pos(&mut self) -> Result<Pos> {
        crate::Db::pos(self)
    }

    fn read_ltx_file(&self, level: u32, min_txid: TXID, max_txid: TXID) -> Result<Vec<u8>> {
        crate::Db::read_ltx_file(self, level, min_txid, max_txid)
    }

    fn ltx_path(&self, level: u32, min_txid: TXID, max_txid: TXID) -> String {
        crate::Db::ltx_path(self, level, min_txid, max_txid)
    }

    fn seed_l0_baseline(&mut self, min_txid: TXID, max_txid: TXID, data: &[u8]) -> Result<()> {
        crate::Db::seed_l0_baseline(self, min_txid, max_txid, data)
    }

    fn seed_continuation(&mut self, txid: TXID, commit: u32) -> Result<Vec<u8>> {
        crate::Db::seed_continuation(self, txid, commit)
    }

    fn checkpoint(&mut self, mode: CheckpointMode) -> Result<()> {
        crate::Db::checkpoint(self, mode)
    }

    fn snapshot_to_writer(&mut self, w: &mut dyn std::io::Write) -> Result<Pos> {
        let mut w = w;
        crate::Db::snapshot_to_writer(self, &mut w)
    }

    fn last_sync_timing(&self) -> SyncTiming {
        crate::Db::last_sync_timing(self)
    }

    fn set_truncate_page_n(&mut self, pages: u32) {
        self.truncate_page_n = pages;
    }

    fn close(self: Box<Self>) -> Result<()> {
        crate::Db::close(*self)
    }
}
