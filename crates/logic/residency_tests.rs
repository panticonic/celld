// Tests for explicit residency holds (`ctx.residency.hold()`).
//
// A small scripted executor answers the core's effects the way a healthy shell
// would, and the tests hold back or withhold exactly the answers a scenario is
// about. Nothing here reads a clock: time is a number the test advances.

use crate::pressure::{Load, PressureConfig};
use crate::*;

const GRACE_MS: u64 = 5_000;
const IDLE_MS: u64 = 1_000;

fn config(max_resident: usize) -> Config {
    Config {
        max_resident,
        max_activations: 8,
        max_evictions: 4,
        max_releases: 4,
        max_outbound_websockets: 2,
        ownership_on_evict: OwnershipOnEvict::Sticky,
        require_node_lease: false,
        peer_protocol: 1,
        operation_deadline_ms: None,
        owner_log_recovery_backoff_ms: 1_000,
        owner_log_recovery_attempts: 3,
        alarm_resident_ms: 0,
        idle_evict_ms: Some(IDLE_MS),
        hold_grace_ms: GRACE_MS,
        max_holds_per_cell: 3,
        pressure: PressureConfig::default(),
    }
}

fn pressured() -> PressureConfig {
    PressureConfig {
        high_bytes: Some(1_000),
        rss_hard_bytes: None,
    }
}

fn load(resident_cells: usize, bytes: u64) -> Load {
    Load {
        resident_cells,
        rss_bytes: bytes,
        in_use_bytes: bytes,
        cgroup_working_set_bytes: None,
        cgroup_current_bytes: None,
        container_reserved_bytes: 0,
    }
}

/// One node plus a scripted shell.
struct Node {
    state: State,
    now: u64,
    next_request: RequestId,
    next_hold: HoldId,
    /// Everything the core ever asked for, in order.
    log: Vec<Effect>,
    /// Pending grace timers the test can fire.
    timers: Vec<(Timer, u64)>,
    /// When false, a durability proof for an eviction is left unanswered.
    answer_durability: bool,
    /// Requests the core completed, by id.
    completed: Vec<(RequestId, Result<Route, RequestError>)>,
}

impl Node {
    fn new(config: Config) -> Self {
        Self {
            state: State::new("node-a", config),
            now: 0,
            next_request: 1,
            next_hold: 1,
            log: Vec::new(),
            timers: Vec::new(),
            answer_durability: true,
            completed: Vec::new(),
        }
    }

    /// Feed one event, then answer every effect it produced (and every effect
    /// those answers produced) the way a healthy shell would.
    fn send(&mut self, event: Event) -> Vec<Effect> {
        let mut all = Vec::new();
        let mut queue = vec![event];
        while let Some(event) = queue.pop() {
            let effects = on_event(&mut self.state, event);
            for effect in &effects {
                if let Some(next) = self.answer(effect) {
                    queue.push(next);
                }
            }
            self.log.extend(effects.iter().cloned());
            all.extend(effects);
        }
        all
    }

    fn answer(&mut self, effect: &Effect) -> Option<Event> {
        match effect {
            Effect::ReadOwner { op, .. } => Some(Event::OwnerRead {
                op: *op,
                now_ms: self.now,
                result: Ok(None),
            }),
            Effect::CasOwner { op, .. } => Some(Event::OwnerCasCompleted {
                op: *op,
                result: Ok(CasOutcome::Applied),
            }),
            Effect::Restore { op, .. } => Some(Event::RestoreCompleted {
                op: *op,
                result: Ok(RestoreOutcome {
                    restored: false,
                    alarm: None,
                }),
            }),
            Effect::StartRuntime { op, .. } => Some(Event::RuntimeStarted {
                op: *op,
                isolate: None,
                generation: 0,
                result: Ok(()),
            }),
            Effect::Publish { op, .. } => Some(Event::Published {
                op: *op,
                result: Ok(()),
            }),
            Effect::EnsureDurable { op, .. } if self.answer_durability => {
                Some(Event::DurabilityChecked {
                    op: *op,
                    result: Ok(()),
                })
            }
            Effect::StopRuntime { op, .. } => Some(Event::RuntimeStopped { op: *op }),
            Effect::ReleaseOwner { op, .. } => Some(Event::OwnerReleased {
                op: *op,
                result: Ok(CasOutcome::Applied),
            }),
            Effect::AdoptReleased { op, .. } => Some(Event::SuccessorAdopted {
                op: *op,
                result: Ok(AdoptedCell {
                    node: "node-b".into(),
                    addr: "node-b:1".into(),
                    epoch: 2,
                    peer_protocol: 1,
                }),
            }),
            Effect::ScheduleTimer { timer, at_mono_ms } => {
                self.timers.push((timer.clone(), *at_mono_ms));
                None
            }
            Effect::Complete { request, result } => {
                self.completed.push((*request, result.clone()));
                None
            }
            _ => None,
        }
    }

    /// Bring `cell` to `Resident`, then finish the request so it is idle.
    fn resident(&mut self, cell: &str) -> Epoch {
        let request = self.next_request;
        self.next_request += 1;
        self.send(Event::RequestAt {
            request,
            cell: cell.into(),
            now_mono_ms: self.now,
        });
        let epoch = match self.state.phase(cell) {
            Some(Phase::Resident { epoch }) => *epoch,
            other => panic!("{cell} did not become resident: {other:?}"),
        };
        self.send(Event::ActivityFinished { request });
        epoch
    }

    fn hold(&mut self, cell: &str, epoch: Epoch) -> Result<HoldId, HoldRefusal> {
        let hold = self.next_hold;
        self.next_hold += 1;
        self.send(Event::HoldAcquired {
            cell: cell.into(),
            hold,
            epoch,
        });
        self.state.hold_verdict(cell, hold, epoch).map(|()| hold)
    }

    fn release(&mut self, cell: &str, hold: HoldId) -> Vec<Effect> {
        self.send(Event::HoldsReleased {
            cell: cell.into(),
            holds: vec![hold],
        })
    }

    fn advance(&mut self, ms: u64) {
        self.now += ms;
    }

    /// A resource sample at the current time, as the shell's sampler sends.
    fn sample(&mut self, bytes: u64) -> Vec<Effect> {
        let resident = self.state.occupied();
        self.send(Event::LoadSampled {
            load: load(resident, bytes),
            now_mono_ms: self.now,
        })
    }

    fn fire_due_timers(&mut self) -> Vec<Effect> {
        let due: Vec<(Timer, u64)> = self
            .timers
            .iter()
            .filter(|(_, at)| *at <= self.now)
            .cloned()
            .collect();
        self.timers.retain(|(_, at)| *at > self.now);
        let mut all = Vec::new();
        for (timer, _) in due {
            all.extend(self.send(Event::TimerFired {
                timer,
                now_ms: self.now,
                now_mono_ms: self.now,
            }));
        }
        all
    }

    fn resident_phase(&self, cell: &str) -> bool {
        matches!(self.state.phase(cell), Some(Phase::Resident { .. }))
    }
}

fn evicts(effects: &[Effect], cell: &str) -> bool {
    effects
        .iter()
        .any(|effect| matches!(effect, Effect::EnsureDurable { cell: c, .. } if c == cell))
}

fn revocations(effects: &[Effect], cell: &str) -> Vec<(Vec<HoldId>, HoldRevocation)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::RevokeHolds {
                cell: c,
                holds,
                reason,
            } if c == cell => Some((holds.clone(), *reason)),
            _ => None,
        })
        .collect()
}

#[test]
fn idle_eviction_skips_a_held_cell_and_takes_it_once_released() {
    let mut node = Node::new(config(10));
    let epoch = node.resident("Kernel:a");
    node.resident("Plain:b");
    let hold = node.hold("Kernel:a", epoch).expect("hold granted");
    assert!(node.state.is_active("Kernel:a"));
    assert!(!node.state.is_hibernatable("Kernel:a"));

    // Far past the idle age, over many samples: the plain cell goes, the held
    // cell never does.
    let mut taken = Vec::new();
    for _ in 0..20 {
        node.advance(10 * IDLE_MS);
        taken.extend(node.sample(0));
    }
    assert!(evicts(&taken, "Plain:b"), "the unheld cell is evicted");
    assert!(
        !evicts(&taken, "Kernel:a"),
        "the held cell is never nominated"
    );
    assert!(node.resident_phase("Kernel:a"));

    // Released: it is an ordinary idle cell again, and the next sample takes it.
    node.release("Kernel:a", hold);
    assert!(!node.state.is_active("Kernel:a"));
    assert!(node.state.is_hibernatable("Kernel:a"));
    node.advance(10 * IDLE_MS);
    let taken = node.sample(0);
    assert!(evicts(&taken, "Kernel:a"), "a released cell is evictable");
    assert!(!node.resident_phase("Kernel:a"));
}

#[test]
fn a_cell_stays_held_until_its_last_hold_is_released_and_release_is_idempotent() {
    let mut node = Node::new(config(10));
    let epoch = node.resident("Kernel:a");
    let first = node.hold("Kernel:a", epoch).unwrap();
    let second = node.hold("Kernel:a", epoch).unwrap();
    assert_eq!(node.state.hold_count("Kernel:a"), 2);
    assert_eq!(node.state.held_cells(), 1);

    node.release("Kernel:a", first);
    node.release("Kernel:a", first); // a repeated release changes nothing
    node.release("Kernel:a", 9_999); // an unknown id changes nothing
    assert_eq!(node.state.hold_count("Kernel:a"), 1);
    node.advance(10 * IDLE_MS);
    assert!(!evicts(&node.sample(0), "Kernel:a"));

    node.release("Kernel:a", second);
    node.release("Kernel:a", second);
    assert_eq!(node.state.held_cells(), 0);
    node.advance(10 * IDLE_MS);
    assert!(evicts(&node.sample(0), "Kernel:a"));
}

#[test]
fn explicit_eviction_of_a_held_cell_is_refused() {
    let mut node = Node::new(config(10));
    let epoch = node.resident("Kernel:a");
    node.hold("Kernel:a", epoch).unwrap();
    let effects = node.send(Event::EvictRequested {
        request: 900,
        cell: "Kernel:a".into(),
        now_mono_ms: node.now,
    });
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::EvictionAdmission {
            result: Err(EvictError::Refused(EvictRefusal::CellActive)),
            ..
        }
    )));
    assert!(node.resident_phase("Kernel:a"));
}

#[test]
fn a_hold_is_refused_when_it_would_exceed_the_node_budget() {
    // A ceiling of 4 gives a budget of 2 pinned cells.
    let mut node = Node::new(config(4));
    let a = node.resident("Kernel:a");
    let b = node.resident("Kernel:b");
    let c = node.resident("Kernel:c");
    node.hold("Kernel:a", a).unwrap();
    node.hold("Kernel:b", b).unwrap();

    let refusal = node
        .hold("Kernel:c", c)
        .expect_err("third cell is over budget");
    assert_eq!(
        refusal,
        HoldRefusal::NodeBudget {
            pinned: 2,
            budget: 2
        }
    );
    assert_eq!(refusal.reason(), "node_hold_budget");
    assert!(refusal.to_string().contains("budget is 2"));
    // The refusal changed nothing and promised nothing: the cell is not held and
    // stays an ordinary evictable cell.
    assert_eq!(node.state.held_cells(), 2);
    assert!(node.state.is_hibernatable("Kernel:c"));

    // A cell that is already pinned costs the budget nothing more.
    node.hold("Kernel:a", a)
        .expect("a second hold on a held cell");

    // Releasing frees budget for another cell.
    let freed = node.hold("Kernel:b", b).unwrap();
    node.release("Kernel:b", freed);
    node.release("Kernel:b", 2);
    assert_eq!(node.state.held_cells(), 1);
    node.hold("Kernel:c", c).expect("budget available again");
}

#[test]
fn holds_and_outbound_sockets_share_one_budget() {
    let mut node = Node::new(config(4)); // budget 2
    let a = node.resident("Kernel:a");
    node.resident("Kernel:b");
    let c = node.resident("Kernel:c");
    node.hold("Kernel:a", a).unwrap();
    node.send(Event::WebSocketOpened {
        cell: "Kernel:b".into(),
        websocket: 1,
        kind: WebSocketKind::Outbound,
    });
    assert!(node.state.holds_websocket("Kernel:b", 1));

    // Two cells are pinned (one by a hold, one by a socket): no room for a third.
    assert!(matches!(
        node.hold("Kernel:c", c),
        Err(HoldRefusal::NodeBudget { pinned: 2, .. })
    ));
    // And the socket side counts holds too.
    let effects = node.send(Event::WebSocketOpened {
        cell: "Kernel:c".into(),
        websocket: 2,
        kind: WebSocketKind::Outbound,
    });
    assert!(effects
        .iter()
        .any(|effect| matches!(effect, Effect::CloseWebSocket { websocket: 2, .. })));
}

#[test]
fn a_cell_may_carry_only_a_bounded_number_of_holds() {
    let mut node = Node::new(config(10));
    let epoch = node.resident("Kernel:a");
    for _ in 0..3 {
        node.hold("Kernel:a", epoch).unwrap();
    }
    assert_eq!(
        node.hold("Kernel:a", epoch),
        Err(HoldRefusal::CellLimit { max: 3 })
    );
}

#[test]
fn a_hold_is_refused_unless_the_cell_is_resident_at_the_observed_epoch() {
    let mut node = Node::new(config(10));
    let epoch = node.resident("Kernel:a");
    assert_eq!(
        node.hold("Kernel:a", epoch + 1),
        Err(HoldRefusal::CellNotResident),
        "a runtime the core already discarded cannot attach to its successor"
    );
    assert_eq!(
        node.hold("Kernel:never-seen", 1),
        Err(HoldRefusal::CellNotResident)
    );
}

#[test]
fn memory_pressure_never_sheds_a_held_cell_and_backpressures_admission() {
    let mut cfg = config(6);
    cfg.pressure = pressured();
    cfg.idle_evict_ms = None;
    let mut node = Node::new(cfg);
    let a = node.resident("Kernel:a");
    let b = node.resident("Kernel:b");
    node.hold("Kernel:a", a).unwrap();
    node.hold("Kernel:b", b).unwrap();

    // Far above the ceiling, sample after sample: every cell is held, so there
    // is nothing to take, and nothing is taken.
    let mut effects = Vec::new();
    for _ in 0..10 {
        node.advance(1_000);
        effects.extend(node.sample(1_000_000));
    }
    assert!(!effects.iter().any(|effect| matches!(
        effect,
        Effect::EnsureDurable { .. } | Effect::StopRuntime { .. }
    )));
    assert!(node.resident_phase("Kernel:a") && node.resident_phase("Kernel:b"));

    // The node is latched shedding, so it refuses new activations rather than
    // taking room from a held cell: the new cell waits for capacity and is not
    // admitted while the node stays over its limit.
    let request = node.next_request;
    node.send(Event::RequestAt {
        request,
        cell: "Plain:new".into(),
        now_mono_ms: node.now,
    });
    assert!(!node.resident_phase("Plain:new"));
    assert!(node
        .completed
        .iter()
        .all(|(id, result)| *id != request || result.is_err()));
    assert!(node.resident_phase("Kernel:a") && node.resident_phase("Kernel:b"));
}

#[test]
fn memory_pressure_sheds_only_the_unheld_cells() {
    let mut cfg = config(8);
    cfg.pressure = pressured();
    cfg.idle_evict_ms = None;
    let mut node = Node::new(cfg);
    let held = node.resident("Kernel:held");
    for name in ["Plain:1", "Plain:2", "Plain:3", "Plain:4"] {
        node.resident(name);
    }
    node.hold("Kernel:held", held).unwrap();

    let mut effects = Vec::new();
    for _ in 0..12 {
        node.advance(1_000);
        effects.extend(node.sample(1_000_000));
    }
    assert!(!evicts(&effects, "Kernel:held"));
    assert!(
        ["Plain:1", "Plain:2", "Plain:3", "Plain:4"]
            .iter()
            .any(|cell| evicts(&effects, cell)),
        "pressure still takes unheld cells"
    );
    assert!(node.resident_phase("Kernel:held"));
}

#[test]
fn a_drain_tells_the_holder_and_waits_for_it_at_most_the_grace_period() {
    let mut node = Node::new(config(10));
    let epoch = node.resident("Kernel:a");
    let first = node.hold("Kernel:a", epoch).unwrap();
    let second = node.hold("Kernel:a", epoch).unwrap();

    let effects = node.send(Event::ReleaseAll);
    // The holder is told, once, with every hold named and the reason.
    let told = revocations(&effects, "Kernel:a");
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].1, HoldRevocation::Drain);
    let mut named = told[0].0.clone();
    named.sort_unstable();
    assert_eq!(named, vec![first, second]);
    // A grace timer bounds the wait.
    assert!(node.timers.iter().any(|(timer, at)| matches!(
        timer,
        Timer::HoldRevokeGrace { cell, .. } if cell == "Kernel:a"
    ) && *at == node.now + GRACE_MS));
    // While the handlers wind down the cell is still resident and held.
    assert!(node.resident_phase("Kernel:a"));
    assert_eq!(node.state.revoking_holds(), 1);
    assert_eq!(node.state.drain_pins()[0].hold_count, 2);
    // A hold taken during the revocation is refused.
    assert_eq!(node.hold("Kernel:a", epoch), Err(HoldRefusal::CellLeaving));

    // The handlers settle: each hold is released, and the drain proceeds.
    node.release("Kernel:a", first);
    assert!(node.resident_phase("Kernel:a"), "one hold still stands");
    let effects = node.release("Kernel:a", second);
    assert!(evicts(&effects, "Kernel:a") || !node.resident_phase("Kernel:a"));
    assert_eq!(node.state.occupied(), 0, "the drained cell left the node");
    assert_eq!(node.state.held_cells(), 0);
}

#[test]
fn a_holder_that_never_answers_cannot_block_a_drain_past_the_grace_period() {
    let mut node = Node::new(config(10));
    let epoch = node.resident("Kernel:a");
    node.hold("Kernel:a", epoch).unwrap();
    node.send(Event::ReleaseAll);
    assert!(node.resident_phase("Kernel:a"));

    node.advance(GRACE_MS - 1);
    assert!(node.fire_due_timers().is_empty(), "not yet");
    assert!(node.resident_phase("Kernel:a"));

    node.advance(1);
    node.fire_due_timers();
    assert_eq!(
        node.state.held_cells(),
        0,
        "the grace period dropped the hold"
    );
    assert_eq!(node.state.occupied(), 0, "and the drain went through");
}

#[test]
fn a_stale_grace_timer_does_nothing_to_a_later_hold() {
    let mut node = Node::new(config(10));
    let epoch = node.resident("Kernel:a");
    let hold = node.hold("Kernel:a", epoch).unwrap();
    node.send(Event::RevokeHolds {
        cell: "Kernel:a".into(),
    });
    // The handlers finish first, so the revocation is over.
    node.release("Kernel:a", hold);
    assert_eq!(node.state.revoking_holds(), 0);
    let later = node.hold("Kernel:a", epoch).expect("a new hold is allowed");

    node.advance(GRACE_MS);
    node.fire_due_timers();
    assert!(
        node.state.holds_hold("Kernel:a", later),
        "the stale timer is inert"
    );
}

#[test]
fn an_operator_can_revoke_holds_without_evicting_the_cell() {
    let mut node = Node::new(config(10));
    let epoch = node.resident("Kernel:a");
    let hold = node.hold("Kernel:a", epoch).unwrap();
    let effects = node.send(Event::RevokeHolds {
        cell: "Kernel:a".into(),
    });
    assert_eq!(
        revocations(&effects, "Kernel:a"),
        vec![(vec![hold], HoldRevocation::Operator)]
    );
    // Repeating it while one is under way tells nobody twice.
    let again = node.send(Event::RevokeHolds {
        cell: "Kernel:a".into(),
    });
    assert!(revocations(&again, "Kernel:a").is_empty());
    node.release("Kernel:a", hold);
    // The cell is still resident and is now an ordinary idle cell.
    assert!(node.resident_phase("Kernel:a"));
    assert!(node.state.is_hibernatable("Kernel:a"));
}

#[test]
fn a_forced_swap_tells_the_holder_before_it_stops_the_runtime() {
    let mut node = Node::new(config(10));
    let epoch = node.resident("Kernel:a");
    node.hold("Kernel:a", epoch).unwrap();

    // The class is eager, so the swap does not wait for a safe point. A hold is
    // not a safe point: without the revocation the cell would be stuck.
    let effects = node.send(Event::GenerationChanged {
        generation: 2,
        now_mono_ms: node.now,
        max_age_ms: 60_000,
        eager_classes: vec!["Kernel".into()],
    });
    assert_eq!(
        revocations(&effects, "Kernel:a")
            .iter()
            .map(|r| r.1)
            .collect::<Vec<_>>(),
        vec![HoldRevocation::Swap]
    );
    assert!(!effects
        .iter()
        .any(|effect| matches!(effect, Effect::StopRuntime { .. })));
}

#[test]
fn an_unforced_swap_waits_for_a_held_cell_until_the_maximum_age() {
    let mut node = Node::new(config(10));
    let epoch = node.resident("Kernel:a");
    node.hold("Kernel:a", epoch).unwrap();
    let effects = node.send(Event::GenerationChanged {
        generation: 2,
        now_mono_ms: node.now,
        max_age_ms: 30_000,
        eager_classes: Vec::new(),
    });
    assert!(revocations(&effects, "Kernel:a").is_empty());
    assert!(node.resident_phase("Kernel:a"));

    node.advance(30_000);
    let effects = node.sample(0);
    assert_eq!(
        revocations(&effects, "Kernel:a")
            .iter()
            .map(|r| r.1)
            .collect::<Vec<_>>(),
        vec![HoldRevocation::Swap],
        "past the maximum age the swap is forced, and the holder is told"
    );
}

#[test]
fn a_fence_leaves_no_holds_behind() {
    let mut node = Node::new(config(10));
    let epoch = node.resident("Kernel:a");
    node.hold("Kernel:a", epoch).unwrap();
    node.send(Event::NodeFenced);
    assert_eq!(node.state.held_cells(), 0);
    assert!(!node.state.is_active("Kernel:a"));
    assert!(node.state.validate().is_ok());
}

#[test]
fn a_reset_cell_leaves_no_holds_and_they_cannot_attach_to_its_successor() {
    let mut node = Node::new(config(10));
    let epoch = node.resident("Kernel:a");
    let hold = node.hold("Kernel:a", epoch).unwrap();

    // A write on the cell fails its durability proof: celld resets the cell,
    // which discards the isolate and every hold it took.
    let request = node.next_request;
    node.next_request += 1;
    node.send(Event::RequestAt {
        request,
        cell: "Kernel:a".into(),
        now_mono_ms: node.now,
    });
    let effects = node.send(Event::Output {
        request,
        channel: Channel::Response,
        position: Some(7),
        observed: None,
        epoch: None,
    });
    let op = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::AwaitDurable { op, .. } => Some(*op),
            _ => None,
        })
        .expect("the write opened a durability gate");
    node.send(Event::DurableReached {
        op,
        result: Err(Failure::Definite),
        source: ProofSource::Fleet,
    });
    assert!(!node.resident_phase("Kernel:a"), "the cell was reset");
    assert_eq!(node.state.held_cells(), 0);
    assert!(!node.state.holds_hold("Kernel:a", hold));

    // The next activation starts clean, and the old isolate's hold id means
    // nothing to it: releasing it is a no-op and the new runtime is unheld.
    node.send(Event::ActivityFinished { request });
    node.resident("Kernel:a");
    node.release("Kernel:a", hold);
    assert_eq!(node.state.held_cells(), 0);
    assert!(node.state.is_hibernatable("Kernel:a"));
}

#[test]
fn rebalancing_never_moves_a_held_cell() {
    let mut node = Node::new(config(10));
    let epoch = node.resident("Kernel:a");
    node.hold("Kernel:a", epoch).unwrap();
    let effects = node.send(Event::Rebalance { cells: 10 });
    assert!(!effects.iter().any(|effect| matches!(
        effect,
        Effect::ReleaseOwner { .. } | Effect::EnsureDurable { .. }
    )));
    assert!(node.resident_phase("Kernel:a"));
}

#[test]
fn a_held_cell_is_not_lent_to_stateless_worker_requests() {
    let mut node = Node::new(config(10));
    let epoch = node.resident("Kernel:a");
    node.hold("Kernel:a", epoch).unwrap();
    let request = node.next_request;
    node.next_request += 1;
    let effects = node.send(Event::WorkerRequest { request });
    assert!(effects
        .iter()
        .any(|effect| matches!(effect, Effect::CompleteWorker { route: None, .. })));
}
