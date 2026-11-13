//! The modelled world: a linearizable bucket, a lossy peer network, node
//! processes with their own clocks and disks, and a shell per node that
//! executes `celld_logic::Effect`s the way `crates/celld/actor.rs` does and
//! feeds the results back as `celld_logic::Event`s.
//!
//! Every simplification relative to the real shell is listed in this
//! crate's README; the comments here point at the real executor each model
//! follows.

use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};

use celld_logic::log_tier::LogState;
use celld_logic::{
    on_event, AdoptedCell, CasGuard, CasOutcome, CellId, Channel, Config, Effect, Epoch, Event,
    Failure, LeaseCasOutcome, LocalCell, NodeId, NodeLeaseRecord, NodeLeaseSpec, OpId, OwnerRecord,
    OwnershipOnEvict, Phase, ProofSource, RequestError, RequestId, RestoreOutcome, RestoreSpec,
    Route, State, StopCause, Timer,
};

use crate::check::{Checker, ViolationKind, WriteId};
use crate::config::{Durability, Mutation, SimConfig};
use crate::rng::Rng;
use crate::trace::Trace;

const PEER_PROTOCOL: u16 = 1;
const MONO_BASE: u64 = 1_000;
const OPERATION_DEADLINE_MS: u64 = 15_000;
const STOP_RETRY_MS: u64 = 500;
const ADOPT_RETRY_MS: u64 = 1_000;
const BOOT_RECOVERY_RETRY_MS: u64 = 1_000;
const SAMPLE_EVERY_MS: u64 = 500;
const DRAIN_DEADLINE_MS: u64 = 12_000;
const DURABILITY_TIMEOUT_MS: u64 = 10_000;
const UPLOAD_RETRY_MS: u64 = 300;
const TAIL_DRAIN_MS: (u64, u64) = (500, 3_000);

// ---------------------------------------------------------------------------
// The bucket.

#[derive(Clone, Debug)]
struct OwnerObj {
    node: Option<NodeId>,
    epoch: Epoch,
    etag: String,
}

/// `nodes/<node>.json`. The core sees `record`; recovery also reads the
/// session and ensemble the folded log object names.
#[derive(Clone, Debug)]
struct LeaseObj {
    record: NodeLeaseRecord,
    session: Option<u64>,
    ensemble: Vec<NodeId>,
}

#[derive(Default)]
struct Bucket {
    owners: BTreeMap<CellId, OwnerObj>,
    leases: BTreeMap<NodeId, LeaseObj>,
    /// `cells/<cell>/ltx/e<epoch>/`: the restorable image of that epoch. Each
    /// epoch opens with a whole-database snapshot (non-paged LTX), so the
    /// newest non-empty prefix is the restore source.
    ltx: BTreeMap<(CellId, Epoch), Vec<WriteId>>,
    next_etag: u64,
}

impl Bucket {
    fn etag(&mut self) -> String {
        self.next_etag += 1;
        format!("e{}", self.next_etag)
    }
}

// ---------------------------------------------------------------------------
// Nodes.

#[derive(Clone, Debug)]
struct Replica {
    epoch: Epoch,
    content: Vec<WriteId>,
    base_len: usize,
    published: bool,
    /// Closed in place or discarded: no further uploads.
    closed: bool,
}

#[derive(Clone, Debug)]
struct Session {
    id: u64,
    ensemble: Vec<NodeId>,
    /// A lease write carrying this session's open log state has applied, so
    /// a takeover will see it. Fleet proofs wait for this (the node-log
    /// record exists before the first fleet-durable acknowledgement).
    published: bool,
}

#[derive(Clone, Debug)]
enum ProofKind {
    Await,
    Ensure,
}

#[derive(Clone, Debug)]
struct Proof {
    kind: ProofKind,
    cell: CellId,
    epoch: Epoch,
    target: usize,
    position: u64,
    settled: bool,
    /// The replicator's durability timeout: a ticket that no upload or
    /// follower quorum covers by then fails.
    deadline: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ClientKind {
    Write(WriteId),
    Read,
}

#[derive(Clone, Debug)]
struct ClientReq {
    id: u64,
    cell: CellId,
    kind: ClientKind,
    hops: u32,
}

/// One request the core routed `Local`, from handler start to its release.
#[derive(Clone, Debug)]
struct Handler {
    client: ClientReq,
    epoch: Option<Epoch>,
    /// The writes this handler's output reveals beyond the baseline.
    revealed: Vec<WriteId>,
    output_decided_at: Option<u64>,
    finished_early: bool,
}

#[derive(Clone, Debug)]
struct HandoffWaiter {
    cell: CellId,
    released_epoch: Epoch,
    donor: usize,
    donor_inc: u64,
    job: u64,
}

#[derive(Clone, Debug)]
struct AdoptJob {
    op: OpId,
    cell: CellId,
    released_epoch: Epoch,
    rebalance: bool,
    candidates: Vec<usize>,
    deadline: u64,
}

#[derive(Default)]
struct Shell {
    replicas: BTreeMap<CellId, Replica>,
    timers: BTreeMap<String, u64>,
    next_ordinal: u64,
    own_log: Option<LogState>,
    session: Option<Session>,
    /// Confirmed follower ends, per (member, cell, epoch), for the session.
    fleet_acks: BTreeMap<(NodeId, CellId, Epoch), usize>,
    proofs: BTreeMap<OpId, Proof>,
    /// Core request id -> the client request it carries.
    routes: BTreeMap<RequestId, ClientReq>,
    handlers: BTreeMap<RequestId, Handler>,
    handoff_waiters: BTreeMap<RequestId, HandoffWaiter>,
    adopt_jobs: BTreeMap<u64, AdoptJob>,
    abandons: BTreeMap<OpId, bool>,
    halting: bool,
    draining_since: Option<u64>,
}

#[derive(Default, Clone)]
struct Retention {
    sealed: bool,
    segments: BTreeMap<(CellId, Epoch), Vec<WriteId>>,
}

/// What survives a process crash.
#[derive(Default)]
struct Disk {
    /// The leader's own node log: every image it shipped to followers, kept
    /// until the bundle drain has folded it into the per-cell prefix.
    own_log: BTreeMap<(CellId, Epoch), Vec<WriteId>>,
    evicted: BTreeMap<(CellId, Epoch), Vec<WriteId>>,
    live: BTreeSet<(CellId, Epoch)>,
    follower: BTreeMap<u64, Retention>,
}

struct Node {
    id: NodeId,
    incarnation: u64,
    alive: bool,
    /// The process has started and installed (or is installing) its lease.
    serving: bool,
    state: Option<State>,
    shell: Shell,
    disk: Disk,
    wall_offset: i64,
    boot_t: u64,
    /// Intervals in which this incarnation's monotonic clock did not
    /// advance (VM suspensions).
    suspended: Vec<(u64, u64)>,
    paused_until: u64,
}

impl Node {
    fn mono(&self, t: u64) -> u64 {
        let lost: u64 = self
            .suspended
            .iter()
            .map(|(a, b)| t.min(*b).saturating_sub(*a))
            .sum();
        MONO_BASE + t.saturating_sub(self.boot_t).saturating_sub(lost)
    }

    fn wall(&self, t: u64) -> u64 {
        (t as i64 + self.wall_offset).max(0) as u64 + 1_000_000
    }
}

// ---------------------------------------------------------------------------
// The scheduler.

#[derive(Clone, Debug)]
enum StoreOp {
    ReadOwner {
        op: OpId,
        cell: CellId,
    },
    VerifyOwner {
        op: OpId,
        cell: CellId,
        epoch: Epoch,
    },
    CasOwner {
        op: OpId,
        cell: CellId,
        guard: CasGuard,
        epoch: Epoch,
    },
    ReleaseOwner {
        op: OpId,
        cell: CellId,
        epoch: Epoch,
    },
    ReadSelfLease {
        op: OpId,
    },
    CasLease {
        op: OpId,
        guard: CasGuard,
        record: NodeLeaseRecord,
        stamped: Option<LogState>,
        session: Option<Session>,
    },
    ReadNodeLease {
        op: OpId,
        owner: NodeId,
    },
    ReadCapacity {
        op: OpId,
    },
    /// A proof's bucket upload, or the background sync.
    Upload {
        cell: CellId,
        epoch: Epoch,
        content: Vec<WriteId>,
        proof: Option<OpId>,
    },
    Restore {
        op: OpId,
        cell: CellId,
        spec: RestoreSpec,
    },
    /// An eviction's or cleanup's final snapshot.
    StopUpload {
        op: OpId,
        cell: CellId,
        epoch: Epoch,
        cause: StopCause,
        bounded_until: Option<u64>,
    },
    /// The bundle drain folding one retained image into its per-cell prefix.
    DrainUpload {
        cell: CellId,
        epoch: Epoch,
        content: Vec<WriteId>,
    },
    /// Recovery: read the dead session's lease and fence it to Recovering.
    RecoveryFence {
        job: u64,
    },
    /// Recovery: upload what the sealed members returned, then seal the lease.
    RecoveryFinish {
        job: u64,
    },
}

#[derive(Clone, Debug)]
enum StoreFault {
    None,
    Definite,
    AmbiguousBefore,
    AmbiguousAfter,
}

#[derive(Clone, Debug)]
enum NetMsg {
    Ship {
        session: u64,
        owner: usize,
        owner_inc: u64,
        cell: CellId,
        epoch: Epoch,
        content: Vec<WriteId>,
    },
    ShipAck {
        session: u64,
        member: NodeId,
        cell: CellId,
        epoch: Epoch,
        len: usize,
    },
    Seal {
        job: u64,
        session: u64,
        recoverer: usize,
        recoverer_inc: u64,
    },
    SealReply {
        job: u64,
        segments: BTreeMap<(CellId, Epoch), Vec<WriteId>>,
    },
    Forward {
        client: ClientReq,
    },
    Handoff {
        donor: usize,
        donor_inc: u64,
        job: u64,
        cell: CellId,
        released_epoch: Epoch,
    },
    HandoffReply {
        job: u64,
        accepted: Option<AdoptedCell>,
    },
}

#[derive(Clone, Debug)]
enum Task {
    StartRuntime {
        op: OpId,
        cell: CellId,
        epoch: Epoch,
    },
    RunHandler {
        request: RequestId,
    },
    Release {
        request: RequestId,
        ok: bool,
    },
    /// The local capture before the snapshot PUT: the last point at which an
    /// abandon can still give the cell back.
    StopIssue {
        op: OpId,
        cell: CellId,
        epoch: Epoch,
        cause: StopCause,
        bounded_until: Option<u64>,
    },
    StopDone {
        op: OpId,
        cell: CellId,
        epoch: Epoch,
        cause: StopCause,
    },
    ProofUploaded {
        op: OpId,
        ok: bool,
    },
    /// The bundle drain: fold the leader's own retained log into per-cell
    /// prefixes.
    DrainTail,
    StoreRetry {
        op: StoreOp,
    },
    AdoptNext {
        job: u64,
    },
    Boot,
    BootRecovered,
    Sample,
    Sync {
        cell: CellId,
    },
    RecoveryTimeout {
        job: u64,
    },
}

#[derive(Clone, Debug)]
enum Ev {
    Core {
        node: usize,
        inc: u64,
        event: Event,
    },
    Timer {
        node: usize,
        inc: u64,
        slot: String,
        ordinal: u64,
        timer: Timer,
        at_mono: u64,
    },
    Store {
        node: usize,
        inc: u64,
        op: StoreOp,
        fault: StoreFault,
        respond_after: u64,
    },
    Net {
        to: usize,
        msg: NetMsg,
    },
    Task {
        node: usize,
        inc: u64,
        task: Task,
    },
    Client,
    Crash {
        node: usize,
    },
    Restart {
        node: usize,
    },
    Partition,
    BucketCut,
    Pause {
        node: usize,
    },
    Drain {
        node: usize,
    },
    FinalPhase,
    FinalReads,
}

struct Scheduled {
    at: u64,
    seq: u64,
    ev: Ev,
}

impl PartialEq for Scheduled {
    fn eq(&self, other: &Self) -> bool {
        (self.at, self.seq) == (other.at, other.seq)
    }
}
impl Eq for Scheduled {}
impl PartialOrd for Scheduled {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Scheduled {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (other.at, other.seq).cmp(&(self.at, self.seq))
    }
}

#[derive(Clone, Debug)]
struct RecoveryJob {
    node: usize,
    inc: u64,
    owner: NodeId,
    /// `Some(op)` for a takeover's `RecoverNodeLog`; `None` for a booting
    /// node recovering its own predecessor.
    op: Option<OpId>,
    session: Option<u64>,
    ensemble: Vec<NodeId>,
    gathered: BTreeMap<(CellId, Epoch), Vec<WriteId>>,
    sealed_any: bool,
    awaiting: usize,
    done: bool,
}

#[derive(Default, Debug, Clone)]
pub struct Stats {
    pub client_requests: u64,
    pub acked_writes: u64,
    pub failed_requests: u64,
    pub crashes: u64,
    pub halts: u64,
    pub takeovers: u64,
    pub recoveries: u64,
    pub events: u64,
    pub final_reads_ok: u64,
    pub fleet_proofs: u64,
    pub bucket_proofs: u64,
}

pub struct World {
    pub cfg: SimConfig,
    now: u64,
    seq: u64,
    queue: BinaryHeap<Scheduled>,
    rng: Rng,
    fault_rng: Rng,
    nodes: Vec<Node>,
    bucket: Bucket,
    /// Unordered pairs of node indexes that cannot reach each other.
    cut: BTreeSet<(usize, usize)>,
    /// Nodes that cannot reach the bucket.
    bucket_cut: BTreeSet<usize>,
    pub checker: Checker,
    pub trace: Trace,
    pub stats: Stats,
    next_request: RequestId,
    next_client: u64,
    next_write: WriteId,
    next_session: u64,
    next_job: u64,
    recoveries: BTreeMap<u64, RecoveryJob>,
    final_phase: bool,
    final_pending: BTreeSet<CellId>,
    end_at: u64,
}

impl World {
    pub fn new(cfg: SimConfig) -> Self {
        let mut rng = Rng::new(cfg.seed);
        let fault_rng = rng.fork();
        let mut nodes = Vec::new();
        for i in 0..cfg.nodes {
            let skew = cfg.faults.skew_ms as i64;
            let wall_offset = if skew > 0 {
                rng.range(0, 2 * skew as u64) as i64 - skew
            } else {
                0
            };
            nodes.push(Node {
                id: format!("n{i}"),
                incarnation: 0,
                alive: false,
                serving: false,
                state: None,
                shell: Shell::default(),
                disk: Disk::default(),
                wall_offset,
                boot_t: 0,
                suspended: Vec::new(),
                paused_until: 0,
            });
        }
        let trace = Trace::new(cfg.trace);
        let checker = Checker::new(cfg.skew_tolerance_ms());
        let end_at = cfg.duration_ms;
        let mut world = World {
            cfg,
            now: 0,
            seq: 0,
            queue: BinaryHeap::new(),
            rng,
            fault_rng,
            nodes,
            bucket: Bucket::default(),
            cut: BTreeSet::new(),
            bucket_cut: BTreeSet::new(),
            checker,
            trace,
            stats: Stats::default(),
            next_request: 1,
            next_client: 1,
            next_write: 1,
            next_session: 1,
            next_job: 1,
            recoveries: BTreeMap::new(),
            final_phase: false,
            final_pending: BTreeSet::new(),
            end_at,
        };
        for i in 0..world.cfg.nodes {
            let at = world.rng.range(0, 200);
            world.schedule(at, Ev::Restart { node: i });
        }
        world.schedule(300, Ev::Client);
        let f = world.cfg.faults.clone();
        if f.partition_every_ms > 0 {
            let at = world.fault_rng.range(500, f.partition_every_ms);
            world.schedule(at, Ev::Partition);
        }
        if f.bucket_cut_every_ms > 0 {
            let at = world.fault_rng.range(500, f.bucket_cut_every_ms);
            world.schedule(at, Ev::BucketCut);
        }
        for i in 0..world.cfg.nodes {
            if f.crash_every_ms > 0 {
                let at = world.fault_rng.range(1_000, 2 * f.crash_every_ms);
                world.schedule(at, Ev::Crash { node: i });
            }
            if f.pause_every_ms > 0 {
                let at = world.fault_rng.range(1_000, 2 * f.pause_every_ms);
                world.schedule(at, Ev::Pause { node: i });
            }
            if f.drain_every_ms > 0 {
                let at = world.fault_rng.range(2_000, 2 * f.drain_every_ms);
                world.schedule(at, Ev::Drain { node: i });
            }
        }
        let end = world.end_at;
        world.schedule(end, Ev::FinalPhase);
        world
    }

    fn schedule(&mut self, delay: u64, ev: Ev) {
        self.seq += 1;
        self.queue.push(Scheduled {
            at: self.now + delay,
            seq: self.seq,
            ev,
        });
    }

    fn mutation(&self, m: Mutation) -> bool {
        self.cfg.mutation == Some(m)
    }

    fn cells(&self) -> Vec<CellId> {
        (0..self.cfg.cells).map(|i| format!("c{i}")).collect()
    }

    fn node_index(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    fn reachable(&self, a: usize, b: usize) -> bool {
        a == b || !self.cut.contains(&(a.min(b), a.max(b)))
    }

    fn live(&self, node: usize, inc: u64) -> bool {
        let n = &self.nodes[node];
        n.alive && n.incarnation == inc
    }

    // -----------------------------------------------------------------------
    // Running.

    /// Run to completion. Returns once the final recovery sweep settles, the
    /// first safety violation is found, or the time budget runs out.
    pub fn run(&mut self) {
        let hard_stop = self.end_at + 120_000;
        while let Some(Scheduled { at, ev, .. }) = self.queue.pop() {
            if at > hard_stop {
                break;
            }
            self.now = at;
            self.stats.events += 1;
            self.dispatch(ev);
            if self.checker.has_safety_violation() {
                return;
            }
            if self.final_phase && self.final_pending.is_empty() && self.now > self.end_at + 1 {
                return;
            }
        }
        if self.final_phase && !self.final_pending.is_empty() {
            let pending: Vec<_> = self.final_pending.iter().cloned().collect();
            for cell in pending {
                self.checker.fail(
                    ViolationKind::Liveness,
                    self.now,
                    Some(&cell),
                    "no final read of the cell succeeded after faults healed".to_string(),
                );
            }
        }
    }

    fn dispatch(&mut self, ev: Ev) {
        match ev {
            Ev::Core { node, inc, event } => {
                if !self.live(node, inc) {
                    return;
                }
                if self.nodes[node].paused_until > self.now {
                    let delay = self.nodes[node].paused_until - self.now;
                    self.schedule(delay, Ev::Core { node, inc, event });
                    return;
                }
                self.deliver(node, event);
            }
            Ev::Timer {
                node,
                inc,
                slot,
                ordinal,
                timer,
                at_mono,
            } => {
                if !self.live(node, inc) {
                    return;
                }
                if self.nodes[node].shell.timers.get(&slot) != Some(&ordinal) {
                    return;
                }
                let mono = self.nodes[node].mono(self.now);
                let paused = self.nodes[node].paused_until > self.now;
                if mono < at_mono || paused {
                    let delay = (at_mono.saturating_sub(mono))
                        .max(self.nodes[node].paused_until.saturating_sub(self.now));
                    self.schedule(
                        delay.max(1),
                        Ev::Timer {
                            node,
                            inc,
                            slot,
                            ordinal,
                            timer,
                            at_mono,
                        },
                    );
                    return;
                }
                self.nodes[node].shell.timers.remove(&slot);
                let n = &self.nodes[node];
                let event = Event::TimerFired {
                    timer,
                    now_ms: n.wall(self.now),
                    now_mono_ms: n.mono(self.now),
                };
                self.deliver(node, event);
            }
            Ev::Store {
                node,
                inc,
                op,
                fault,
                respond_after,
            } => self.store_apply(node, inc, op, fault, respond_after),
            Ev::Net { to, msg } => {
                if self.nodes[to].paused_until > self.now {
                    let delay = self.nodes[to].paused_until - self.now;
                    self.schedule(delay, Ev::Net { to, msg });
                    return;
                }
                self.net_deliver(to, msg)
            }
            Ev::Task { node, inc, task } => {
                if !self.live(node, inc) {
                    return;
                }
                if self.nodes[node].paused_until > self.now {
                    let delay = self.nodes[node].paused_until - self.now;
                    self.schedule(delay, Ev::Task { node, inc, task });
                    return;
                }
                self.task(node, task);
            }
            Ev::Client => self.client_tick(),
            Ev::Crash { node } => {
                if !self.final_phase {
                    if self.nodes[node].alive {
                        self.crash(node, "crash");
                    }
                    let every = self.cfg.faults.crash_every_ms;
                    let at = self.fault_rng.range(every / 2, every * 3 / 2);
                    self.schedule(at, Ev::Crash { node });
                }
            }
            Ev::Restart { node } => self.restart(node),
            Ev::Partition => self.partition_tick(),
            Ev::BucketCut => self.bucket_cut_tick(),
            Ev::Pause { node } => self.pause_tick(node),
            Ev::Drain { node } => self.drain_tick(node),
            Ev::FinalPhase => self.final_phase_begin(),
            Ev::FinalReads => self.final_reads(),
        }
    }

    // -----------------------------------------------------------------------
    // Process lifecycle.

    fn restart(&mut self, node: usize) {
        if self.nodes[node].alive {
            return;
        }
        let n = &mut self.nodes[node];
        n.incarnation += 1;
        n.alive = true;
        n.serving = false;
        n.boot_t = self.now;
        n.suspended.clear();
        n.paused_until = 0;
        n.shell = Shell::default();
        let config = Config {
            max_resident: self.cfg.max_resident,
            max_activations: 4,
            max_evictions: 2,
            max_releases: 2,
            max_outbound_websockets: 4,
            ownership_on_evict: if self.cfg.sticky {
                OwnershipOnEvict::Sticky
            } else {
                OwnershipOnEvict::Release
            },
            require_node_lease: true,
            peer_protocol: PEER_PROTOCOL,
            operation_deadline_ms: Some(OPERATION_DEADLINE_MS),
            owner_log_recovery_backoff_ms: 1_000,
            owner_log_recovery_attempts: 30,
            alarm_resident_ms: 5_000,
            idle_evict_ms: self.cfg.idle_evict_ms,
            hold_grace_ms: 5_000,
            max_holds_per_cell: 16,
            pressure: Default::default(),
        };
        n.state = Some(State::new(n.id.clone(), config));
        let id = n.id.clone();
        let inc = n.incarnation;
        self.trace.line(
            self.now,
            &id,
            None,
            format!("process start (incarnation {inc})"),
        );
        // `startup.rs`: a node recovers its own predecessor's node log before
        // it installs its lease, so a not-sealed predecessor never blocks the
        // install and no acknowledged tail is stranded on followers.
        self.schedule(
            1,
            Ev::Task {
                node,
                inc,
                task: Task::Boot,
            },
        );
    }

    fn crash(&mut self, node: usize, why: &str) {
        let n = &mut self.nodes[node];
        if !n.alive {
            return;
        }
        n.alive = false;
        n.serving = false;
        n.state = None;
        // Open replicas leave their live database files behind.
        let live: Vec<(CellId, Epoch)> = n
            .shell
            .replicas
            .iter()
            .filter(|(_, r)| !r.closed)
            .map(|(c, r)| (c.clone(), r.epoch))
            .collect();
        n.disk.live.extend(live);
        n.shell = Shell::default();
        let id = n.id.clone();
        self.stats.crashes += 1;
        self.trace
            .line(self.now, &id, None, format!("process exit ({why})"));
        // The supervisor waits at least one lease lifetime (guarantees.md).
        let delay = self.cfg.ttl_ms + self.fault_rng.range(100, 2_000);
        if !self.final_phase {
            self.schedule(delay, Ev::Restart { node });
        }
    }

    fn boot(&mut self, node: usize) {
        let inc = self.nodes[node].incarnation;
        let id = self.nodes[node].id.clone();
        if self.cfg.durability == Durability::Fleet {
            let unrecovered = self.bucket.leases.get(&id).is_some_and(|lease| {
                !matches!(lease.record.log_state, None | Some(LogState::Sealed))
            });
            if unrecovered {
                if self.bucket_cut.contains(&node) {
                    self.schedule(
                        BOOT_RECOVERY_RETRY_MS,
                        Ev::Task {
                            node,
                            inc,
                            task: Task::Boot,
                        },
                    );
                    return;
                }
                self.start_recovery(node, inc, id, None);
                return;
            }
        }
        self.start_lease(node);
    }

    fn start_lease(&mut self, node: usize) {
        let n = &mut self.nodes[node];
        n.serving = true;
        let spec = NodeLeaseSpec {
            addr: format!("addr-{}", n.id),
            peer_protocol: PEER_PROTOCOL,
            generation: format!("{}-g{}", n.id, n.incarnation),
            resume_generation: None,
            ttl_ms: self.cfg.ttl_ms,
        };
        let event = Event::StartNodeLease {
            now_ms: n.wall(self.now),
            now_mono_ms: n.mono(self.now),
            spec,
        };
        let inc = n.incarnation;
        self.deliver(node, event);
        self.schedule(
            SAMPLE_EVERY_MS,
            Ev::Task {
                node,
                inc,
                task: Task::Sample,
            },
        );
    }

    // -----------------------------------------------------------------------
    // Delivering events to the core.

    fn deliver(&mut self, node: usize, first: Event) {
        let mut queue = VecDeque::from([first]);
        while let Some(event) = queue.pop_front() {
            if !self.nodes[node].alive {
                return;
            }
            let id = self.nodes[node].id.clone();
            self.trace
                .line(self.now, &id, event_cell(&event), format!("<- {event:?}"));
            let effects = {
                let state = self.nodes[node]
                    .state
                    .as_mut()
                    .expect("live node has state");
                on_event(state, event)
            };
            self.check_node(node);
            for effect in effects {
                self.trace.line(
                    self.now,
                    &id,
                    effect_cell(&effect),
                    format!("-> {effect:?}"),
                );
                self.execute(node, effect, &mut queue);
            }
            self.settle_handoffs(node, &mut queue);
        }
        if self.nodes[node].shell.halting {
            self.stats.halts += 1;
            self.crash(node, "halt (self-fence)");
        }
    }

    /// The per-step checks that need only one node's core.
    fn check_node(&mut self, node: usize) {
        let now = self.now;
        let id = self.nodes[node].id.clone();
        let state = self.nodes[node].state.as_ref().expect("live");
        if let Err(error) = state.validate() {
            self.checker
                .fail(ViolationKind::CoreValidate, now, None, format!("{id}: {error}"));
        }
        // The StepSafety census: both sides of each structural count.
        let census = state.step_safety_observer_snapshot();
        if census.occupied_counter != census.capacity_holder_ids.len() {
            self.checker.fail(
                ViolationKind::StepSafety,
                now,
                None,
                format!(
                    "{id}: occupied counter {} but {} capacity holders {:?}",
                    census.occupied_counter,
                    census.capacity_holder_ids.len(),
                    census.capacity_holder_ids
                ),
            );
        }
        if census.occupied_counter > self.cfg.max_resident {
            self.checker.fail(
                ViolationKind::StepSafety,
                now,
                None,
                format!(
                    "{id}: occupied {} above max_resident",
                    census.occupied_counter
                ),
            );
        }
        if !census
            .activation_permit_ids
            .is_subset(&census.activation_holder_ids)
        {
            self.checker.fail(
                ViolationKind::StepSafety,
                now,
                None,
                format!(
                    "{id}: activation permits {:?} not all held by activation phases {:?}",
                    census.activation_permit_ids, census.activation_holder_ids
                ),
            );
        }
        let phases = state.phase_snapshot_for_test();
        let mut holders = Vec::new();
        for (cell, phase) in &phases {
            let authority = state.authority_observer_snapshot(cell);
            if authority.fenced && authority.node_authoritative {
                self.checker.fail(
                    ViolationKind::Authority,
                    now,
                    Some(cell),
                    format!("{id}: fenced but reports node authority"),
                );
            }
            if matches!(phase, Phase::Fenced) && !authority.fenced {
                self.checker.fail(
                    ViolationKind::Authority,
                    now,
                    Some(cell),
                    format!("{id}: cell fenced on an unfenced node"),
                );
            }
            if let Some(epoch) = owned_epoch(phase) {
                holders.push((cell.clone(), epoch, format!("{phase:?}")));
            }
        }
        for (cell, epoch, phase) in holders {
            self.checker
                .on_holder(now, &cell, epoch, &id, &format!("is in {phase}"));
        }
    }

    // -----------------------------------------------------------------------
    // Effect executors. Each arm names the `actor.rs` arm it models.

    fn execute(&mut self, node: usize, effect: Effect, immediate: &mut VecDeque<Event>) {
        let inc = self.nodes[node].incarnation;
        match effect {
            Effect::ScheduleTimer { timer, at_mono_ms } => {
                let slot = timer_slot(&timer);
                let shell = &mut self.nodes[node].shell;
                shell.next_ordinal += 1;
                let ordinal = shell.next_ordinal;
                shell.timers.insert(slot.clone(), ordinal);
                let mono = self.nodes[node].mono(self.now);
                let delay = at_mono_ms.saturating_sub(mono);
                self.schedule(
                    delay,
                    Ev::Timer {
                        node,
                        inc,
                        slot,
                        ordinal,
                        timer,
                        at_mono: at_mono_ms,
                    },
                );
            }
            Effect::ReadSelfNodeLease { op } => self.store(node, StoreOp::ReadSelfLease { op }),
            Effect::CasNodeLease {
                op, guard, record, ..
            } => {
                // `ownership_store::cas_node_lease`: the body carries the
                // shell's own folded log slot, not the core's belief.
                let shell = &self.nodes[node].shell;
                let stamped = shell.own_log;
                let session = shell.session.clone();
                self.store(
                    node,
                    StoreOp::CasLease {
                        op,
                        guard,
                        record,
                        stamped,
                        session,
                    },
                );
            }
            Effect::ReadLocalCells => {
                let disk = &self.nodes[node].disk;
                let mut cells: Vec<LocalCell> = disk
                    .live
                    .iter()
                    .map(|(id, epoch)| LocalCell {
                        id: id.clone(),
                        epoch: *epoch,
                        live: true,
                    })
                    .chain(disk.evicted.keys().map(|(id, epoch)| LocalCell {
                        id: id.clone(),
                        epoch: *epoch,
                        live: false,
                    }))
                    .collect();
                cells.sort();
                cells.dedup();
                immediate.push_back(Event::LocalCellsRead { result: Ok(cells) });
            }
            Effect::ReadOwner { op, cell } => self.store(node, StoreOp::ReadOwner { op, cell }),
            Effect::ReadNodeLease { op, owner, .. } => {
                self.store(node, StoreOp::ReadNodeLease { op, owner })
            }
            Effect::RecoverNodeLog { op, owner, .. } => {
                if self.cfg.durability == Durability::Bucket
                    || self.mutation(Mutation::SkipLogRecovery)
                {
                    // No log tier on this node: nothing to recover.
                    let delay = self.rng.range(1, 20);
                    self.schedule(
                        delay,
                        Ev::Core {
                            node,
                            inc,
                            event: Event::NodeLogRecovered { op, result: Ok(()) },
                        },
                    );
                } else {
                    self.stats.recoveries += 1;
                    self.start_recovery(node, inc, owner, Some(op));
                }
            }
            Effect::ReadCapacityPeers { op, .. } => self.store(node, StoreOp::ReadCapacity { op }),
            Effect::CasOwner {
                op,
                cell,
                guard,
                epoch,
                takeover,
            } => {
                if takeover {
                    self.stats.takeovers += 1;
                }
                self.store(
                    node,
                    StoreOp::CasOwner {
                        op,
                        cell,
                        guard,
                        epoch,
                    },
                )
            }
            Effect::ReconcileWakeEntry { .. } => {}
            Effect::ReleaseOwner { op, cell, epoch } => {
                self.store(node, StoreOp::ReleaseOwner { op, cell, epoch })
            }
            Effect::AdoptReleased {
                op,
                cell,
                released_epoch,
                rebalance,
            } => {
                let job = self.next_job;
                self.next_job += 1;
                let mut candidates: Vec<usize> =
                    (0..self.nodes.len()).filter(|i| *i != node).collect();
                // Order candidates deterministically but not by index alone.
                let rot = self.rng.index(candidates.len().max(1));
                let len = candidates.len();
                candidates.rotate_left(rot.min(len));
                let deadline = if rebalance {
                    self.now + OPERATION_DEADLINE_MS
                } else {
                    u64::MAX
                };
                self.nodes[node].shell.adopt_jobs.insert(
                    job,
                    AdoptJob {
                        op,
                        cell,
                        released_epoch,
                        rebalance,
                        candidates,
                        deadline,
                    },
                );
                self.schedule(
                    1,
                    Ev::Task {
                        node,
                        inc,
                        task: Task::AdoptNext { job },
                    },
                );
            }
            Effect::CancelCellActivity { requests, .. } => {
                for request in requests {
                    if self.nodes[node].shell.handlers.remove(&request).is_some() {
                        self.stats.failed_requests += 1;
                    }
                    immediate.push_back(Event::ActivityFinished { request });
                }
            }
            Effect::Restore { op, cell, spec } => {
                self.store(node, StoreOp::Restore { op, cell, spec })
            }
            Effect::StartRuntime { op, cell, epoch } => {
                let delay = self.rng.range(1, 30);
                self.schedule(
                    delay,
                    Ev::Task {
                        node,
                        inc,
                        task: Task::StartRuntime { op, cell, epoch },
                    },
                );
            }
            Effect::Publish { op, cell, epoch } => {
                // Synchronous registry insert (`runtime.publish_cell`).
                let result = match self.nodes[node].shell.replicas.get_mut(&cell) {
                    Some(replica) if replica.epoch == epoch && !replica.closed => {
                        replica.published = true;
                        Ok(())
                    }
                    _ => Err(Failure::Definite),
                };
                immediate.push_back(Event::Published { op, result });
            }
            Effect::EvictionAdmission { .. } | Effect::EvictionFinished { .. } => {}
            Effect::EnsureDurable {
                op, cell, epoch, ..
            } => self.begin_proof(node, op, cell, epoch, None, ProofKind::Ensure, immediate),
            Effect::AwaitDurable {
                op,
                cell,
                epoch,
                position,
            } => self.begin_proof(
                node,
                op,
                cell,
                epoch,
                Some(position),
                ProofKind::Await,
                immediate,
            ),
            Effect::VerifyOwnership { op, cell, epoch } => {
                if self.mutation(Mutation::SkipVerifyOwnership) {
                    immediate.push_back(Event::OwnershipVerified { op, result: Ok(()) });
                } else {
                    self.store(node, StoreOp::VerifyOwner { op, cell, epoch });
                }
            }
            Effect::Release {
                request,
                channel,
                result,
            } => {
                debug_assert_eq!(channel, Channel::Response);
                self.on_release(node, request, result);
            }
            Effect::StopRuntime {
                op,
                cell,
                epoch,
                cause,
                bounded,
            } => self.stop_runtime(node, op, cell, epoch, cause, bounded, immediate),
            Effect::AbandonEvictionStop { op, .. } => {
                if let Some(flag) = self.nodes[node].shell.abandons.get_mut(&op) {
                    *flag = true;
                }
            }
            Effect::FireAlarm {
                op, cell, epoch, ..
            } => {
                // Alarms are not modelled; no cell arms one.
                let n = &self.nodes[node];
                immediate.push_back(Event::AlarmFinished {
                    op,
                    cell,
                    epoch,
                    now_ms: n.wall(self.now),
                    now_mono_ms: n.mono(self.now),
                    result: Err(Failure::Definite),
                });
            }
            Effect::Complete { request, result } => {
                self.on_complete(node, request, result, immediate)
            }
            Effect::CompleteWorker { .. }
            | Effect::CloseWebSocket { .. }
            | Effect::RevokeHolds { .. } => {}
            Effect::Halt { .. } => {
                self.nodes[node].shell.halting = true;
            }
        }
    }

    // -----------------------------------------------------------------------
    // The store.

    fn store(&mut self, node: usize, op: StoreOp) {
        let inc = self.nodes[node].incarnation;
        let f = &self.cfg.faults;
        let mut issue = self.rng.range(1, 15);
        if self.rng.chance(f.store_slow) {
            // A tail request: it can linearize after its caller gave up.
            issue += self.rng.range(self.cfg.ttl_ms, 3 * self.cfg.ttl_ms);
        }
        let respond = self.rng.range(1, 15);
        let fault = if self.bucket_cut.contains(&node) {
            if self.rng.chance(0.5) {
                StoreFault::Definite
            } else {
                StoreFault::AmbiguousBefore
            }
        } else if self.rng.chance(f.store_fail) {
            match self.rng.index(3) {
                0 => StoreFault::Definite,
                1 => StoreFault::AmbiguousBefore,
                _ => StoreFault::AmbiguousAfter,
            }
        } else {
            StoreFault::None
        };
        self.schedule(
            issue,
            Ev::Store {
                node,
                inc,
                op,
                fault,
                respond_after: respond,
            },
        );
    }

    fn respond(&mut self, node: usize, inc: u64, delay: u64, event: Event) {
        self.schedule(delay, Ev::Core { node, inc, event });
    }

    fn store_apply(&mut self, node: usize, inc: u64, op: StoreOp, fault: StoreFault, delay: u64) {
        let applies = matches!(fault, StoreFault::None | StoreFault::AmbiguousAfter);
        // The response's failure class, if the caller does not hear success.
        let failed: Option<Failure> = match fault {
            StoreFault::None => None,
            StoreFault::Definite => Some(Failure::Definite),
            StoreFault::AmbiguousBefore | StoreFault::AmbiguousAfter => Some(Failure::Ambiguous),
        };
        let node_id = self.nodes[node].id.clone();
        // Response clocks are sampled when the response is handled; using the
        // apply-time clock plus the transit delay is the same thing here.
        let resp_t = self.now + delay;
        let wall_at = |w: &World| w.nodes[node].wall(resp_t);
        let mono_at = |w: &World| w.nodes[node].mono(resp_t);
        match op {
            StoreOp::ReadOwner { op, cell } => {
                let result = match failed {
                    // `ownership_read_failed`: every read error is definite.
                    Some(_) => Err(Failure::Definite),
                    None => Ok(self.bucket.owners.get(&cell).map(owner_record)),
                };
                let now_ms = wall_at(self);
                self.respond(node, inc, delay, Event::OwnerRead { op, now_ms, result });
            }
            StoreOp::VerifyOwner { op, cell, epoch } => {
                let result = match failed {
                    Some(f) => Err(f),
                    None => match self.bucket.owners.get(&cell) {
                        Some(o)
                            if o.node.as_deref() == Some(node_id.as_str()) && o.epoch == epoch =>
                        {
                            Ok(())
                        }
                        _ => Err(Failure::Definite),
                    },
                };
                self.respond(node, inc, delay, Event::OwnershipVerified { op, result });
            }
            StoreOp::CasOwner {
                op,
                cell,
                guard,
                epoch,
            } => {
                let mut outcome = CasOutcome::Rejected;
                if applies {
                    let current = self.bucket.owners.get(&cell);
                    let allowed = self.mutation(Mutation::NonConditionalOwnerCas)
                        || match (&guard, current) {
                            (CasGuard::Absent, None) => true,
                            (CasGuard::Match(etag), Some(o)) => &o.etag == etag,
                            _ => false,
                        };
                    if allowed {
                        let etag = self.bucket.etag();
                        self.bucket.owners.insert(
                            cell.clone(),
                            OwnerObj {
                                node: Some(node_id.clone()),
                                epoch,
                                etag,
                            },
                        );
                        self.checker.on_record_write(self.now, &cell, epoch);
                        self.checker.on_grant(self.now, &cell, epoch, &node_id);
                        self.trace.line(
                            self.now,
                            "bucket",
                            Some(&cell),
                            format!("own.json := {node_id}@{epoch}"),
                        );
                        outcome = CasOutcome::Applied;
                    }
                }
                let result = match failed {
                    Some(f) => Err(f),
                    None => Ok(outcome),
                };
                self.respond(node, inc, delay, Event::OwnerCasCompleted { op, result });
            }
            StoreOp::ReleaseOwner { op, cell, epoch } => {
                let mut outcome = CasOutcome::Rejected;
                if applies {
                    let current = self.bucket.owners.get(&cell).cloned();
                    if let Some(o) = current {
                        if o.node.as_deref() == Some(node_id.as_str()) && o.epoch == epoch {
                            let etag = self.bucket.etag();
                            self.bucket.owners.insert(
                                cell.clone(),
                                OwnerObj {
                                    node: None,
                                    epoch,
                                    etag,
                                },
                            );
                            self.checker.on_record_write(self.now, &cell, epoch);
                            self.trace.line(
                                self.now,
                                "bucket",
                                Some(&cell),
                                format!("own.json := released@{epoch}"),
                            );
                            outcome = CasOutcome::Applied;
                        }
                    }
                }
                let result = match failed {
                    Some(f) => Err(f),
                    None => Ok(outcome),
                };
                self.respond(node, inc, delay, Event::OwnerReleased { op, result });
            }
            StoreOp::ReadSelfLease { op } => {
                let result = match failed {
                    Some(_) => Err(Failure::Definite),
                    None => Ok(self.bucket.leases.get(&node_id).map(|l| l.record.clone())),
                };
                let (now_ms, now_mono_ms) = (wall_at(self), mono_at(self));
                self.respond(
                    node,
                    inc,
                    delay,
                    Event::SelfNodeLeaseRead {
                        op,
                        now_ms,
                        now_mono_ms,
                        result,
                    },
                );
            }
            StoreOp::CasLease {
                op,
                guard,
                record,
                stamped,
                session,
            } => {
                let mut outcome = LeaseCasOutcome::Rejected;
                if applies {
                    let current = self.bucket.leases.get(&node_id);
                    let allowed = match (&guard, current) {
                        (CasGuard::Absent, None) => true,
                        (CasGuard::Match(etag), Some(l)) => &l.record.etag == etag,
                        _ => false,
                    };
                    if allowed {
                        let etag = self.bucket.etag();
                        let mut stored = record.clone();
                        stored.etag = etag.clone();
                        stored.log_state = stamped;
                        let (sid, ensemble) = match (&stamped, &session) {
                            (Some(_), Some(s)) => (Some(s.id), s.ensemble.clone()),
                            _ => (None, Vec::new()),
                        };
                        self.bucket.leases.insert(
                            node_id.clone(),
                            LeaseObj {
                                record: stored,
                                session: sid,
                                ensemble,
                            },
                        );
                        outcome = LeaseCasOutcome::Applied { etag };
                        // The log tier learns its record is published.
                        if stamped == Some(LogState::Open) && self.live(node, inc) {
                            if let (Some(mine), Some(s)) =
                                (self.nodes[node].shell.session.as_mut(), session.as_ref())
                            {
                                if mine.id == s.id {
                                    mine.published = true;
                                }
                            }
                        }
                    }
                }
                let result = match failed {
                    Some(f) => Err(f),
                    None => Ok(outcome),
                };
                let now_mono_ms = mono_at(self);
                self.respond(
                    node,
                    inc,
                    delay,
                    Event::NodeLeaseCasCompleted {
                        op,
                        now_mono_ms,
                        result,
                        stamped_log_state: stamped,
                    },
                );
            }
            StoreOp::ReadNodeLease { op, owner } => {
                let result = match failed {
                    Some(_) => Err(Failure::Definite),
                    None => Ok(self.bucket.leases.get(&owner).map(|l| {
                        let mut record = l.record.clone();
                        if self.cfg.mutation == Some(Mutation::LeaseAlwaysExpired) {
                            record.expires_ms = 0;
                        }
                        record
                    })),
                };
                let now_ms = wall_at(self);
                self.respond(
                    node,
                    inc,
                    delay,
                    Event::NodeLeaseRead { op, now_ms, result },
                );
            }
            StoreOp::ReadCapacity { op } => {
                let result = match failed {
                    Some(_) => Err(Failure::Definite),
                    None => Ok(Vec::new()),
                };
                let now_ms = wall_at(self);
                self.respond(
                    node,
                    inc,
                    delay,
                    Event::CapacityPeersRead { op, now_ms, result },
                );
            }
            StoreOp::Upload {
                cell,
                epoch,
                content,
                proof,
            } => {
                if applies {
                    self.put_ltx(&cell, epoch, &content);
                }
                if let Some(op) = proof {
                    let ok = failed.is_none();
                    self.schedule(
                        delay,
                        Ev::Task {
                            node,
                            inc,
                            task: Task::ProofUploaded { op, ok },
                        },
                    );
                }
            }
            StoreOp::Restore { op, cell, spec } => {
                self.restore_apply(node, inc, op, cell, spec, failed, delay)
            }
            StoreOp::StopUpload {
                op,
                cell,
                epoch,
                cause,
                bounded_until,
            } => {
                if !self.live(node, inc) {
                    if applies {
                        // The process died; its in-flight PUT can still land.
                        if let Some(content) = self.stop_content(node, &cell, epoch) {
                            self.put_ltx(&cell, epoch, &content);
                        }
                    }
                    return;
                }
                let content = self.stop_content(node, &cell, epoch);
                match (failed, content) {
                    (None, Some(content)) => {
                        self.put_ltx(&cell, epoch, &content);
                        self.schedule(
                            delay,
                            Ev::Task {
                                node,
                                inc,
                                task: Task::StopDone {
                                    op,
                                    cell,
                                    epoch,
                                    cause,
                                },
                            },
                        );
                    }
                    (_, None) => {
                        self.schedule(
                            delay,
                            Ev::Task {
                                node,
                                inc,
                                task: Task::StopDone {
                                    op,
                                    cell,
                                    epoch,
                                    cause,
                                },
                            },
                        );
                    }
                    (Some(_), Some(content)) => {
                        if applies {
                            self.put_ltx(&cell, epoch, &content);
                        }
                        // `report_stopped_when_released` retries a failed
                        // final sync until its deadline.
                        if bounded_until.is_some_and(|until| self.now >= until) {
                            if let Some(r) = self.nodes[node].shell.replicas.get_mut(&cell) {
                                r.published = false;
                            }
                            self.nodes[node].shell.abandons.remove(&op);
                            self.respond(node, inc, delay, Event::RuntimeStopFailed { op });
                        } else {
                            let retry = StoreOp::StopUpload {
                                op,
                                cell,
                                epoch,
                                cause,
                                bounded_until,
                            };
                            self.schedule_store_retry(node, retry);
                        }
                    }
                }
            }
            StoreOp::DrainUpload {
                cell,
                epoch,
                content,
            } => {
                if applies {
                    self.put_ltx(&cell, epoch, &content);
                }
                // Only a live leader learns its drain landed and trims its log.
                if failed.is_none() && self.live(node, inc) {
                    let own = &mut self.nodes[node].disk.own_log;
                    if own
                        .get(&(cell.clone(), epoch))
                        .is_some_and(|v| v.len() <= content.len())
                    {
                        own.remove(&(cell, epoch));
                    }
                } else if self.live(node, inc) {
                    self.schedule(
                        UPLOAD_RETRY_MS,
                        Ev::Task {
                            node,
                            inc,
                            task: Task::DrainTail,
                        },
                    );
                }
            }
            StoreOp::RecoveryFence { job } => self.recovery_fence(job, failed),
            StoreOp::RecoveryFinish { job } => self.recovery_finish(job, failed),
        }
    }

    fn schedule_store_retry(&mut self, node: usize, op: StoreOp) {
        self.schedule_store_retry_after(node, op, STOP_RETRY_MS);
    }

    fn schedule_store_retry_after(&mut self, node: usize, op: StoreOp, delay: u64) {
        let inc = self.nodes[node].incarnation;
        self.schedule(
            delay,
            Ev::Task {
                node,
                inc,
                task: Task::StoreRetry { op },
            },
        );
    }

    fn stop_content(&self, node: usize, cell: &str, epoch: Epoch) -> Option<Vec<WriteId>> {
        self.nodes[node]
            .shell
            .replicas
            .get(cell)
            .filter(|r| r.epoch == epoch)
            .map(|r| r.content.clone())
    }

    /// An unconditional PUT into an epoch prefix. Within one epoch there is
    /// one replica, so every upload is a prefix of every later one; the
    /// longer image wins, exactly as later LTX files extend a prefix.
    fn put_ltx(&mut self, cell: &str, epoch: Epoch, content: &[WriteId]) {
        let key = (cell.to_string(), epoch);
        let existing = self.bucket.ltx.get(&key).cloned().unwrap_or_default();
        let (short, long) = if existing.len() <= content.len() {
            (&existing[..], content)
        } else {
            (content, &existing[..])
        };
        if long[..short.len()] != *short {
            self.checker.fail(
                ViolationKind::ModelError,
                self.now,
                Some(cell),
                format!(
                    "two different images uploaded into epoch {epoch}: {existing:?} vs {content:?}"
                ),
            );
        }
        if content.len() > existing.len() {
            self.trace.line(
                self.now,
                "bucket",
                Some(cell),
                format!("ltx/e{epoch} := {content:?}"),
            );
            self.bucket.ltx.insert(key, content.to_vec());
        }
    }

    // -----------------------------------------------------------------------
    // Restore (`runtime.restore_cell` -> `LtxRepl::activate_with`).

    #[allow(clippy::too_many_arguments)]
    fn restore_apply(
        &mut self,
        node: usize,
        inc: u64,
        op: OpId,
        cell: CellId,
        spec: RestoreSpec,
        failed: Option<Failure>,
        delay: u64,
    ) {
        if !self.live(node, inc) {
            return;
        }
        if failed.is_some() {
            self.respond(
                node,
                inc,
                delay,
                Event::RestoreCompleted {
                    op,
                    result: Err(Failure::Definite),
                },
            );
            return;
        }
        let node_id = self.nodes[node].id.clone();
        let mut restored = false;
        let content: Vec<WriteId>;
        let previous = celld_logic::restore::previous_epoch_reusable(spec.epoch, spec.took_over)
            .then(|| (cell.clone(), spec.epoch - 1));
        let local = (!spec.fresh && !spec.resume_local)
            .then(|| previous.and_then(|key| self.nodes[node].disk.evicted.remove(&key)))
            .flatten();
        if spec.fresh {
            content = Vec::new();
        } else if let Some(snapshot) = local {
            self.trace.line(
                self.now,
                &node_id,
                Some(&cell),
                "reused local eviction snapshot".into(),
            );
            content = snapshot;
            restored = true;
        } else {
            // This node's own open session may hold an acknowledged tail
            // outside the per-cell prefix; gather it first (the dirty-tail
            // fold). At least one member must answer.
            if let Some(session) = self.nodes[node].shell.session.clone() {
                if !session.ensemble.is_empty() {
                    let mut answered = false;
                    let mut tails = Vec::new();
                    for member in &session.ensemble {
                        let Some(m) = self.node_index(member) else {
                            continue;
                        };
                        if !self.nodes[m].alive || !self.reachable(node, m) {
                            continue;
                        }
                        answered = true;
                        if let Some(ret) = self.nodes[m].disk.follower.get(&session.id) {
                            for ((c, e), seg) in &ret.segments {
                                if *c == cell {
                                    tails.push((*e, seg.clone()));
                                }
                            }
                        }
                    }
                    if !answered {
                        self.respond(
                            node,
                            inc,
                            delay,
                            Event::RestoreCompleted {
                                op,
                                result: Err(Failure::Definite),
                            },
                        );
                        return;
                    }
                    // The fold also reads this leader's own node log.
                    for ((c, e), seg) in &self.nodes[node].disk.own_log {
                        if *c == cell {
                            tails.push((*e, seg.clone()));
                        }
                    }
                    for (epoch, seg) in tails {
                        self.put_ltx(&cell, epoch, &seg);
                    }
                }
            }
            let mut epochs: Vec<Epoch> = self
                .bucket
                .ltx
                .range((cell.clone(), 0)..=(cell.clone(), Epoch::MAX))
                .filter(|(_, v)| !v.is_empty())
                .map(|((_, e), _)| *e)
                .collect();
            if self.mutation(Mutation::RestoreOldestEpoch) {
                epochs.reverse();
            }
            match epochs.last() {
                Some(newest) if *newest >= spec.epoch => {
                    // "refusing to restore from used epoch" (`epoch_chain`).
                    self.respond(
                        node,
                        inc,
                        delay,
                        Event::RestoreCompleted {
                            op,
                            result: Err(Failure::Definite),
                        },
                    );
                    return;
                }
                Some(newest) => {
                    content = self.bucket.ltx[&(cell.clone(), *newest)].clone();
                    restored = true;
                }
                None => content = Vec::new(),
            }
        }
        self.checker
            .on_restore(self.now, &cell, spec.epoch, &node_id, &content);
        self.trace.line(
            self.now,
            &node_id,
            Some(&cell),
            format!("restored e{} = {content:?}", spec.epoch),
        );
        let n = &mut self.nodes[node];
        n.disk.live.insert((cell.clone(), spec.epoch));
        let base_len = content.len();
        n.shell.replicas.insert(
            cell,
            Replica {
                epoch: spec.epoch,
                content,
                base_len,
                published: false,
                closed: false,
            },
        );
        self.respond(
            node,
            inc,
            delay,
            Event::RestoreCompleted {
                op,
                result: Ok(RestoreOutcome {
                    restored,
                    alarm: None,
                }),
            },
        );
    }

    // -----------------------------------------------------------------------
    // Durability proofs (`LtxRepl::await_durable`, `ensure_durable`).

    #[allow(clippy::too_many_arguments)]
    fn begin_proof(
        &mut self,
        node: usize,
        op: OpId,
        cell: CellId,
        epoch: Epoch,
        position: Option<u64>,
        kind: ProofKind,
        immediate: &mut VecDeque<Event>,
    ) {
        let inc = self.nodes[node].incarnation;
        let replica = self.nodes[node]
            .shell
            .replicas
            .get(&cell)
            .filter(|r| r.epoch == epoch && !r.closed)
            .cloned();
        let Some(replica) = replica else {
            // "ltx cell not resident"
            match kind {
                ProofKind::Await => immediate.push_back(Event::DurableReached {
                    op,
                    result: Err(Failure::Ambiguous),
                    source: ProofSource::Bucket,
                }),
                ProofKind::Ensure => immediate.push_back(Event::DurabilityChecked {
                    op,
                    result: Err(Failure::Ambiguous),
                }),
            }
            return;
        };
        let target = replica.content.len();
        let position = position.unwrap_or(target as u64);
        if self.mutation(Mutation::AckBeforeDurable) {
            match kind {
                ProofKind::Await => immediate.push_back(Event::DurableReached {
                    op,
                    result: Ok(position),
                    source: ProofSource::Fleet,
                }),
                ProofKind::Ensure => {
                    immediate.push_back(Event::DurabilityChecked { op, result: Ok(()) })
                }
            }
            return;
        }
        let deadline = self.now + DURABILITY_TIMEOUT_MS;
        self.nodes[node].shell.proofs.insert(
            op,
            Proof {
                kind,
                cell: cell.clone(),
                epoch,
                target,
                position,
                settled: false,
                deadline,
            },
        );
        // The bucket path races every fleet proof.
        let content = replica.content.clone();
        self.store(
            node,
            StoreOp::Upload {
                cell: cell.clone(),
                epoch,
                content: content.clone(),
                proof: Some(op),
            },
        );
        if self.cfg.durability == Durability::Fleet {
            self.ensure_session(node, immediate);
            self.ship(node, inc, &cell, epoch, content);
            self.check_fleet_proofs(node);
        }
    }

    /// Open this process's node-log session on first use: pick the ensemble,
    /// stamp `Open` into the folded log slot, and nudge the lease so the
    /// record is published before any fleet acknowledgement.
    fn ensure_session(&mut self, node: usize, immediate: &mut VecDeque<Event>) {
        if self.nodes[node].shell.session.is_some() {
            return;
        }
        let me = self.nodes[node].id.clone();
        let wall = self.nodes[node].wall(self.now);
        let mut ensemble: Vec<NodeId> = self
            .bucket
            .leases
            .values()
            .filter(|l| l.record.node != me && l.record.expires_ms > wall)
            .map(|l| l.record.node.clone())
            .collect();
        ensemble.truncate(2);
        let id = self.next_session;
        self.next_session += 1;
        let empty = ensemble.is_empty();
        let shell = &mut self.nodes[node].shell;
        shell.session = Some(Session {
            id,
            ensemble: ensemble.clone(),
            published: false,
        });
        if empty {
            // A node without an ensemble acknowledges on bucket proofs only.
            return;
        }
        shell.own_log = Some(LogState::Open);
        self.trace.line(
            self.now,
            &me,
            None,
            format!("node-log session s{id} opens with ensemble {ensemble:?}"),
        );
        let n = &self.nodes[node];
        immediate.push_back(Event::NudgeNodeLease {
            now_ms: n.wall(self.now),
            now_mono_ms: n.mono(self.now),
        });
    }

    fn ship(&mut self, node: usize, inc: u64, cell: &str, epoch: Epoch, content: Vec<WriteId>) {
        let Some(session) = self.nodes[node].shell.session.clone() else {
            return;
        };
        if session.ensemble.is_empty() {
            return;
        }
        // The leader appends to its own node log before it ships.
        let own = self.nodes[node]
            .disk
            .own_log
            .entry((cell.to_string(), epoch))
            .or_default();
        if content.len() > own.len() {
            *own = content.clone();
        }
        let delay = self.rng.range(TAIL_DRAIN_MS.0, TAIL_DRAIN_MS.1);
        self.schedule(
            delay,
            Ev::Task {
                node,
                inc,
                task: Task::DrainTail,
            },
        );
        for member in &session.ensemble {
            let Some(m) = self.node_index(member) else {
                continue;
            };
            self.send(
                node,
                m,
                NetMsg::Ship {
                    session: session.id,
                    owner: node,
                    owner_inc: inc,
                    cell: cell.to_string(),
                    epoch,
                    content: content.clone(),
                },
            );
        }
    }

    fn check_fleet_proofs(&mut self, node: usize) {
        let inc = self.nodes[node].incarnation;
        let shell = &self.nodes[node].shell;
        let Some(session) = shell.session.as_ref() else {
            return;
        };
        if !session.published || session.ensemble.is_empty() {
            return;
        }
        let mut done = Vec::new();
        for (op, proof) in &shell.proofs {
            if proof.settled {
                continue;
            }
            let covered = session.ensemble.iter().all(|member| {
                shell
                    .fleet_acks
                    .get(&(member.clone(), proof.cell.clone(), proof.epoch))
                    .is_some_and(|len| *len >= proof.target)
            });
            if covered {
                done.push(*op);
            }
        }
        for op in done {
            self.settle_proof(node, inc, op, true, ProofSource::Fleet);
        }
    }

    fn settle_proof(&mut self, node: usize, inc: u64, op: OpId, ok: bool, source: ProofSource) {
        let Some(proof) = self.nodes[node].shell.proofs.get_mut(&op) else {
            return;
        };
        if proof.settled {
            return;
        }
        proof.settled = true;
        let proof = proof.clone();
        if ok {
            match source {
                ProofSource::Fleet => self.stats.fleet_proofs += 1,
                ProofSource::Bucket => self.stats.bucket_proofs += 1,
            }
        }
        self.nodes[node].shell.proofs.remove(&op);
        let event = match proof.kind {
            ProofKind::Await => Event::DurableReached {
                op,
                result: if ok {
                    Ok(proof.position)
                } else {
                    Err(Failure::Ambiguous)
                },
                source: if ok { source } else { ProofSource::Bucket },
            },
            ProofKind::Ensure => Event::DurabilityChecked {
                op,
                result: if ok { Ok(()) } else { Err(Failure::Ambiguous) },
            },
        };
        self.schedule(0, Ev::Core { node, inc, event });
    }

    // -----------------------------------------------------------------------
    // Stopping a runtime (`StopRuntime` -> `runtime.stop_cell`).

    #[allow(clippy::too_many_arguments)]
    fn stop_runtime(
        &mut self,
        node: usize,
        op: OpId,
        cell: CellId,
        epoch: Epoch,
        cause: StopCause,
        bounded: bool,
        immediate: &mut VecDeque<Event>,
    ) {
        let inc = self.nodes[node].incarnation;
        // `published.remove(&cell)`: the runtime stops serving at once.
        if let Some(r) = self.nodes[node].shell.replicas.get_mut(&cell) {
            if r.epoch == epoch {
                r.published = false;
            }
        }
        match cause {
            StopCause::Evict { .. } | StopCause::Cleanup => {
                if bounded && matches!(cause, StopCause::Evict { .. }) {
                    self.nodes[node].shell.abandons.insert(op, false);
                }
                let bounded_until = bounded.then_some(self.now + OPERATION_DEADLINE_MS);
                // The handoff snapshot PUT happens after a short local
                // capture; an abandon before that point cancels the stop.
                let delay = self.rng.range(1, 20);
                self.schedule(
                    delay,
                    Ev::Task {
                        node,
                        inc,
                        task: Task::StopIssue {
                            op,
                            cell,
                            epoch,
                            cause,
                            bounded_until,
                        },
                    },
                );
            }
            StopCause::Fence => {
                if let Some(r) = self.nodes[node].shell.replicas.get_mut(&cell) {
                    if r.epoch == epoch {
                        r.closed = true;
                    }
                }
                immediate.push_back(Event::RuntimeStopped { op });
            }
            StopCause::Reset => {
                let shell = &mut self.nodes[node].shell;
                if shell.replicas.get(&cell).is_some_and(|r| r.epoch == epoch) {
                    shell.replicas.remove(&cell);
                }
                self.nodes[node].disk.live.remove(&(cell, epoch));
                immediate.push_back(Event::RuntimeStopped { op });
            }
            StopCause::Swap => immediate.push_back(Event::RuntimeStopped { op }),
        }
    }

    fn stop_done(&mut self, node: usize, op: OpId, cell: CellId, epoch: Epoch, cause: StopCause) {
        let inc = self.nodes[node].incarnation;
        self.nodes[node].shell.abandons.remove(&op);
        // The snapshot is visible; drop the local handle, keeping the closed
        // image only for an idle eviction or a cleanup (`remove_local`).
        let preserve_local = match cause {
            StopCause::Evict { rebalance } => !rebalance,
            StopCause::Cleanup => true,
            _ => false,
        };
        let n = &mut self.nodes[node];
        if n.shell
            .replicas
            .get(&cell)
            .is_some_and(|r| r.epoch == epoch)
        {
            let replica = n.shell.replicas.remove(&cell).expect("checked");
            if preserve_local {
                n.disk
                    .evicted
                    .insert((cell.clone(), epoch), replica.content);
            }
        }
        n.disk.live.remove(&(cell, epoch));
        self.respond(node, inc, 0, Event::RuntimeStopped { op });
    }

    // -----------------------------------------------------------------------
    // Node-log recovery (`interlock.ensure_recovered`, and the boot order).

    fn start_recovery(&mut self, node: usize, inc: u64, owner: NodeId, op: Option<OpId>) {
        let job = self.next_job;
        self.next_job += 1;
        self.recoveries.insert(
            job,
            RecoveryJob {
                node,
                inc,
                owner,
                op,
                session: None,
                ensemble: Vec::new(),
                gathered: BTreeMap::new(),
                sealed_any: false,
                awaiting: 0,
                done: false,
            },
        );
        self.store(node, StoreOp::RecoveryFence { job });
    }

    fn finish_recovery(&mut self, job: u64, ok: bool) {
        let Some(mut r) = self.recoveries.remove(&job) else {
            return;
        };
        if r.done {
            return;
        }
        r.done = true;
        if !self.live(r.node, r.inc) {
            return;
        }
        let owner = r.owner.clone();
        self.trace.line(
            self.now,
            &self.nodes[r.node].id.clone(),
            None,
            format!(
                "node-log recovery of {owner}: {}",
                if ok { "sealed" } else { "failed" }
            ),
        );
        match r.op {
            Some(op) => {
                let result = if ok { Ok(()) } else { Err(Failure::Ambiguous) };
                self.respond(r.node, r.inc, 1, Event::NodeLogRecovered { op, result });
            }
            None => {
                let task = if ok { Task::BootRecovered } else { Task::Boot };
                let delay = if ok { 1 } else { BOOT_RECOVERY_RETRY_MS };
                self.schedule(
                    delay,
                    Ev::Task {
                        node: r.node,
                        inc: r.inc,
                        task,
                    },
                );
            }
        }
    }

    fn recovery_fence(&mut self, job: u64, failed: Option<Failure>) {
        let Some(r) = self.recoveries.get(&job).cloned() else {
            return;
        };
        if failed.is_some() {
            self.finish_recovery(job, false);
            return;
        }
        let Some(lease) = self.bucket.leases.get(&r.owner).cloned() else {
            // No lease: the session never acknowledged past the bucket.
            self.finish_recovery(job, true);
            return;
        };
        match lease.record.log_state {
            None | Some(LogState::Sealed) => {
                self.finish_recovery(job, true);
                return;
            }
            Some(LogState::Open) | Some(LogState::Recovering) => {}
        }
        // Fence the record: the owner's next renewal loses its CAS and its
        // readback sees a foreign log state.
        let mut fenced = lease.clone();
        fenced.record.log_state = Some(LogState::Recovering);
        fenced.record.etag = self.bucket.etag();
        self.bucket.leases.insert(r.owner.clone(), fenced);
        let Some(session) = lease.session else {
            self.finish_recovery(job, true);
            return;
        };
        let recoverer = r.node;
        let rinc = r.inc;
        let entry = self.recoveries.get_mut(&job).expect("present");
        entry.session = Some(session);
        entry.ensemble = lease.ensemble.clone();
        entry.awaiting = lease.ensemble.len();
        if lease.ensemble.is_empty() {
            self.finish_recovery(job, true);
            return;
        }
        for member in &lease.ensemble {
            if let Some(m) = self.node_index(member) {
                self.send(
                    recoverer,
                    m,
                    NetMsg::Seal {
                        job,
                        session,
                        recoverer,
                        recoverer_inc: rinc,
                    },
                );
            }
        }
        // A member that never answers is not proof its data is absent.
        self.schedule(
            2_000,
            Ev::Task {
                node: recoverer,
                inc: rinc,
                task: Task::RecoveryTimeout { job },
            },
        );
    }

    fn recovery_sealed(&mut self, job: u64, segments: BTreeMap<(CellId, Epoch), Vec<WriteId>>) {
        let Some(r) = self.recoveries.get_mut(&job) else {
            return;
        };
        if r.done {
            return;
        }
        r.sealed_any = true;
        for (key, seg) in segments {
            let entry = r.gathered.entry(key).or_default();
            if seg.len() > entry.len() {
                *entry = seg;
            }
        }
        r.awaiting = r.awaiting.saturating_sub(1);
        let node = r.node;
        // Write-all, ack-all: one sealed member certifies every acknowledged
        // frame. Finish as soon as one answered.
        self.store(node, StoreOp::RecoveryFinish { job });
    }

    fn recovery_finish(&mut self, job: u64, failed: Option<Failure>) {
        let Some(r) = self.recoveries.get(&job).cloned() else {
            return;
        };
        if r.done {
            return;
        }
        if failed.is_some() {
            self.finish_recovery(job, false);
            return;
        }
        let mut gathered: Vec<((CellId, Epoch), Vec<WriteId>)> = r
            .gathered
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if r.op.is_none() {
            // A node recovering its own predecessor also reads its local log.
            let own = std::mem::take(&mut self.nodes[r.node].disk.own_log);
            gathered.extend(own);
        }
        for ((cell, epoch), seg) in &gathered {
            self.put_ltx(cell, *epoch, seg);
        }
        if let Some(lease) = self.bucket.leases.get_mut(&r.owner) {
            if lease.session == r.session {
                lease.record.log_state = Some(LogState::Sealed);
                lease.record.etag = format!("s{}", lease.record.etag);
            }
        }
        self.finish_recovery(job, true);
    }

    // -----------------------------------------------------------------------
    // The network.

    fn send(&mut self, from: usize, to: usize, msg: NetMsg) {
        if !self.reachable(from, to) || self.rng.chance(self.cfg.faults.net_drop) {
            return;
        }
        let delay = self.rng.range(1, self.cfg.faults.net_delay_ms.max(1));
        self.schedule(delay, Ev::Net { to, msg });
    }

    fn net_deliver(&mut self, to: usize, msg: NetMsg) {
        if let NetMsg::Forward { client } = msg {
            self.client_arrive(to, client);
            return;
        }
        if !self.nodes[to].alive {
            return;
        }
        match msg {
            NetMsg::Ship {
                session,
                owner,
                owner_inc,
                cell,
                epoch,
                content,
            } => {
                // A follower fsyncs the frame unless it sealed the session.
                let disk = &mut self.nodes[to].disk;
                let ret = disk.follower.entry(session).or_default();
                if ret.sealed && self.cfg.mutation != Some(Mutation::FollowerIgnoresSeal) {
                    return;
                }
                let seg = ret.segments.entry((cell.clone(), epoch)).or_default();
                if content.len() > seg.len() {
                    *seg = content.clone();
                }
                let len = seg.len();
                let member = self.nodes[to].id.clone();
                let _ = owner_inc;
                self.send(
                    to,
                    owner,
                    NetMsg::ShipAck {
                        session,
                        member,
                        cell,
                        epoch,
                        len,
                    },
                );
            }
            NetMsg::ShipAck {
                session,
                member,
                cell,
                epoch,
                len,
            } => {
                let shell = &mut self.nodes[to].shell;
                if shell.session.as_ref().is_none_or(|s| s.id != session) {
                    return;
                }
                let entry = shell.fleet_acks.entry((member, cell, epoch)).or_insert(0);
                *entry = (*entry).max(len);
                self.check_fleet_proofs(to);
            }
            NetMsg::Seal {
                job,
                session,
                recoverer,
                recoverer_inc,
            } => {
                let ret = self.nodes[to].disk.follower.entry(session).or_default();
                ret.sealed = true;
                let segments = ret.segments.clone();
                let _ = recoverer_inc;
                self.send(to, recoverer, NetMsg::SealReply { job, segments });
            }
            NetMsg::SealReply { job, segments, .. } => {
                let Some(r) = self.recoveries.get(&job) else {
                    return;
                };
                if r.node != to || !self.live(r.node, r.inc) {
                    return;
                }
                self.recovery_sealed(job, segments);
            }
            NetMsg::Forward { .. } => unreachable!("handled above"),
            NetMsg::Handoff {
                donor,
                donor_inc,
                job,
                cell,
                released_epoch,
            } => {
                let n = &self.nodes[to];
                if !n.serving || n.shell.draining_since.is_some() {
                    self.send(
                        to,
                        donor,
                        NetMsg::HandoffReply {
                            job,
                            accepted: None,
                        },
                    );
                    return;
                }
                let request = self.next_request;
                self.next_request += 1;
                self.nodes[to].shell.handoff_waiters.insert(
                    request,
                    HandoffWaiter {
                        cell: cell.clone(),
                        released_epoch,
                        donor,
                        donor_inc,
                        job,
                    },
                );
                let n = &self.nodes[to];
                let event = Event::HandoffRequestAt {
                    request,
                    cell,
                    now_ms: n.wall(self.now),
                    now_mono_ms: n.mono(self.now),
                };
                self.deliver(to, event);
            }
            NetMsg::HandoffReply { job, accepted } => {
                let inc = self.nodes[to].incarnation;
                let Some(adopt) = self.nodes[to].shell.adopt_jobs.get(&job).cloned() else {
                    return;
                };
                match accepted {
                    Some(cell) if cell.epoch > adopt.released_epoch => {
                        self.nodes[to].shell.adopt_jobs.remove(&job);
                        self.respond(
                            to,
                            inc,
                            0,
                            Event::SuccessorAdopted {
                                op: adopt.op,
                                result: Ok(cell),
                            },
                        );
                    }
                    _ => self.schedule(
                        1,
                        Ev::Task {
                            node: to,
                            inc,
                            task: Task::AdoptNext { job },
                        },
                    ),
                }
            }
        }
    }

    /// `settle_handoff_accepts`: answer a donor once this node holds the
    /// released cell at a newer epoch.
    fn settle_handoffs(&mut self, node: usize, immediate: &mut VecDeque<Event>) {
        let Some(state) = self.nodes[node].state.as_ref() else {
            return;
        };
        let accepted: Vec<(RequestId, Epoch)> = self.nodes[node]
            .shell
            .handoff_waiters
            .iter()
            .filter_map(|(request, w)| {
                state
                    .accepted_epoch(&w.cell)
                    .filter(|epoch| *epoch > w.released_epoch)
                    .map(|epoch| (*request, epoch))
            })
            .collect();
        for (request, epoch) in accepted {
            let w = self.nodes[node]
                .shell
                .handoff_waiters
                .remove(&request)
                .expect("present");
            let me = self.nodes[node].id.clone();
            let adopted = AdoptedCell {
                node: me.clone(),
                addr: format!("addr-{me}"),
                epoch,
                peer_protocol: PEER_PROTOCOL,
            };
            let _ = w.donor_inc;
            self.send(
                node,
                w.donor,
                NetMsg::HandoffReply {
                    job: w.job,
                    accepted: Some(adopted),
                },
            );
            immediate.push_back(Event::Cancel { request });
        }
    }

    // -----------------------------------------------------------------------
    // Clients and handlers.

    fn client_tick(&mut self) {
        if self.final_phase {
            return;
        }
        let gap = self.rng.range(1, 2 * self.cfg.request_every_ms);
        self.schedule(gap, Ev::Client);
        let cells = self.cells();
        let cell = cells[self.rng.index(cells.len())].clone();
        let kind = if self.rng.chance(0.7) {
            let w = self.next_write;
            self.next_write += 1;
            ClientKind::Write(w)
        } else {
            ClientKind::Read
        };
        let id = self.next_client;
        self.next_client += 1;
        self.stats.client_requests += 1;
        let ingress = self.rng.index(self.nodes.len());
        self.schedule(
            0,
            Ev::Net {
                to: ingress,
                msg: NetMsg::Forward {
                    client: ClientReq {
                        id,
                        cell,
                        kind,
                        hops: 0,
                    },
                },
            },
        );
    }

    fn client_arrive(&mut self, node: usize, client: ClientReq) {
        let n = &self.nodes[node];
        if !n.alive || !n.serving {
            self.client_failed(&client, "ingress down");
            return;
        }
        let request = self.next_request;
        self.next_request += 1;
        self.nodes[node]
            .shell
            .routes
            .insert(request, client.clone());
        let n = &self.nodes[node];
        let event = Event::RequestAt {
            request,
            cell: client.cell.clone(),
            now_mono_ms: n.mono(self.now),
        };
        self.deliver(node, event);
    }

    fn client_failed(&mut self, client: &ClientReq, why: &str) {
        self.stats.failed_requests += 1;
        if self.cfg.trace {
            self.trace.line(
                self.now,
                "client",
                Some(&client.cell),
                format!("request {} failed: {why}", client.id),
            );
        }
    }

    fn on_complete(
        &mut self,
        node: usize,
        request: RequestId,
        result: Result<Route, RequestError>,
        immediate: &mut VecDeque<Event>,
    ) {
        // A synthetic handoff request: answer the donor.
        if let Some(w) = self.nodes[node]
            .shell
            .handoff_waiters
            .get(&request)
            .cloned()
        {
            match result {
                Ok(Route::Local) => {
                    // Demand joined; the accept settles from `accepted_epoch`.
                    immediate.push_back(Event::ActivityFinished { request });
                }
                _ => {
                    self.nodes[node].shell.handoff_waiters.remove(&request);
                    self.send(
                        node,
                        w.donor,
                        NetMsg::HandoffReply {
                            job: w.job,
                            accepted: None,
                        },
                    );
                }
            }
            return;
        }
        let Some(client) = self.nodes[node].shell.routes.remove(&request) else {
            return;
        };
        match result {
            Ok(Route::Local) => {
                let inc = self.nodes[node].incarnation;
                self.nodes[node].shell.handlers.insert(
                    request,
                    Handler {
                        client,
                        epoch: None,
                        revealed: Vec::new(),
                        output_decided_at: None,
                        finished_early: false,
                    },
                );
                let delay = self.rng.range(0, 20);
                self.schedule(
                    delay,
                    Ev::Task {
                        node,
                        inc,
                        task: Task::RunHandler { request },
                    },
                );
            }
            Ok(Route::Remote { node: owner, .. }) => {
                let Some(target) = self.node_index(&owner) else {
                    self.client_failed(&client, "unknown owner");
                    return;
                };
                if client.hops >= 3 || target == node {
                    self.client_failed(&client, "too many hops");
                    return;
                }
                let mut client = client;
                client.hops += 1;
                self.send(node, target, NetMsg::Forward { client });
            }
            Err(_) => self.client_failed(&client, "route error"),
        }
    }

    /// The handler runs against the published runtime, commits (for a
    /// write), and raises its response on the output gate.
    fn run_handler(&mut self, node: usize, request: RequestId) {
        let Some(handler) = self.nodes[node].shell.handlers.get(&request).cloned() else {
            return;
        };
        let cell = handler.client.cell.clone();
        let node_id = self.nodes[node].id.clone();
        let replica = self.nodes[node]
            .shell
            .replicas
            .get(&cell)
            .filter(|r| r.published && !r.closed)
            .cloned();
        let Some(replica) = replica else {
            // `fetch_cell` finds no published runtime: the handler fails.
            self.nodes[node].shell.handlers.remove(&request);
            self.client_failed(&handler.client, "no runtime");
            self.deliver(node, Event::ActivityFinished { request });
            return;
        };
        let epoch = replica.epoch;
        let (position, observed, revealed) = match handler.client.kind {
            ClientKind::Write(w) => {
                self.checker
                    .on_holder(self.now, &cell, epoch, &node_id, "appended a write");
                let r = self.nodes[node]
                    .shell
                    .replicas
                    .get_mut(&cell)
                    .expect("present");
                r.content.push(w);
                let len = r.content.len();
                self.trace.line(
                    self.now,
                    &node_id,
                    Some(&cell),
                    format!("commit w{w} at e{epoch} position {len}"),
                );
                // The paced background sync uploads this later.
                let inc = self.nodes[node].incarnation;
                let delay = self.rng.range(50, 1_500);
                self.schedule(
                    delay,
                    Ev::Task {
                        node,
                        inc,
                        task: Task::Sync { cell: cell.clone() },
                    },
                );
                (Some(len as u64), None, Vec::new())
            }
            ClientKind::Read => {
                let len = replica.content.len();
                let observed = (len > replica.base_len).then_some(len as u64);
                (None, observed, replica.content[replica.base_len..].to_vec())
            }
        };
        if let Some(h) = self.nodes[node].shell.handlers.get_mut(&request) {
            h.epoch = Some(epoch);
            h.revealed = revealed;
            h.output_decided_at = Some(self.now);
        }
        let early = self.rng.chance(0.1);
        let n = &self.nodes[node];
        let event = Event::OutputAt {
            request,
            channel: Channel::Response,
            position,
            observed,
            epoch: None,
            now_mono_ms: n.mono(self.now),
        };
        self.deliver(node, event);
        if early && self.nodes[node].alive {
            // The activity can end before its gated output settles (the
            // gate-pinned path).
            if let Some(h) = self.nodes[node].shell.handlers.get_mut(&request) {
                h.finished_early = true;
                self.deliver(node, Event::ActivityFinished { request });
            }
        }
    }

    fn on_release(&mut self, node: usize, request: RequestId, result: Result<(), RequestError>) {
        let inc = self.nodes[node].incarnation;
        // The release reaches the client after a hop; record the decision
        // now, because the decision is what the core is accountable for.
        let Some(handler) = self.nodes[node].shell.handlers.get(&request).cloned() else {
            return;
        };
        let node_id = self.nodes[node].id.clone();
        let cell = handler.client.cell.clone();
        if result.is_ok() {
            if let Some(epoch) = handler.epoch {
                match handler.client.kind {
                    ClientKind::Write(w) => {
                        self.stats.acked_writes += 1;
                        self.trace.line(
                            self.now,
                            &node_id,
                            Some(&cell),
                            format!("ACK w{w} at e{epoch}"),
                        );
                        let committed = handler.output_decided_at.unwrap_or(self.now);
                        self.checker
                            .on_ack(self.now, &cell, w, epoch, &node_id, committed);
                    }
                    ClientKind::Read => {
                        if !handler.revealed.is_empty() {
                            self.checker.on_reveal(
                                self.now,
                                &cell,
                                &handler.revealed,
                                epoch,
                                &node_id,
                            );
                        }
                    }
                }
                if let Some(decided) = handler.output_decided_at {
                    self.checker
                        .on_released(self.now, &cell, epoch, &node_id, decided);
                }
            }
        }
        self.schedule(
            0,
            Ev::Task {
                node,
                inc,
                task: Task::Release {
                    request,
                    ok: result.is_ok(),
                },
            },
        );
    }

    fn release_task(&mut self, node: usize, request: RequestId, ok: bool) {
        let Some(handler) = self.nodes[node].shell.handlers.remove(&request) else {
            return;
        };
        if ok {
            if handler.client.id == 0 {
                self.final_read_done(&handler.client.cell);
            }
        } else {
            self.client_failed(&handler.client, "released with error");
        }
        if !handler.finished_early {
            self.deliver(node, Event::ActivityFinished { request });
        }
    }

    // -----------------------------------------------------------------------
    // Tasks.

    fn task(&mut self, node: usize, task: Task) {
        let inc = self.nodes[node].incarnation;
        match task {
            Task::Boot => self.boot(node),
            Task::BootRecovered => self.start_lease(node),
            Task::StartRuntime { op, cell, epoch } => {
                let ok = self.nodes[node]
                    .shell
                    .replicas
                    .get(&cell)
                    .is_some_and(|r| r.epoch == epoch && !r.closed);
                let result = if ok { Ok(()) } else { Err(Failure::Definite) };
                self.deliver(
                    node,
                    Event::RuntimeStarted {
                        op,
                        isolate: None,
                        generation: 0,
                        result,
                    },
                );
            }
            Task::RunHandler { request } => self.run_handler(node, request),
            Task::Release { request, ok } => self.release_task(node, request, ok),
            Task::StopDone {
                op,
                cell,
                epoch,
                cause,
            } => self.stop_done(node, op, cell, epoch, cause),
            Task::AdoptNext { job } => self.adopt_next(node, job),
            Task::Sample => self.sample(node),
            Task::ProofUploaded { op, ok } => {
                if ok {
                    self.settle_proof(node, inc, op, true, ProofSource::Bucket);
                    return;
                }
                // A failed upload is retried until the durability timeout,
                // while the fleet path may still answer first.
                let Some(proof) = self.nodes[node].shell.proofs.get(&op).cloned() else {
                    return;
                };
                let replica = self.nodes[node]
                    .shell
                    .replicas
                    .get(&proof.cell)
                    .filter(|r| r.epoch == proof.epoch && !r.closed)
                    .cloned();
                match replica {
                    Some(r) if self.now < proof.deadline => {
                        let content = r.content[..proof.target.min(r.content.len())].to_vec();
                        let retry = StoreOp::Upload {
                            cell: proof.cell,
                            epoch: proof.epoch,
                            content,
                            proof: Some(op),
                        };
                        self.schedule_store_retry_after(node, retry, UPLOAD_RETRY_MS);
                    }
                    _ => self.settle_proof(node, inc, op, false, ProofSource::Bucket),
                }
            }
            Task::DrainTail => self.drain_tail(node),
            Task::StoreRetry { op } => self.store(node, op),
            Task::StopIssue {
                op,
                cell,
                epoch,
                cause,
                bounded_until,
            } => {
                // An abandon that arrived before the snapshot PUT cancels the
                // stop, and the core restarts the cell on its open database.
                if self.nodes[node].shell.abandons.remove(&op) == Some(true) {
                    self.deliver(node, Event::RuntimeStopCancelled { op });
                    return;
                }
                self.store(
                    node,
                    StoreOp::StopUpload {
                        op,
                        cell,
                        epoch,
                        cause,
                        bounded_until,
                    },
                );
            }
            Task::Sync { cell } => {
                // The background sync loop: upload the open replica.
                let replica = self.nodes[node]
                    .shell
                    .replicas
                    .get(&cell)
                    .filter(|r| !r.closed)
                    .cloned();
                if let Some(r) = replica {
                    self.store(
                        node,
                        StoreOp::Upload {
                            cell,
                            epoch: r.epoch,
                            content: r.content,
                            proof: None,
                        },
                    );
                }
            }
            Task::RecoveryTimeout { job } => {
                if self.recoveries.get(&job).is_some_and(|r| !r.sealed_any) {
                    self.finish_recovery(job, false);
                }
            }
        }
    }

    fn drain_tail(&mut self, node: usize) {
        let pending: Vec<((CellId, Epoch), Vec<WriteId>)> = self.nodes[node]
            .disk
            .own_log
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for ((cell, epoch), content) in pending {
            self.store(
                node,
                StoreOp::DrainUpload {
                    cell,
                    epoch,
                    content,
                },
            );
        }
    }

    fn adopt_next(&mut self, node: usize, job: u64) {
        let inc = self.nodes[node].incarnation;
        let Some(adopt) = self.nodes[node].shell.adopt_jobs.get_mut(&job) else {
            return;
        };
        if self.now >= adopt.deadline {
            let op = adopt.op;
            self.nodes[node].shell.adopt_jobs.remove(&job);
            self.respond(
                node,
                inc,
                0,
                Event::SuccessorAdopted {
                    op,
                    result: Err(Failure::Definite),
                },
            );
            return;
        }
        if adopt.candidates.is_empty() {
            // No peer can accept yet: retry the whole candidate list.
            let mut all: Vec<usize> = (0..self.nodes.len()).filter(|i| *i != node).collect();
            let rot = self.rng.index(all.len().max(1));
            let len = all.len();
            all.rotate_left(rot.min(len));
            let adopt = self.nodes[node]
                .shell
                .adopt_jobs
                .get_mut(&job)
                .expect("present");
            adopt.candidates = all;
            self.schedule(
                ADOPT_RETRY_MS,
                Ev::Task {
                    node,
                    inc,
                    task: Task::AdoptNext { job },
                },
            );
            return;
        }
        let target = adopt.candidates.remove(0);
        let (cell, released_epoch, _rebalance) =
            (adopt.cell.clone(), adopt.released_epoch, adopt.rebalance);
        self.send(
            node,
            target,
            NetMsg::Handoff {
                donor: node,
                donor_inc: inc,
                job,
                cell,
                released_epoch,
            },
        );
        // An unanswered attempt moves on after a while.
        self.schedule(
            1_500,
            Ev::Task {
                node,
                inc,
                task: Task::AdoptNext { job },
            },
        );
    }

    fn sample(&mut self, node: usize) {
        let inc = self.nodes[node].incarnation;
        self.schedule(
            SAMPLE_EVERY_MS,
            Ev::Task {
                node,
                inc,
                task: Task::Sample,
            },
        );
        if let Some(since) = self.nodes[node].shell.draining_since {
            let state = self.nodes[node].state.as_ref().expect("live");
            let done = state.residents().is_empty() && state.handoffs_in_flight() == 0;
            if done || self.now >= since + DRAIN_DEADLINE_MS {
                self.crash(node, "drained shutdown");
            }
            return;
        }
        let resident_cells = self.nodes[node]
            .state
            .as_ref()
            .expect("live")
            .residents()
            .len();
        let n = &self.nodes[node];
        let load = celld_logic::pressure::Load {
            resident_cells,
            rss_bytes: 0,
            in_use_bytes: 0,
            cgroup_working_set_bytes: None,
            cgroup_current_bytes: None,
            container_reserved_bytes: 0,
        };
        let event = Event::LoadSampled {
            load,
            now_mono_ms: n.mono(self.now),
        };
        self.deliver(node, event);
        if !self.nodes[node].alive || self.final_phase {
            return;
        }
        if self.rng.chance(0.05) {
            let residents = self.nodes[node].state.as_ref().expect("live").residents();
            if !residents.is_empty() {
                let cell = residents[self.rng.index(residents.len())].clone();
                self.deliver(node, Event::Evict { cell });
            }
        }
        if self.nodes[node].alive && self.rng.chance(0.02) {
            self.deliver(node, Event::Rebalance { cells: 1 });
        }
    }

    // -----------------------------------------------------------------------
    // Fault injectors.

    fn partition_tick(&mut self) {
        if self.final_phase {
            self.cut.clear();
            return;
        }
        self.cut.clear();
        if self.fault_rng.chance(0.6) && self.nodes.len() >= 2 {
            let a = self.fault_rng.index(self.nodes.len());
            for b in 0..self.nodes.len() {
                if b != a && self.fault_rng.chance(0.7) {
                    self.cut.insert((a.min(b), a.max(b)));
                }
            }
        }
        let cut = self.cut.clone();
        self.trace
            .line(self.now, "net", None, format!("partition {cut:?}"));
        let every = self.cfg.faults.partition_every_ms;
        let at = self.fault_rng.range(every / 2, every * 3 / 2);
        self.schedule(at, Ev::Partition);
    }

    fn bucket_cut_tick(&mut self) {
        if self.final_phase {
            self.bucket_cut.clear();
            return;
        }
        self.bucket_cut.clear();
        if self.fault_rng.chance(0.5) {
            let a = self.fault_rng.index(self.nodes.len());
            self.bucket_cut.insert(a);
        }
        let cut = self.bucket_cut.clone();
        self.trace
            .line(self.now, "net", None, format!("bucket cut {cut:?}"));
        let every = self.cfg.faults.bucket_cut_every_ms;
        let at = self.fault_rng.range(every / 2, every * 3 / 2);
        self.schedule(at, Ev::BucketCut);
    }

    fn pause_tick(&mut self, node: usize) {
        if self.final_phase {
            return;
        }
        let every = self.cfg.faults.pause_every_ms;
        let at = self.fault_rng.range(every / 2, every * 3 / 2);
        self.schedule(at, Ev::Pause { node });
        if !self.nodes[node].alive {
            return;
        }
        let length = self
            .fault_rng
            .range(self.cfg.ttl_ms / 2, 2 * self.cfg.ttl_ms);
        let n = &mut self.nodes[node];
        n.paused_until = self.now + length;
        let stops_mono = self.cfg.faults.pause_stops_mono
            && self.cfg.mutation == Some(Mutation::SuspendStopsMonotonic);
        if stops_mono {
            n.suspended.push((self.now, self.now + length));
        }
        let id = n.id.clone();
        let kind = if stops_mono {
            "suspend (monotonic clock stops)"
        } else {
            "pause"
        };
        self.trace
            .line(self.now, &id, None, format!("{kind} for {length} ms"));
    }

    fn drain_tick(&mut self, node: usize) {
        if self.final_phase {
            return;
        }
        let every = self.cfg.faults.drain_every_ms;
        let at = self.fault_rng.range(every / 2, every * 3 / 2);
        self.schedule(at, Ev::Drain { node });
        let n = &self.nodes[node];
        if !n.alive || !n.serving || n.shell.draining_since.is_some() {
            return;
        }
        self.nodes[node].shell.draining_since = Some(self.now);
        let inc = self.nodes[node].incarnation;
        self.schedule(
            0,
            Ev::Core {
                node,
                inc,
                event: Event::ReleaseAll,
            },
        );
    }

    // -----------------------------------------------------------------------
    // The end of a run: heal everything, crash every node at once, restart,
    // and read every cell. Each read restores from the bucket and the
    // recovered node logs, and the restore check compares that state with
    // every acknowledged write.

    fn final_phase_begin(&mut self) {
        self.final_phase = true;
        self.cut.clear();
        self.bucket_cut.clear();
        // Let in-flight work settle with faults off.
        self.cfg.faults.net_drop = 0.0;
        self.cfg.faults.store_fail = 0.0;
        self.cfg.faults.store_slow = 0.0;
        self.trace.line(
            self.now,
            "sim",
            None,
            "final phase: heal, crash all, restart, read every cell".into(),
        );
        for node in 0..self.nodes.len() {
            self.nodes[node].paused_until = 0;
            if self.nodes[node].alive {
                self.crash(node, "final crash");
            }
        }
        for node in 0..self.nodes.len() {
            let delay = self.cfg.ttl_ms + 100 + 50 * node as u64;
            self.schedule(delay, Ev::Restart { node });
        }
        self.final_pending = self.cells().into_iter().collect();
        self.schedule(2 * self.cfg.ttl_ms + 1_000, Ev::FinalReads);
    }

    fn final_reads(&mut self) {
        if self.final_pending.is_empty() {
            return;
        }
        // Final reads retry until they succeed or the time budget runs out.
        self.schedule(700, Ev::FinalReads);
        let pending: Vec<CellId> = self.final_pending.iter().cloned().collect();
        for cell in pending {
            let ingress = self.rng.index(self.nodes.len());
            let client = ClientReq {
                id: 0,
                cell,
                kind: ClientKind::Read,
                hops: 0,
            };
            self.schedule(
                0,
                Ev::Net {
                    to: ingress,
                    msg: NetMsg::Forward { client },
                },
            );
        }
    }

    fn final_read_done(&mut self, cell: &str) {
        if self.final_pending.remove(cell) {
            self.stats.final_reads_ok += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers.

fn owner_record(o: &OwnerObj) -> OwnerRecord {
    OwnerRecord {
        node: o.node.clone(),
        epoch: o.epoch,
        etag: o.etag.clone(),
    }
}

/// `phase_owned_epoch` (private in the core): the phases that assert this
/// node's confirmed ownership of an epoch.
fn owned_epoch(phase: &Phase) -> Option<Epoch> {
    match phase {
        Phase::Restoring { spec, .. } => Some(spec.epoch),
        Phase::Starting { epoch, .. }
        | Phase::Publishing { epoch, .. }
        | Phase::EnsuringDurability { epoch, .. }
        | Phase::Cleaning { epoch, .. }
        | Phase::Dormant { epoch }
        | Phase::Resident { epoch } => Some(*epoch),
        _ => None,
    }
}

/// `TimerSlot::of` in `actor.rs`.
fn timer_slot(timer: &Timer) -> String {
    match timer {
        Timer::NodeLeaseRenew { .. } => "renew".into(),
        Timer::NodeLeaseFence { .. } => "fence".into(),
        Timer::CellAlarm { cell, .. } => format!("alarm:{cell}"),
        Timer::OperationDeadline { op } => format!("deadline:{op}"),
        Timer::QueuedActivation { cell, generation } => format!("queued:{cell}:{generation}"),
        Timer::OwnerLogRecoveryRetry { cell, generation } => {
            format!("recovery:{cell}:{generation}")
        }
        Timer::HoldRevokeGrace { cell, generation } => format!("holdgrace:{cell}:{generation}"),
    }
}

fn event_cell(event: &Event) -> Option<&str> {
    match event {
        Event::RequestAt { cell, .. }
        | Event::HandoffRequestAt { cell, .. }
        | Event::Evict { cell } => Some(cell),
        _ => None,
    }
}

fn effect_cell(effect: &Effect) -> Option<&str> {
    match effect {
        Effect::ReadOwner { cell, .. }
        | Effect::ReadNodeLease { cell, .. }
        | Effect::RecoverNodeLog { cell, .. }
        | Effect::CasOwner { cell, .. }
        | Effect::ReleaseOwner { cell, .. }
        | Effect::AdoptReleased { cell, .. }
        | Effect::Restore { cell, .. }
        | Effect::StartRuntime { cell, .. }
        | Effect::Publish { cell, .. }
        | Effect::EnsureDurable { cell, .. }
        | Effect::AwaitDurable { cell, .. }
        | Effect::VerifyOwnership { cell, .. }
        | Effect::StopRuntime { cell, .. } => Some(cell),
        _ => None,
    }
}
