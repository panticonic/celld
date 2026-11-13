// Copyright 2026 Deno Land Inc. Apache-2.0 license.

//! The production selector for the Actor execution domain.

use super::*;
use super::effects::{delay_for, expire, Expiry};
use futures_util::StreamExt as _;

impl Actor {
    /// Runs the production start-select-step loop.
    ///
    /// This selector stays unbiased because production permits every
    /// ready-input order. Direct callers can use [`Actor::start`] and
    /// [`Actor::step`] without this raw Tokio select.
    pub async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Message>) {
        let mut effects = FuturesUnordered::new();
        let mut delays = DelayQueue::new();
        let mut timer_slots = TimerSlots::<(delay_queue::Key, u64)>::default();
        let mut resume = crate::asyncrt::ResumeWatch::new();
        let mut out = StepOutput::default();
        self.start(&mut out);
        drain_step_output(&mut out, &mut effects, &mut delays, &mut timer_slots);
        loop {
            crate::asyncrt::select! {
                message = rx.recv() => {
                    let Some(message) = message else {
                        break;
                    };
                    self.step(ActorInput::Message(message), &mut out);
                }
                Some(completed) = effects.next(), if !effects.is_empty() => {
                    self.step(ActorInput::Completed(completed), &mut out);
                }
                // The delay queue runs on Tokio's clock, which stops while the
                // machine is suspended, but `at_mono_ms` is on the clock that
                // keeps running. After a resume, re-aim every pending timer
                // at what the node clock says is left, so a lease that lapsed
                // during the suspension fires now, not a lease-length later.
                () = resume.resumed() => {
                    reaim_delays(&mut delays, &timer_slots);
                }
                Some(expired) = delays.next(), if !delays.is_empty() => {
                    // A bounded chunk of a far deadline ends before the deadline;
                    // `expire` arms the next one, and the timer reaches the
                    // core at its deadline, as a near one does.
                    let now = crate::asyncrt::mono_ms();
                    if let Expiry::Fire(timer) =
                        expire(&mut delays, &mut timer_slots, expired.into_inner(), now)
                    {
                        self.step(ActorInput::TimerFired(timer), &mut out);
                    }
                }
            }
            drain_step_output(&mut out, &mut effects, &mut delays, &mut timer_slots);
        }
    }
}

fn reaim_delays(
    delays: &mut DelayQueue<TimerArm>,
    timers: &TimerSlots<(delay_queue::Key, u64)>,
) {
    let now = crate::asyncrt::mono_ms();
    for (key, at_mono_ms) in timers.values() {
        let left = at_mono_ms.saturating_sub(now);
        delays.reset(key, delay_for(left));
    }
}
