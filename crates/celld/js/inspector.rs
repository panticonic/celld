// Copyright 2026 Deno Land Inc. Apache-2.0 license.

//! The V8 half of the inspector: a `V8Inspector` for one isolate.
//!
//! `crate::inspector` queues protocol messages for an isolate; this module
//! dispatches them, on a turn that holds the isolate's lock.
//!
//! **Where code runs.** Isolates belong to no thread. A turn locks the
//! isolate on whichever Tokio worker holds the slot's permit, so nothing here
//! may assume a thread. Everything that touches V8 runs inside such a turn: a
//! pump turn (`Worker::drain_inspector`) for ordinary messages, and, when the
//! debugger pauses JavaScript, the turn that was already running.
//!
//! **Pausing.** V8 calls `run_message_loop_on_pause` from inside a turn, on
//! its thread, and expects the call not to return until the debugger resumes.
//! That blocks a thread and keeps the isolate's turn permit, so the
//! whole isolate is frozen: every cell it hosts queues behind the permit,
//! exactly as when a debugger stops a shared process. The loop pumps this
//! isolate's own inbox, so resume and evaluation requests reach it without
//! another turn. Three things keep that from hurting the node:
//!
//! - Only the paused isolate stops. Other isolates take their own permit, so
//!   cells in them keep serving.
//! - The loop runs inside `block_in_place`, so the worker hands its queued
//!   tasks (the socket task that delivers `Debugger.paused` among them) to
//!   another thread first. At most `MAX_PAUSED_ISOLATES` pauses may hold a
//!   thread; one beyond that is declined (the loop returns at once, so
//!   execution continues, and a warning says why).
//! - A pause ends when the last debugger session disconnects, so a vanished
//!   client cannot wedge the isolate.
//!
//! The wall-clock deadlines celld arms for a request keep running while an
//! isolate is paused (see `docs/services/inspector.md`); `celld dev` disarms
//! them.

use super::*;
use crate::inspector::{Inbound, Target};
use std::cell::Cell;
use std::rc::Rc;
use v8::inspector::{
    Channel, ChannelImpl, StringBuffer, StringView, V8Inspector, V8InspectorClient,
    V8InspectorClientImpl, V8InspectorClientTrustLevel, V8InspectorSession,
};

/// The one context group an isolate has: every cell shares its realm.
const CONTEXT_GROUP: i32 = 1;

/// Pauses in flight across the node, and the most that are allowed. Each one
/// holds a thread (see `crate::inspector::blocking`), so the count is bounded
/// even though no pause takes a Tokio worker for good.
static PAUSED_ISOLATES: AtomicUsize = AtomicUsize::new(0);
const MAX_PAUSED_ISOLATES: usize = 16;

/// How often a paused loop re-checks that a debugger is still attached.
const PAUSE_POLL: Duration = Duration::from_millis(250);

/// A message from V8 to one debugger connection.
struct SessionChannel {
    out: tokio::sync::mpsc::UnboundedSender<String>,
}

fn buffer_text(message: &UniquePtr<StringBuffer>) -> Option<String> {
    let view = message.as_ref()?.string();
    // An 8-bit view carries UTF-8 here, as the protocol's JSON is serialized
    // by V8 itself; a 16-bit view is UTF-16.
    Some(match view {
        StringView::U8(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        StringView::U16(units) => String::from_utf16_lossy(&units),
    })
}

use v8::UniquePtr;

impl ChannelImpl for SessionChannel {
    fn send_response(&self, _call_id: i32, message: UniquePtr<StringBuffer>) {
        if let Some(text) = buffer_text(&message) {
            let _ = self.out.send(text);
        }
    }

    fn send_notification(&self, message: UniquePtr<StringBuffer>) {
        if let Some(text) = buffer_text(&message) {
            let _ = self.out.send(text);
        }
    }

    fn flush_protocol_notifications(&self) {}
}

/// State the client callbacks and the isolate's drain share. It is `Rc`
/// because it never leaves the thread holding the isolate lock; see the
/// `Send` assertion on [`IsolateInspector`].
struct Shared {
    target: Arc<Target>,
    sessions: RefCell<HashMap<u64, Rc<V8InspectorSession>>>,
    /// `quit_message_loop_on_pause` was called for the pause in progress.
    quit: Cell<bool>,
    /// The `V8Inspector` these callbacks belong to, for a connection that
    /// arrives while paused. It owns the client that owns this `Shared`, so it
    /// outlives every callback.
    inspector: Cell<*const V8Inspector>,
    isolate: v8::UnsafeRawIsolatePtr,
    /// The realm the protocol evaluates in when a command names no context:
    /// there is one, and every cell shares it.
    default_context: RefCell<Option<v8::Global<v8::Context>>>,
}

impl Shared {
    fn process(&self, message: Inbound) {
        match message {
            Inbound::Connect { session, out } => {
                // SAFETY: see `inspector`; set before the first message can
                // be processed and valid until the inspector is dropped.
                let Some(inspector) = (unsafe { self.inspector.get().as_ref() }) else {
                    return;
                };
                let channel = Channel::new(Box::new(SessionChannel { out }));
                let connected = inspector.connect(
                    CONTEXT_GROUP,
                    channel,
                    StringView::empty(),
                    V8InspectorClientTrustLevel::FullyTrusted,
                );
                self.sessions
                    .borrow_mut()
                    .insert(session, Rc::new(connected));
            }
            Inbound::Message { session, text } => {
                // Cloned out so no borrow is held while V8 runs: a
                // `Runtime.evaluate` of `debugger;` re-enters this module
                // through the pause loop.
                let found = self.sessions.borrow().get(&session).cloned();
                if let Some(found) = found {
                    found.dispatch_protocol_message(StringView::from(text.as_bytes()));
                }
            }
            Inbound::Disconnect { session } => {
                self.sessions.borrow_mut().remove(&session);
            }
        }
    }
}

struct Client {
    shared: Rc<Shared>,
}

impl Client {
    /// Whether this pause may take another thread. See the module comment.
    fn pause_allowed() -> bool {
        if PAUSED_ISOLATES.fetch_add(1, Ordering::AcqRel) >= MAX_PAUSED_ISOLATES {
            PAUSED_ISOLATES.fetch_sub(1, Ordering::AcqRel);
            return false;
        }
        true
    }
}

impl V8InspectorClientImpl for Client {
    fn run_message_loop_on_pause(&self, _context_group_id: i32) {
        let shared = &self.shared;
        if !Self::pause_allowed() {
            tracing::warn!(
                event = "inspector_pause_declined",
                target = shared.target.id(),
                "the debugger paused an isolate, but too many isolates are already paused; \
                 execution continues"
            );
            return;
        }
        shared.quit.set(false);
        shared.target.set_paused(true);
        // A debugger that stops answering is a disconnect: the socket task
        // pushes `Disconnect`, and the check below sees no session left.
        // The loop blocks this thread for as long as the debugger holds the
        // pause. `blocking` first hands the worker's other tasks (the socket
        // task that will deliver `Debugger.paused`, among them) to another
        // thread; blocking a worker outright would strand whatever it had
        // queued.
        crate::inspector::blocking(|| {
            while !shared.quit.get() && !shared.sessions.borrow().is_empty() {
                if let Some(message) = shared.target.pop_wait(PAUSE_POLL) {
                    shared.process(message);
                }
            }
        });
        shared.target.set_paused(false);
        PAUSED_ISOLATES.fetch_sub(1, Ordering::AcqRel);
    }

    fn quit_message_loop_on_pause(&self) {
        self.shared.quit.set(true);
    }

    fn ensure_default_context_in_group(
        &self,
        _context_group_id: i32,
    ) -> Option<v8::Local<'_, v8::Context>> {
        let default = self.shared.default_context.borrow();
        let global = default.as_ref()?;
        // SAFETY: V8 calls this from inside a dispatch or a paused loop, with
        // the isolate locked and a handle scope open. A callback scope opens no
        // scope of its own, so the handle lands in that open one and outlives
        // this call, as the trait's signature requires.
        unsafe {
            let mut isolate = v8::Isolate::from_raw_isolate_ptr(self.shared.isolate);
            let scope = std::pin::pin!(v8::CallbackScope::new(&mut isolate));
            let scope = &mut scope.init();
            let local = v8::Local::new(scope, global);
            Some(std::mem::transmute::<v8::Local<'_, v8::Context>, v8::Local<'_, v8::Context>>(local))
        }
    }

    // `Runtime.runIfWaitingForDebugger`: celld never holds an isolate back
    // waiting for a debugger, so there is nothing to release.
    fn run_if_waiting_for_debugger(&self, _context_group_id: i32) {}
}

/// An isolate's inspector, held by its `WorkerIsolate`.
pub(super) struct IsolateInspector {
    // Dropped before `inspector` (declaration order): sessions reference it.
    shared: Rc<Shared>,
    inspector: Box<V8Inspector>,
}

// SAFETY: `Rc` and `Cell` make this `!Send`, but the value is only touched
// while the isolate's `v8::Locker` is held (a turn, or `WorkerIsolate::drop`
// under `lock()`), and it never hands an `Rc` clone to anything that outlives
// that lock. The lock is exclusive, so at most one thread uses it at a time,
// and taking and releasing it orders the accesses between threads.
unsafe impl Send for IsolateInspector {}
unsafe impl Sync for IsolateInspector {}

impl IsolateInspector {
    /// Create the inspector of `isolate` and register `context` with it. Run
    /// while the isolate is entered, before the context runs any script.
    pub(super) fn attach(isolate: &mut v8::Isolate, target: Arc<Target>) -> Self {
        let shared = Rc::new(Shared {
            target,
            sessions: RefCell::new(HashMap::new()),
            quit: Cell::new(false),
            inspector: Cell::new(std::ptr::null()),
            // SAFETY: the pointer is only turned back into an `Isolate` while
            // the isolate is locked, in `ensure_default_context_in_group`.
            isolate: unsafe { isolate.as_raw_isolate_ptr() },
            default_context: RefCell::new(None),
        });
        let client = V8InspectorClient::new(Box::new(Client {
            shared: shared.clone(),
        }));
        let inspector = Box::new(V8Inspector::create(isolate, client));
        shared.inspector.set(&*inspector);
        Self { shared, inspector }
    }

    pub(super) fn context_created(
        &self,
        isolate: &v8::Isolate,
        context: v8::Local<v8::Context>,
        name: &str,
    ) {
        *self.shared.default_context.borrow_mut() = Some(v8::Global::new(isolate, context));
        let aux = format!(
            r#"{{"isDefault":true,"type":"default","frameId":"{}"}}"#,
            self.shared.target.id()
        );
        self.inspector.context_created(
            context,
            CONTEXT_GROUP,
            StringView::from(name.as_bytes()),
            StringView::from(aux.as_bytes()),
        );
    }

    pub(super) fn context_destroyed(&self, context: v8::Local<v8::Context>) {
        self.inspector.context_destroyed(context);
    }

    /// Dispatch everything queued. Returns whether anything was.
    fn drain(&self) -> bool {
        let mut any = false;
        while let Some(message) = self.shared.target.try_pop() {
            any = true;
            self.shared.process(message);
        }
        any
    }

    pub(super) fn target(&self) -> &Arc<Target> {
        &self.shared.target
    }
}

impl Drop for IsolateInspector {
    fn drop(&mut self) {
        // Sessions go first, then `inspector`; the caller holds the isolate
        // lock. Dropping the sessions closes every debugger socket.
        self.shared.sessions.borrow_mut().clear();
        self.shared.default_context.borrow_mut().take();
        self.shared.target.close();
    }
}

impl Worker {
    /// The inspector target of this isolate, when the node runs an inspector.
    pub(crate) fn inspector_target(&self) -> Option<Arc<Target>> {
        self.inner
            .as_ref()?
            .inspector
            .as_ref()
            .map(|inspector| inspector.target().clone())
    }

    /// The thread that must run this isolate's turns while a CPU profile
    /// records. See `crate::inspector::PinnedThread`.
    pub(crate) fn inspector_pin(&self) -> Option<Arc<crate::inspector::PinnedThread>> {
        self.inner.as_ref()?.inspector.as_ref()?.target().pinned_thread()
    }

    /// Note that a cell entered or left this isolate, for its target's title.
    pub(super) fn inspector_cell(&self, cell: &str, present: bool) {
        if let Some(inspector) = self.inner.as_ref().and_then(|inner| inner.inspector.as_ref()) {
            inspector.target().cell_adopted(cell, present);
        }
    }

    /// Dispatch the protocol messages queued for this isolate. A turn.
    pub(crate) fn drain_inspector(&mut self) {
        let Some(inner) = self.inner.as_ref() else {
            return;
        };
        let Some(inspector) = inner.inspector.as_ref() else {
            return;
        };
        let (mut locker, _cells) = inner.lock();
        v8::scope!(let hs, &mut *locker);
        // The microtask checkpoint is what lets a `Runtime.evaluate` with
        // `awaitPromise` settle: an idle isolate runs none of its own.
        if inspector.drain() {
            hs.perform_microtask_checkpoint();
            // V8 finishes some commands (`HeapProfiler.takeHeapSnapshot`,
            // `HeapProfiler.collectGarbage`) in a task it posts to the
            // isolate's foreground queue, and answers only then. celld pumps
            // that queue once per event turn, so an idle isolate would never
            // answer; run it dry here, bounded so a task that reposts itself
            // cannot hold the turn.
            for _ in 0..1024 {
                if !v8::Platform::pump_message_loop(&v8::V8::get_current_platform(), hs, false) {
                    break;
                }
                hs.perform_microtask_checkpoint();
            }
        }
    }
}
