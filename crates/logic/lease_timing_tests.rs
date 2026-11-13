// The node lease's renewal cadence is the spec's `renew_ms`, not a fixed third
// of the TTL: an operator who raises the TTL for a loaded host, or lowers the
// cadence, gets exactly that.

use crate::pressure::PressureConfig;
use crate::*;

fn config() -> Config {
    Config {
        max_resident: 8,
        max_activations: 8,
        max_evictions: 4,
        max_releases: 4,
        max_outbound_websockets: 2,
        ownership_on_evict: OwnershipOnEvict::Sticky,
        require_node_lease: true,
        peer_protocol: 1,
        operation_deadline_ms: None,
        owner_log_recovery_backoff_ms: 1_000,
        owner_log_recovery_attempts: 3,
        alarm_resident_ms: 0,
        idle_evict_ms: None,
        hold_grace_ms: 5_000,
        max_holds_per_cell: 3,
        pressure: PressureConfig::default(),
    }
}

fn spec(ttl_ms: u64, renew_ms: u64) -> NodeLeaseSpec {
    NodeLeaseSpec {
        addr: "127.0.0.1:1".to_string(),
        peer_protocol: 1,
        generation: "g1".to_string(),
        resume_generation: None,
        ttl_ms,
        renew_ms,
    }
}

/// Acquire the first lease and return the timers the core armed for it, as
/// `(renew_at, fence_at)`.
fn first_lease_timers(spec: NodeLeaseSpec) -> (u64, u64) {
    let mut state = State::new("node-a", config());
    let effects = on_event(
        &mut state,
        Event::StartNodeLease {
            now_ms: 1_000_000,
            now_mono_ms: 100,
            spec,
        },
    );
    let read = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::ReadSelfNodeLease { op } => Some(*op),
            _ => None,
        })
        .expect("the core reads its own lease first");
    let effects = on_event(
        &mut state,
        Event::SelfNodeLeaseRead {
            op: read,
            now_ms: 1_000_000,
            now_mono_ms: 100,
            result: Ok(None),
        },
    );
    let cas = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::CasNodeLease { op, .. } => Some(*op),
            _ => None,
        })
        .expect("the core writes its lease");
    let effects = on_event(
        &mut state,
        Event::NodeLeaseCasCompleted {
            op: cas,
            now_mono_ms: 120,
            result: Ok(LeaseCasOutcome::Applied {
                etag: "e1".to_string(),
            }),
            stamped_log_state: None,
        },
    );
    let at = |wanted: fn(&Timer) -> bool| {
        effects
            .iter()
            .find_map(|effect| match effect {
                Effect::ScheduleTimer { timer, at_mono_ms } if wanted(timer) => Some(*at_mono_ms),
                _ => None,
            })
            .expect("the lease arms its timers")
    };
    (
        at(|timer| matches!(timer, Timer::NodeLeaseRenew { .. })),
        at(|timer| matches!(timer, Timer::NodeLeaseFence { .. })),
    )
}

#[test]
fn the_default_cadence_is_a_third_of_the_lease() {
    assert_eq!(NodeLeaseSpec::default_renew_ms(10_000), 3_333);
    let (renew, fence) = first_lease_timers(spec(9_000, NodeLeaseSpec::default_renew_ms(9_000)));
    // Anchored at the start of the acquisition (100 ms), not at its end.
    assert_eq!(renew, 100 + 3_000);
    assert_eq!(fence, 100 + 9_000);
}

#[test]
fn a_set_cadence_is_the_renewal_interval_and_the_ttl_stays_the_fence() {
    let (renew, fence) = first_lease_timers(spec(60_000, 5_000));
    assert_eq!(renew, 100 + 5_000);
    assert_eq!(fence, 100 + 60_000);
}

#[test]
fn a_zero_cadence_is_never_zero() {
    assert_eq!(spec(9_000, 0).renew_interval_ms(), 1);
}
