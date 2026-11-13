// Copyright 2026 Deno Land Inc. Apache-2.0 license.

//! Effect dispatch keeps ordinary work off the core's lease timer thread.

use super::{CompletedEffect, EffectFuture, Ownership, TimerArm, TimerSlots};
use crate::ownership_store::now_ms;
use celld_logic::{CasGuard, Event, Failure, LeaseCasOutcome, NodeLeaseRecord, OpId, Timer};
use futures_util::stream::FuturesUnordered;
use std::future::Future;
use tokio_util::time::{delay_queue, DelayQueue};

enum EffectJob {
    Spawn(EffectFuture),
    // Lease authority must progress even when the host workers are occupied.
    Lease(EffectFuture),
}

#[cfg(celld_internal_tests)]
impl EffectJob {
    fn into_future(self) -> EffectFuture {
        match self {
            Self::Spawn(future) | Self::Lease(future) => future,
        }
    }
}

pub(crate) enum LeaseRequest {
    ReadSelf {
        op: OpId,
        node: String,
    },
    Cas {
        op: OpId,
        guard: CasGuard,
        record: NodeLeaseRecord,
        authority_expires_ms: Option<u64>,
    },
}

/// Futures and timer arms emitted by one Actor transition.
#[derive(Default)]
pub struct StepOutput {
    effects: Vec<EffectJob>,
    pub timers: Vec<TimerArm>,
}

impl StepOutput {
    /// Queue ordinary effect work. In production the drain spawns it on the host
    /// runtime immediately after the Actor step, so it runs to completion and
    /// the core polls only its join handle.
    ///
    /// The difference from `spawn_detached` is only visible when a deterministic
    /// driver takes the queue: dropping an unpolled future discards this work
    /// before it starts.
    pub(crate) fn spawn_scoped(
        &mut self,
        future: impl Future<Output = CompletedEffect> + Send + 'static,
    ) {
        self.effects.push(EffectJob::Spawn(Box::pin(future)));
    }

    /// Production behavior is identical to `spawn_scoped`: the work runs to
    /// completion on the host runtime and the core polls only a join handle.
    ///
    /// This method exists because deterministic worlds drop queued futures and
    /// name these tasks by their `actor.rs` spawn site. Starting the work at the
    /// call keeps seeded stop, restore, and durability regressions on their
    /// existing schedules.
    ///
    /// `#[track_caller]` is load-bearing: the simulated spawn records the caller
    /// location as the task's spawn site.
    #[track_caller]
    pub(crate) fn spawn_detached(
        &mut self,
        future: impl Future<Output = CompletedEffect> + Send + 'static,
    ) {
        let task = crate::asyncrt::spawn(future);
        self.spawn_scoped(async move { task.await.expect("actor effect task panicked") });
    }

    pub(crate) fn lease(&mut self, request: LeaseRequest, ownership: &Ownership) {
        let ownership = ownership.clone();
        let future: EffectFuture = match request {
            LeaseRequest::ReadSelf { op, node } => Box::pin(async move {
                let result = ownership.read_self_node_lease(&node).await;
                CompletedEffect::plain(Event::SelfNodeLeaseRead {
                    op,
                    now_ms: now_ms(),
                    now_mono_ms: crate::asyncrt::mono_ms(),
                    result,
                })
            }),
            LeaseRequest::Cas {
                op,
                guard,
                record,
                authority_expires_ms,
            } => Box::pin(async move {
                let attempt_started_mono_ms = crate::asyncrt::mono_ms();
                let node = record.node.clone();
                let candidate_expires_ms = record.expires_ms;
                // Logged before the CAS: with only the completion line, a
                // renewal hung on a storage tail is indistinguishable from
                // a timer that never fired (the n6 fence, 2026-08-11).
                tracing::info!(
                    event = "node_lease_attempt_started",
                    %node,
                    attempt = if authority_expires_ms.is_some() {
                        "renew"
                    } else {
                        "acquire"
                    },
                    prior_authority_headroom_ms = authority_expires_ms
                        .map(|expires_ms| expires_ms.saturating_sub(now_ms()))
                        .unwrap_or(0),
                    "node lease attempt started"
                );
                // A renewal must return while proven authority remains,
                // because only a returned attempt lets the ambiguity
                // read-back run before the watchdog. The 10:15Z R2
                // brownout fenced 9 nodes whose sole hung attempt was
                // still inside the transport's 15 s timeout when the
                // 10 s TTL expired. Bound the attempt to half the
                // remaining authority (capped, floored) and map timeout
                // to Ambiguous — the same conservative outcome a lost
                // response already produces, so safety is unchanged.
                // The stamp survives a timed-out attempt: the backend
                // writes it through the out-parameter synchronously at
                // serialization, before the transport await, so even a
                // dropped future has reported what the possibly-landed
                // body carried.
                let mut stamped_log_state = None;
                let result = match authority_expires_ms {
                    Some(expires_ms) => {
                        let remaining = expires_ms.saturating_sub(now_ms());
                        let bound =
                            std::time::Duration::from_millis((remaining / 2).clamp(250, 2_500));
                        match crate::asyncrt::timeout(
                            bound,
                            ownership.cas_node_lease(guard, record, &mut stamped_log_state),
                        )
                        .await
                        {
                            Ok(result) => result,
                            Err(_) => Err(Failure::Ambiguous),
                        }
                    }
                    None => {
                        ownership
                            .cas_node_lease(guard, record, &mut stamped_log_state)
                            .await
                    }
                };
                let completed_ms = now_ms();
                let elapsed_ms = crate::asyncrt::mono_ms().saturating_sub(attempt_started_mono_ms);
                let prior_authority_headroom_ms = authority_expires_ms
                    .map(|expires_ms| expires_ms.saturating_sub(completed_ms))
                    .unwrap_or(0);
                let candidate_headroom_ms = candidate_expires_ms.saturating_sub(completed_ms);
                let attempt = if authority_expires_ms.is_some() {
                    "renew"
                } else {
                    "acquire"
                };
                let outcome = match &result {
                    Ok(LeaseCasOutcome::Applied { .. }) => "applied",
                    Ok(LeaseCasOutcome::Rejected) => "rejected",
                    Err(Failure::Ambiguous) => "ambiguous",
                    Err(Failure::Definite) => "definite_failure",
                };
                if matches!(&result, Ok(LeaseCasOutcome::Applied { .. })) {
                    tracing::info!(
                        event = "node_lease_attempt",
                        %node,
                        attempt,
                        outcome,
                        elapsed_ms,
                        prior_authority_headroom_ms,
                        candidate_headroom_ms,
                        "node lease attempt completed"
                    );
                } else {
                    tracing::warn!(
                        event = "node_lease_attempt",
                        %node,
                        attempt,
                        outcome,
                        elapsed_ms,
                        prior_authority_headroom_ms,
                        candidate_headroom_ms,
                        "node lease attempt did not apply"
                    );
                }
                CompletedEffect::plain(Event::NodeLeaseCasCompleted {
                    op,
                    now_mono_ms: crate::asyncrt::mono_ms(),
                    result,
                    stamped_log_state,
                })
            }),
        };
        self.effects.push(EffectJob::Lease(future));
    }

    // Binary test harnesses compile the library without cfg(test).
    // celld_internal_tests is never set in a shipped build.
    #[cfg(celld_internal_tests)]
    pub fn take_effects(&mut self) -> Vec<EffectFuture> {
        self.effects.drain(..).map(EffectJob::into_future).collect()
    }

    #[cfg(celld_internal_tests)]
    pub fn effects_is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    // A driver that runs only the newest effect must retain every older job.
    #[cfg(celld_internal_tests)]
    pub fn pop_effect(&mut self) -> Option<EffectFuture> {
        self.effects.pop().map(EffectJob::into_future)
    }

    #[cfg(celld_internal_tests)]
    pub fn effects_len(&self) -> usize {
        self.effects.len()
    }
}

/// The longest a single delay-queue entry is armed for.
///
/// `DelayQueue` panics ("invalid deadline") on a delay past its timer wheel's
/// range, about two years, and an alarm can be set for any date: a `setAlarm`
/// for the year 2100 took the node down. A deadline further out is armed in
/// chunks of this length. When a chunk expires with the deadline still ahead,
/// the loop arms the next one (see `production.rs`); the timer reaches the
/// core only at its deadline, exactly as a nearer one does.
pub(crate) const MAX_DELAY_CHUNK_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// A chunk that expires this far (or less) ahead of its deadline is the
/// deadline: Tokio's timer rounds, and the clock the deadline is on is read a
/// moment after it. Further ahead means a chunk ended, or the clock jumped,
/// and either way the remainder is armed again.
pub(crate) const EARLY_FIRE_TOLERANCE_MS: u64 = 1_000;

/// The delay to arm for a deadline `left_ms` away: all of it, up to a chunk.
pub(crate) fn delay_for(left_ms: u64) -> std::time::Duration {
    std::time::Duration::from_millis(left_ms.min(MAX_DELAY_CHUNK_MS))
}

/// What an expired delay-queue entry means.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Expiry {
    /// The deadline is here: the timer fires.
    Fire(Timer),
    /// A chunk of a far deadline ended; the next chunk is armed.
    Rearmed,
    /// The arm was replaced or cancelled while it waited.
    Stale,
}

/// Settle an entry the delay queue has expired, `now_mono_ms` being the
/// node's monotonic clock.
pub(crate) fn expire(
    delays: &mut DelayQueue<TimerArm>,
    timers: &mut TimerSlots<(delay_queue::Key, u64)>,
    arm: TimerArm,
    now_mono_ms: u64,
) -> Expiry {
    let left = arm.at_mono_ms.saturating_sub(now_mono_ms);
    if left > EARLY_FIRE_TOLERANCE_MS {
        if !timers.is_current(&arm.slot, arm.ordinal) {
            return Expiry::Stale;
        }
        let key = delays.insert(arm.clone(), delay_for(left));
        timers.install(&arm, (key, arm.at_mono_ms));
        return Expiry::Rearmed;
    }
    match timers.fire(&arm.slot, arm.ordinal) {
        Some(_) => Expiry::Fire(arm.timer),
        None => Expiry::Stale,
    }
}

pub(crate) fn drain_step_output(
    out: &mut StepOutput,
    effects: &mut FuturesUnordered<EffectFuture>,
    delays: &mut DelayQueue<TimerArm>,
    timers: &mut TimerSlots<(delay_queue::Key, u64)>,
) {
    for job in out.effects.drain(..) {
        match job {
            EffectJob::Spawn(future) => {
                // Spawn only at the production drain so deterministic drivers
                // can take the futures and retain their existing schedule.
                let task = crate::asyncrt::spawn(future);
                effects.push(Box::pin(async move {
                    task.await.expect("actor effect task panicked")
                }));
            }
            EffectJob::Lease(future) => effects.push(future),
        }
    }
    for arm in out.timers.drain(..) {
        let delay = delay_for(arm.at_mono_ms.saturating_sub(crate::asyncrt::mono_ms()));
        let key = delays.insert(arm.clone(), delay);
        // The deadline rides along so a resume can re-aim the delay.
        if let Some((displaced, _)) = timers.install(&arm, (key, arm.at_mono_ms)) {
            delays.remove(&displaced);
        }
    }
}

#[cfg(test)]
mod far_deadline_tests {
    use super::*;

    #[test]
    fn a_far_deadline_is_armed_in_a_bounded_chunk() {
        assert_eq!(delay_for(0), std::time::Duration::ZERO);
        assert_eq!(delay_for(1_500), std::time::Duration::from_millis(1_500));
        let chunk = std::time::Duration::from_millis(MAX_DELAY_CHUNK_MS);
        assert_eq!(delay_for(MAX_DELAY_CHUNK_MS), chunk);
        assert_eq!(delay_for(MAX_DELAY_CHUNK_MS + 1), chunk);
        // An alarm set for the year 2100, from 2026: 74 years.
        assert_eq!(delay_for(74 * 365 * 24 * 60 * 60 * 1000), chunk);
        assert_eq!(delay_for(u64::MAX), chunk);
    }

    type Slots = TimerSlots<(delay_queue::Key, u64)>;

    /// Arm `timer` for `at` the way production does: the Actor numbers the
    /// arm, the drain puts it in the delay queue and records its key.
    fn arm(
        delays: &mut DelayQueue<TimerArm>,
        slots: &mut Slots,
        timer: Timer,
        at: u64,
        now: u64,
    ) -> (TimerArm, delay_queue::Key) {
        let (arm, _) = TimerSlots::<()>::default().arm(timer, at, ());
        let key = delays.insert(arm.clone(), delay_for(at.saturating_sub(now)));
        slots.install(&arm, (key, at));
        (arm, key)
    }

    const DAY: u64 = 24 * 60 * 60 * 1000;

    fn alarm_timer() -> Timer {
        Timer::CellAlarm {
            cell: "Agent:1".to_string(),
            generation: 1,
        }
    }

    /// An alarm far ahead is armed in chunks and reaches the core only at its
    /// deadline: each chunk's expiry arms the next, and nothing fires early.
    #[tokio::test]
    async fn a_far_alarm_fires_at_its_deadline_not_at_a_chunk() {
        let mut delays = DelayQueue::new();
        let mut slots = Slots::default();
        let (now, deadline) = (1_000_000_u64, 1_000_000 + 20 * DAY);
        let (first, key) = arm(&mut delays, &mut slots, alarm_timer(), deadline, now);
        assert_eq!(delays.len(), 1);

        // The first chunk (7 days) ends 13 days short of the deadline.
        let expired = delays.remove(&key).into_inner();
        let outcome = expire(&mut delays, &mut slots, expired, now + 7 * DAY);
        assert_eq!(outcome, Expiry::Rearmed);
        assert_eq!(delays.len(), 1, "the next chunk is armed");
        assert!(slots.is_current(&first.slot, first.ordinal), "still the current arm");
        // The second ends 6 days short: what is left is under a chunk.
        let key = slots.values().next().unwrap().0;
        let expired = delays.remove(&key).into_inner();
        let outcome = expire(&mut delays, &mut slots, expired, now + 14 * DAY);
        assert_eq!(outcome, Expiry::Rearmed);
        // The last lands on the deadline and fires.
        let key = slots.values().next().unwrap().0;
        let expired = delays.remove(&key).into_inner();
        let outcome = expire(&mut delays, &mut slots, expired, deadline);
        assert_eq!(outcome, Expiry::Fire(alarm_timer()));
        assert_eq!(delays.len(), 0);
        assert!(!slots.is_current(&first.slot, first.ordinal), "a fired arm is spent");
    }

    /// An ordinary timer fires at its deadline, a moment early or late, and a
    /// spent arm never fires again.
    #[tokio::test]
    async fn an_ordinary_timer_fires_once() {
        let mut delays = DelayQueue::new();
        let mut slots = Slots::default();
        let timer = Timer::NodeLeaseRenew { generation: 3 };
        let (_, key) = arm(&mut delays, &mut slots, timer.clone(), 5_000, 0);
        let expired = delays.remove(&key).into_inner();
        // Tokio fired it 1 ms early: that is still the deadline.
        let outcome = expire(&mut delays, &mut slots, expired.clone(), 4_999);
        assert_eq!(outcome, Expiry::Fire(timer));
        let outcome = expire(&mut delays, &mut slots, expired, 5_000);
        assert_eq!(outcome, Expiry::Stale);
    }

    /// A deadline replaced while its chunk waited is not carried on.
    #[tokio::test]
    async fn a_replaced_far_arm_is_not_rearmed() {
        let mut delays = DelayQueue::new();
        let mut slots = Slots::default();
        let (_, key) = arm(&mut delays, &mut slots, alarm_timer(), 20 * DAY, 0);
        let stale = delays.remove(&key).into_inner();
        // A newer arm of the same slot takes it over.
        let mut newer = stale.clone();
        newer.ordinal += 1;
        let spare = delays.insert(newer.clone(), delay_for(DAY));
        slots.install(&newer, (spare, 30 * DAY));
        let outcome = expire(&mut delays, &mut slots, stale, 7 * DAY);
        assert_eq!(outcome, Expiry::Stale);
        assert_eq!(delays.len(), 1, "only the newer arm is waiting");
    }

    /// The delay queue is what refused a far deadline ("invalid deadline"), so
    /// it is what the chunk has to fit.
    #[tokio::test]
    async fn the_delay_queue_takes_the_largest_chunk_and_the_far_deadline() {
        let mut queue = DelayQueue::new();
        queue.insert((), delay_for(MAX_DELAY_CHUNK_MS));
        queue.insert((), delay_for(74 * 365 * 24 * 60 * 60 * 1000));
        queue.insert((), delay_for(u64::MAX));
        assert_eq!(queue.len(), 3);
    }
}
