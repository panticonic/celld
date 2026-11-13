//! The invariants, and the history they are checked against.
//!
//! The checker never reads the modelled world's internals to decide what is
//! true: it is told about grants, acknowledgements, reveals and restores as
//! they happen, and it judges each against the history so far. That keeps a
//! modelling shortcut in the world from silently satisfying an invariant.

use std::collections::{BTreeMap, BTreeSet};

use celld_logic::{CellId, Epoch, NodeId};

pub type WriteId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ViolationKind {
    /// `State::validate` refused the core's own state.
    CoreValidate,
    /// The `StepSafetyObserverSnapshot` census disagrees with itself.
    StepSafety,
    /// The `AuthorityObserverSnapshot` reports authority on a fenced node.
    Authority,
    /// The decision core panicked.
    CorePanic,
    /// Two nodes were granted, or act on, the same (cell, epoch).
    OneWriterPerEpoch,
    /// An ownership epoch went backwards or was granted twice.
    EpochRegression,
    /// A write committed at an epoch after a successor epoch had already been
    /// granted was acknowledged: a deposed writer kept accepting writes. (A
    /// write committed before the takeover may be acknowledged late; that is
    /// a slow response, not a second writer, and `LostAck` judges whether the
    /// successor kept it.)
    StaleAck,
    /// A restore did not contain an acknowledged write.
    LostAck,
    /// A restore did not contain a write a released read-only output had
    /// revealed beyond its runtime's restored baseline.
    LostRevealed,
    /// An output was decided at an epoch after a successor epoch had been
    /// granted, by more than the clock-skew tolerance.
    StaleServe,
    /// The fleet did not serve a cell after faults healed.
    Liveness,
    /// The modelled world contradicted itself (a simulator bug, not a
    /// protocol bug).
    ModelError,
    /// A `State::validate` failure the simulator has already reported and
    /// minimised (see README, "Findings"). Recorded, but it does not stop a
    /// run unless `--strict` asks it to, so a sweep can look past it.
    KnownCoreIssue,
}

impl ViolationKind {
    /// Safety violations are the ones a mutation must produce and a clean
    /// run must never produce. Liveness is reported, but judged separately.
    pub fn is_safety(self) -> bool {
        !matches!(
            self,
            ViolationKind::Liveness | ViolationKind::KnownCoreIssue
        )
    }
}

#[derive(Clone, Debug)]
pub struct Violation {
    pub kind: ViolationKind,
    pub time: u64,
    pub cell: Option<CellId>,
    pub detail: String,
}

#[derive(Clone, Debug)]
struct Acked {
    write: WriteId,
    epoch: Epoch,
    node: NodeId,
    time: u64,
}

#[derive(Clone, Debug)]
struct Restored {
    epoch: Epoch,
    node: NodeId,
    content: BTreeSet<WriteId>,
}

#[derive(Default)]
pub struct Checker {
    grants: BTreeMap<(CellId, Epoch), (NodeId, u64)>,
    max_granted: BTreeMap<CellId, Epoch>,
    record_epoch: BTreeMap<CellId, Epoch>,
    acked: BTreeMap<CellId, Vec<Acked>>,
    revealed: BTreeMap<CellId, Vec<Acked>>,
    restores: BTreeMap<CellId, Vec<Restored>>,
    pub violations: Vec<Violation>,
    pub acks: u64,
    pub reveals: u64,
    pub restores_checked: u64,
    pub grants_seen: u64,
    skew_tolerance_ms: u64,
}

impl Checker {
    pub fn new(skew_tolerance_ms: u64) -> Self {
        Checker {
            skew_tolerance_ms,
            ..Default::default()
        }
    }

    pub fn fail(&mut self, kind: ViolationKind, time: u64, cell: Option<&str>, detail: String) {
        // One report per (kind, cell) is enough; the first is the useful one.
        if self
            .violations
            .iter()
            .any(|v| v.kind == kind && v.cell.as_deref() == cell)
        {
            return;
        }
        self.violations.push(Violation {
            kind,
            time,
            cell: cell.map(str::to_string),
            detail,
        });
    }

    pub fn has_safety_violation(&self) -> bool {
        self.violations.iter().any(|v| v.kind.is_safety())
    }

    /// The bucket accepted an ownership compare-and-swap naming `node` at
    /// `epoch`.
    pub fn on_grant(&mut self, time: u64, cell: &str, epoch: Epoch, node: &str) {
        self.grants_seen += 1;
        let key = (cell.to_string(), epoch);
        if let Some((holder, _)) = self.grants.get(&key) {
            if holder != node {
                self.fail(
                    ViolationKind::OneWriterPerEpoch,
                    time,
                    Some(cell),
                    format!("epoch {epoch} granted to {holder} and again to {node}"),
                );
            } else {
                self.fail(
                    ViolationKind::EpochRegression,
                    time,
                    Some(cell),
                    format!("epoch {epoch} granted to {node} twice"),
                );
            }
            return;
        }
        let max = self.max_granted.entry(cell.to_string()).or_insert(0);
        if epoch <= *max {
            let prior = *max;
            self.fail(
                ViolationKind::EpochRegression,
                time,
                Some(cell),
                format!("epoch {epoch} granted to {node} after epoch {prior}"),
            );
        } else {
            *max = epoch;
        }
        self.grants.insert(key, (node.to_string(), time));
    }

    /// Any successful write of the ownership record.
    pub fn on_record_write(&mut self, time: u64, cell: &str, epoch: Epoch) {
        let current = self.record_epoch.entry(cell.to_string()).or_insert(0);
        if epoch < *current {
            let prior = *current;
            self.fail(
                ViolationKind::EpochRegression,
                time,
                Some(cell),
                format!("ownership record moved from epoch {prior} back to {epoch}"),
            );
        } else {
            *current = epoch;
        }
    }

    /// `node` acts as the holder of `cell` at `epoch` (a core phase that
    /// names an owned epoch, or a write appended to its replica).
    pub fn on_holder(&mut self, time: u64, cell: &str, epoch: Epoch, node: &str, what: &str) {
        match self.grants.get(&(cell.to_string(), epoch)) {
            Some((holder, _)) if holder == node => {}
            Some((holder, _)) => {
                let holder = holder.clone();
                self.fail(
                    ViolationKind::OneWriterPerEpoch,
                    time,
                    Some(cell),
                    format!("{node} {what} at epoch {epoch}, which was granted to {holder}"),
                );
            }
            None => self.fail(
                ViolationKind::OneWriterPerEpoch,
                time,
                Some(cell),
                format!("{node} {what} at epoch {epoch}, which the bucket never granted"),
            ),
        }
    }

    /// The output gate released an acknowledgement for `write`, which was
    /// committed at `committed_at`.
    pub fn on_ack(
        &mut self,
        time: u64,
        cell: &str,
        write: WriteId,
        epoch: Epoch,
        node: &str,
        committed_at: u64,
    ) {
        self.acks += 1;
        let successor = self
            .grants
            .range((cell.to_string(), epoch + 1)..)
            .take_while(|((c, _), _)| c == cell)
            .find(|(_, (_, granted))| *granted < committed_at)
            .map(|((_, e), (n, granted))| (*e, n.clone(), *granted));
        if let Some((e, n, granted)) = successor {
            self.fail(
                ViolationKind::StaleAck,
                time,
                Some(cell),
                format!(
                    "{node} acknowledged w{write}, committed at epoch {epoch} at t={committed_at}, \
                     after epoch {e} was granted to {n} at t={granted}"
                ),
            );
        }
        let record = Acked {
            write,
            epoch,
            node: node.to_string(),
            time,
        };
        self.check_against_restores(cell, &record, ViolationKind::LostAck, "acknowledged");
        self.acked.entry(cell.to_string()).or_default().push(record);
    }

    /// A read-only output was released after observing `writes` beyond its
    /// runtime's restored baseline.
    pub fn on_reveal(
        &mut self,
        time: u64,
        cell: &str,
        writes: &[WriteId],
        epoch: Epoch,
        node: &str,
    ) {
        for &write in writes {
            self.reveals += 1;
            let record = Acked {
                write,
                epoch,
                node: node.to_string(),
                time,
            };
            self.check_against_restores(cell, &record, ViolationKind::LostRevealed, "revealed");
            self.revealed
                .entry(cell.to_string())
                .or_default()
                .push(record);
        }
    }

    fn check_against_restores(
        &mut self,
        cell: &str,
        record: &Acked,
        kind: ViolationKind,
        verb: &str,
    ) {
        let missing: Option<(Epoch, NodeId)> = self.restores.get(cell).and_then(|restores| {
            restores
                .iter()
                .find(|r| r.epoch > record.epoch && !r.content.contains(&record.write))
                .map(|r| (r.epoch, r.node.clone()))
        });
        if let Some((epoch, node)) = missing {
            self.fail(
                kind,
                record.time,
                Some(cell),
                format!(
                    "w{} was {verb} by {} at epoch {} after {node} had already restored epoch \
                     {epoch} without it",
                    record.write, record.node, record.epoch
                ),
            );
        }
    }

    /// A restore completed: `content` is the database `node` will serve at
    /// `epoch`.
    pub fn on_restore(
        &mut self,
        time: u64,
        cell: &str,
        epoch: Epoch,
        node: &str,
        content: &[WriteId],
    ) {
        self.restores_checked += 1;
        let content: BTreeSet<WriteId> = content.iter().copied().collect();
        let mut lost = Vec::new();
        for (list, kind, verb) in [
            (&self.acked, ViolationKind::LostAck, "acknowledged"),
            (&self.revealed, ViolationKind::LostRevealed, "revealed"),
        ] {
            if let Some(a) = list.get(cell).and_then(|writes| {
                writes
                    .iter()
                    .find(|a| a.epoch < epoch && !content.contains(&a.write))
            }) {
                lost.push((
                    kind,
                    format!(
                        "{node} restored epoch {epoch} without w{}, {verb} by {} at epoch {} (t={})",
                        a.write, a.node, a.epoch, a.time
                    ),
                ));
            }
        }
        for (kind, detail) in lost {
            self.fail(kind, time, Some(cell), detail);
        }
        self.restores
            .entry(cell.to_string())
            .or_default()
            .push(Restored {
                epoch,
                node: node.to_string(),
                content,
            });
    }

    /// An output was decided at `decided_at` for a runtime at `epoch` and
    /// later released successfully.
    pub fn on_released(
        &mut self,
        time: u64,
        cell: &str,
        epoch: Epoch,
        node: &str,
        decided_at: u64,
        after_suspension: bool,
    ) {
        let successor = self
            .grants
            .range((cell.to_string(), epoch + 1)..)
            .take_while(|((c, _), _)| c == cell)
            .find(|(_, (_, granted))| granted + self.skew_tolerance_ms < decided_at)
            .map(|((_, e), (n, granted))| (*e, n.clone(), *granted));
        if let Some((e, n, granted)) = successor {
            let kind = if after_suspension {
                ViolationKind::KnownCoreIssue
            } else {
                ViolationKind::StaleServe
            };
            self.fail(
                kind,
                time,
                Some(cell),
                format!(
                    "{node} decided an output at epoch {epoch} at t={decided_at}, after epoch \
                     {e} was granted to {n} at t={granted}"
                ),
            );
        }
    }

    /// Every acknowledged write the run produced, for the final report.
    pub fn acked_writes(&self, cell: &str) -> Vec<WriteId> {
        self.acked
            .get(cell)
            .map(|v| v.iter().map(|a| a.write).collect())
            .unwrap_or_default()
    }
}
