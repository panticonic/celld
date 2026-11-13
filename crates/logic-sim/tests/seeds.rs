//! The fixed seed set (`cargo test -p logic-sim`) and the mutation
//! self-checks. A longer sweep is `cargo run --release -p logic-sim --
//! --sweep 0..10000`.

use logic_sim::check::ViolationKind;
use logic_sim::config::{Faults, Mutation, SimConfig};
use logic_sim::{describe, run, run_seed};

/// Seeds every change must keep passing. Each is a full 30 s simulated run
/// with faults chosen by the seed, plus the crash-all recovery sweep.
const FIXED: std::ops::Range<u64> = 0..64;

#[test]
fn fixed_seeds_hold_every_safety_invariant() {
    let mut failures = Vec::new();
    let mut acks = 0;
    for seed in FIXED {
        let outcome = run_seed(seed, None, false);
        acks += outcome.acks;
        if let Some(v) = outcome.safety_violation() {
            failures.push(format!(
                "seed {seed}: {:?}: {} ({})",
                v.kind,
                v.detail,
                describe(&outcome.config)
            ));
        }
    }
    assert!(acks > 1_000, "the seed set acknowledged only {acks} writes");
    assert!(
        failures.is_empty(),
        "replay with `cargo run -p logic-sim -- --seed N`:\n{}",
        failures.join("\n")
    );
}

#[test]
fn a_seed_replays_exactly() {
    for seed in [3, 17] {
        let a = run_seed(seed, None, false);
        let b = run_seed(seed, None, false);
        assert_eq!(format!("{:?}", a.stats), format!("{:?}", b.stats));
        assert_eq!(a.acks, b.acks);
    }
}

#[test]
fn a_quiet_world_acknowledges_and_recovers_every_cell() {
    let mut config = SimConfig::for_seed(1);
    config.faults = Faults::none();
    let outcome = run(config);
    assert!(outcome.safety_violation().is_none());
    assert!(outcome.liveness_violation().is_none());
    assert!(outcome.acks > 50);
    assert_eq!(outcome.stats.final_reads_ok as usize, outcome.config.cells);
}

/// Each deliberately broken world must be caught, with a violation of a
/// kind that names the broken mechanism. This is the simulator's version of
/// upstream's pinned failing verdicts: a checker that cannot fail these is
/// not checking anything.
fn caught(mutation: Mutation, expected: &[ViolationKind]) {
    for seed in 0..200 {
        let outcome = run_seed(seed, Some(mutation), false);
        if let Some(v) = outcome.safety_violation() {
            assert!(
                expected.contains(&v.kind),
                "{} caught as {:?} on seed {seed}, expected one of {expected:?}: {}",
                mutation.name(),
                v.kind,
                v.detail
            );
            return;
        }
    }
    panic!("mutation {} was not caught in 200 seeds", mutation.name());
}

#[test]
fn mutation_non_conditional_owner_cas_is_caught() {
    caught(
        Mutation::NonConditionalOwnerCas,
        &[
            ViolationKind::OneWriterPerEpoch,
            ViolationKind::EpochRegression,
        ],
    );
}

#[test]
fn mutation_ack_before_durable_is_caught() {
    caught(Mutation::AckBeforeDurable, &[ViolationKind::LostAck]);
}

#[test]
fn mutation_skip_verify_ownership_is_caught() {
    caught(
        Mutation::SkipVerifyOwnership,
        &[ViolationKind::StaleAck, ViolationKind::LostAck],
    );
}

#[test]
fn mutation_skip_log_recovery_is_caught() {
    caught(
        Mutation::SkipLogRecovery,
        &[ViolationKind::StaleAck, ViolationKind::LostAck],
    );
}

#[test]
fn mutation_follower_ignores_seal_is_caught() {
    caught(
        Mutation::FollowerIgnoresSeal,
        &[ViolationKind::StaleAck, ViolationKind::LostAck],
    );
}

#[test]
fn mutation_restore_oldest_epoch_is_caught() {
    caught(Mutation::RestoreOldestEpoch, &[ViolationKind::LostAck]);
}

#[test]
fn mutation_lease_always_expired_is_caught() {
    caught(
        Mutation::LeaseAlwaysExpired,
        &[ViolationKind::StaleServe, ViolationKind::StaleAck],
    );
}

/// Seeds that once reproduced a defect in the core (see
/// `docs/fork/upstream-findings.md`), replayed with every finding fatal.
const FIXED_FINDINGS: &[(&str, &[u64])] = &[("S1", &[52]), ("S2", &[97, 4]), ("S4", &[1442, 2395, 3393, 1358])];

#[test]
fn fixed_findings_stay_fixed() {
    let mut failures = Vec::new();
    for (finding, seeds) in FIXED_FINDINGS {
        for &seed in *seeds {
            let mut config = SimConfig::for_seed(seed);
            config.strict = true;
            let outcome = run(config);
            if let Some(v) = outcome.safety_violation() {
                failures.push(format!("{finding} seed {seed}: {:?}: {}", v.kind, v.detail));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
