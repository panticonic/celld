//! What one simulated run looks like. Everything here is derived from the
//! seed by [`SimConfig::for_seed`], so `--seed N` alone replays a run.

use crate::rng::Rng;

/// Which durability posture the modelled shell runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Durability {
    /// `CELLD_DURABILITY=bucket`: every proof is a bucket upload followed by
    /// an ownership read.
    Bucket,
    /// The default fleet posture: a write is proven when every follower of
    /// the owner's node-log session holds it, racing the bucket upload.
    Fleet,
}

/// Deliberately broken variants of the modelled world. Each one must be
/// caught by the checker; that is how the checker proves it has teeth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutation {
    /// The store applies an ownership compare-and-swap whatever its guard.
    NonConditionalOwnerCas,
    /// The replicator reports a durability proof without replicating.
    AckBeforeDurable,
    /// A bucket proof's ownership read always answers "still yours".
    SkipVerifyOwnership,
    /// Dead-owner node-log recovery reports success without sealing or
    /// gathering anything.
    SkipLogRecovery,
    /// A follower keeps accepting appends for a session it sealed.
    FollowerIgnoresSeal,
    /// Restore picks the oldest non-empty epoch prefix, not the newest.
    RestoreOldestEpoch,
    /// Peers read every node lease as already expired.
    LeaseAlwaysExpired,
    /// The node's monotonic clock stops during a suspension (`pause_stops_mono`
    /// pauses): the shell's clock before it was changed to include suspended
    /// time. The shipped shell's clock keeps running, so without this
    /// mutation a suspension is only a long pause.
    SuspendStopsMonotonic,
}

impl Mutation {
    pub const ALL: [Mutation; 8] = [
        Mutation::NonConditionalOwnerCas,
        Mutation::AckBeforeDurable,
        Mutation::SkipVerifyOwnership,
        Mutation::SkipLogRecovery,
        Mutation::FollowerIgnoresSeal,
        Mutation::RestoreOldestEpoch,
        Mutation::LeaseAlwaysExpired,
        Mutation::SuspendStopsMonotonic,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Mutation::NonConditionalOwnerCas => "non-conditional-owner-cas",
            Mutation::AckBeforeDurable => "ack-before-durable",
            Mutation::SkipVerifyOwnership => "skip-verify-ownership",
            Mutation::SkipLogRecovery => "skip-log-recovery",
            Mutation::FollowerIgnoresSeal => "follower-ignores-seal",
            Mutation::RestoreOldestEpoch => "restore-oldest-epoch",
            Mutation::LeaseAlwaysExpired => "lease-always-expired",
            Mutation::SuspendStopsMonotonic => "suspend-stops-monotonic",
        }
    }

    pub fn parse(name: &str) -> Option<Mutation> {
        Mutation::ALL.into_iter().find(|m| m.name() == name)
    }

    /// The posture a mutation needs in order to matter at all.
    pub fn durability(self) -> Option<Durability> {
        match self {
            Mutation::SkipVerifyOwnership => Some(Durability::Bucket),
            Mutation::SkipLogRecovery | Mutation::FollowerIgnoresSeal => Some(Durability::Fleet),
            _ => None,
        }
    }
}

/// Fault knobs. Each one can be zeroed independently, which is what the
/// trace minimiser does.
#[derive(Clone, Debug, PartialEq)]
pub struct Faults {
    /// Probability a peer message is dropped.
    pub net_drop: f64,
    /// Upper bound of ordinary peer latency (ms). Random latency reorders.
    pub net_delay_ms: u64,
    /// Mean time between partition changes (ms); 0 disables partitions.
    pub partition_every_ms: u64,
    /// Mean time between process crashes (ms); 0 disables crashes.
    pub crash_every_ms: u64,
    /// Mean time between graceful drains (ms); 0 disables them.
    pub drain_every_ms: u64,
    /// Max absolute wall-clock offset per node (ms).
    pub skew_ms: u64,
    /// Mean time between process pauses (ms); 0 disables them.
    pub pause_every_ms: u64,
    /// Whether a pause is a VM suspension rather than only a stopped process
    /// (a SIGSTOP or a long GC). The shell's monotonic clock counts suspended
    /// time (`asyncrt::mono_ms`), so both look alike to the core; only
    /// `Mutation::SuspendStopsMonotonic` makes the clock stop.
    pub pause_stops_mono: bool,
    /// Probability a store request fails or loses its response.
    pub store_fail: f64,
    /// Probability a store request linearizes very late (a tail request).
    pub store_slow: f64,
    /// Mean time between node-to-bucket partitions (ms); 0 disables them.
    pub bucket_cut_every_ms: u64,
}

impl Faults {
    pub fn none() -> Self {
        Faults {
            net_drop: 0.0,
            net_delay_ms: 5,
            partition_every_ms: 0,
            crash_every_ms: 0,
            drain_every_ms: 0,
            skew_ms: 0,
            pause_every_ms: 0,
            pause_stops_mono: false,
            store_fail: 0.0,
            store_slow: 0.0,
            bucket_cut_every_ms: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SimConfig {
    pub seed: u64,
    pub nodes: usize,
    pub cells: usize,
    pub durability: Durability,
    /// Simulated time with faults and load (ms), before the final recovery.
    pub duration_ms: u64,
    /// Mean gap between client requests (ms).
    pub request_every_ms: u64,
    pub ttl_ms: u64,
    pub max_resident: usize,
    pub idle_evict_ms: Option<u64>,
    pub sticky: bool,
    pub faults: Faults,
    pub mutation: Option<Mutation>,
    /// Record a human-readable trace (slower; on for replays).
    pub trace: bool,
}

impl SimConfig {
    /// Everything about a run is a function of the seed (plus an optional
    /// mutation), so a seed is a complete reproduction.
    pub fn for_seed(seed: u64) -> Self {
        let mut rng = Rng::new(seed.wrapping_mul(0x2545_F491_4F6C_DD1D));
        let durability = if rng.chance(0.5) {
            Durability::Fleet
        } else {
            Durability::Bucket
        };
        let nodes = rng.range(2, 4) as usize;
        let cells = rng.range(1, 4) as usize;
        let faults = Faults {
            net_drop: [0.0, 0.01, 0.05][rng.index(3)],
            net_delay_ms: [5, 20, 80][rng.index(3)],
            partition_every_ms: [0, 4_000, 10_000][rng.index(3)],
            crash_every_ms: [0, 5_000, 12_000][rng.index(3)],
            drain_every_ms: [0, 0, 15_000][rng.index(3)],
            skew_ms: [0, 0, 50, 300][rng.index(4)],
            pause_every_ms: [0, 0, 8_000][rng.index(3)],
            pause_stops_mono: rng.chance(0.3),
            store_fail: [0.0, 0.02, 0.08][rng.index(3)],
            store_slow: [0.0, 0.0, 0.02][rng.index(3)],
            bucket_cut_every_ms: [0, 0, 9_000][rng.index(3)],
        };
        SimConfig {
            seed,
            nodes,
            cells,
            durability,
            duration_ms: 30_000,
            request_every_ms: [15, 40, 120][rng.index(3)],
            ttl_ms: 3_000,
            max_resident: rng.range(1, 4) as usize,
            idle_evict_ms: [None, Some(1_500), Some(5_000)][rng.index(3)],
            sticky: rng.chance(0.3),
            faults,
            mutation: None,
            trace: false,
        }
    }

    pub fn with_mutation(mut self, mutation: Option<Mutation>) -> Self {
        self.mutation = mutation;
        if let Some(durability) = mutation.and_then(Mutation::durability) {
            self.durability = durability;
        }
        self
    }

    /// The tolerance the exclusive-service check grants: two wall clocks
    /// may disagree by up to twice the configured offset.
    pub fn skew_tolerance_ms(&self) -> u64 {
        2 * self.faults.skew_ms
    }
}
