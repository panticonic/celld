//! A deterministic simulator for celld's coordination core.
//!
//! N nodes each run an unmodified `celld_logic::State` (compiled with
//! `cfg(celld_internal_tests)` for its observer hooks). A modelled shell per
//! node executes the core's effects against a modelled bucket, peer network,
//! clock and disk, and feeds the results back as events, under one seeded
//! scheduler. A checker judges every step. See README.md for the model, the
//! invariants and the fidelity gaps.

pub mod check;
pub mod config;
pub mod rng;
pub mod trace;
mod world;

use check::{Violation, ViolationKind};
use config::{Faults, Mutation, SimConfig};

pub use world::Stats;

/// The result of one run.
pub struct Outcome {
    pub config: SimConfig,
    pub violations: Vec<Violation>,
    pub stats: Stats,
    pub acks: u64,
    pub restores_checked: u64,
    pub trace: Option<String>,
}

impl Outcome {
    pub fn safety_violation(&self) -> Option<&Violation> {
        self.violations.iter().find(|v| v.kind.is_safety())
    }

    pub fn liveness_violation(&self) -> Option<&Violation> {
        self.violations
            .iter()
            .find(|v| v.kind == ViolationKind::Liveness)
    }
}

/// Run one configuration to completion. A panic inside the decision core is
/// itself a finding, so it is caught and reported as one.
pub fn run(config: SimConfig) -> Outcome {
    let recorded = config.clone();
    let traced = config.trace;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let mut world = world::World::new(config);
        world.run();
        let trace = traced.then(|| {
            let first = world
                .checker
                .violations
                .iter()
                .find(|v| v.kind.is_safety())
                .or(world.checker.violations.first());
            let full = std::env::var_os("LOGIC_SIM_FULL_TRACE").is_some();
            let cell = first.and_then(|v| v.cell.clone()).filter(|_| !full);
            let until = first.map_or(u64::MAX, |v| v.time);
            let lines = world.trace.focused(cell.as_deref(), until);
            trace::render(&lines, if full { usize::MAX } else { 400 })
        });
        (
            world.checker.violations.clone(),
            world.stats.clone(),
            world.checker.acks,
            world.checker.restores_checked,
            trace,
        )
    }));
    match result {
        Ok((violations, stats, acks, restores_checked, trace)) => Outcome {
            config: recorded,
            violations,
            stats,
            acks,
            restores_checked,
            trace,
        },
        Err(panic) => {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "non-string panic".to_string());
            Outcome {
                config: recorded,
                violations: vec![Violation {
                    kind: ViolationKind::CorePanic,
                    time: 0,
                    cell: None,
                    detail: message,
                }],
                stats: Stats::default(),
                acks: 0,
                restores_checked: 0,
                trace: None,
            }
        }
    }
}

/// Run the configuration a seed names.
pub fn run_seed(seed: u64, mutation: Option<Mutation>, trace: bool) -> Outcome {
    let mut config = SimConfig::for_seed(seed).with_mutation(mutation);
    config.trace = trace;
    run(config)
}

/// Shrink a failing configuration while it keeps failing with the same kind
/// of violation: turn off each fault class, then fewer nodes and cells, then
/// a shorter run. The seed stays the same, so the result is a replayable
/// configuration that needs fewer moving parts to fail.
pub fn minimise(config: &SimConfig) -> SimConfig {
    let mut quiet = config.clone();
    quiet.trace = false;
    let Some(kind) = run(quiet.clone()).safety_violation().map(|v| v.kind) else {
        return config.clone();
    };
    let fails = |c: &SimConfig| {
        let mut c = c.clone();
        c.trace = false;
        run(c).safety_violation().is_some_and(|v| v.kind == kind)
    };
    let mut best = quiet;
    let none = Faults::none();
    type Knob = fn(&mut Faults, &Faults);
    let knobs: [Knob; 11] = [
        |f, n| f.net_drop = n.net_drop,
        |f, n| f.net_delay_ms = n.net_delay_ms,
        |f, n| f.partition_every_ms = n.partition_every_ms,
        |f, n| f.crash_every_ms = n.crash_every_ms,
        |f, n| f.drain_every_ms = n.drain_every_ms,
        |f, n| f.skew_ms = n.skew_ms,
        |f, n| f.pause_every_ms = n.pause_every_ms,
        |f, n| f.pause_stops_mono = n.pause_stops_mono,
        |f, n| f.store_fail = n.store_fail,
        |f, n| f.store_slow = n.store_slow,
        |f, n| f.bucket_cut_every_ms = n.bucket_cut_every_ms,
    ];
    for knob in knobs {
        let mut candidate = best.clone();
        knob(&mut candidate.faults, &none);
        if candidate.faults != best.faults && fails(&candidate) {
            best = candidate;
        }
    }
    while best.cells > 1 {
        let mut candidate = best.clone();
        candidate.cells -= 1;
        if !fails(&candidate) {
            break;
        }
        best = candidate;
    }
    while best.nodes > 2 {
        let mut candidate = best.clone();
        candidate.nodes -= 1;
        if !fails(&candidate) {
            break;
        }
        best = candidate;
    }
    if best.idle_evict_ms.is_some() {
        let mut candidate = best.clone();
        candidate.idle_evict_ms = None;
        if fails(&candidate) {
            best = candidate;
        }
    }
    best.trace = config.trace;
    best
}

/// A one-line description of a run's configuration.
pub fn describe(c: &SimConfig) -> String {
    let f = &c.faults;
    format!(
        "seed={} durability={:?} nodes={} cells={} max_resident={} idle_evict={:?} sticky={} \
         req_every={}ms mutation={} | drop={} delay<={}ms partition~{}ms crash~{}ms drain~{}ms \
         skew<={}ms pause~{}ms{} store_fail={} store_slow={} bucket_cut~{}ms",
        c.seed,
        c.durability,
        c.nodes,
        c.cells,
        c.max_resident,
        c.idle_evict_ms,
        c.sticky,
        c.request_every_ms,
        c.mutation.map_or("none", Mutation::name),
        f.net_drop,
        f.net_delay_ms,
        f.partition_every_ms,
        f.crash_every_ms,
        f.drain_every_ms,
        f.skew_ms,
        f.pause_every_ms,
        if f.pause_stops_mono {
            "(mono stops)"
        } else {
            ""
        },
        f.store_fail,
        f.store_slow,
        f.bucket_cut_every_ms,
    )
}
