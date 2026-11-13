// Copyright 2026 Deno Land Inc. Apache-2.0 license.

// The inspector is an operator tool outside the execution boundary: it binds a
// real socket and uses real host tasks.
#![allow(clippy::disallowed_methods)]

//! The V8 inspector endpoint: Chrome DevTools Protocol over WebSocket, one
//! target per live isolate.
//!
//! This module is everything that is not V8. It parses the operator's
//! configuration, keeps the registry of targets, serves `/json/version`,
//! `/json/list` and `/<target-id>` (the shape workerd's `--inspector-addr`
//! serves, so a relay written for workerd works unchanged), and moves protocol
//! messages between a socket and the isolate that owns them. The V8 half,
//! which creates a `V8Inspector` for an isolate and dispatches the messages on
//! the isolate's own turn, is `js::inspector`.
//!
//! **Security.** An inspector connection is full code execution in the
//! isolate, and through it in every cell that isolate hosts: it reads their
//! heap, calls their storage, and runs arbitrary JavaScript with their
//! authority. There is no authentication. The listener therefore refuses a
//! non-loopback address unless the operator also passes
//! `--inspector-allow-non-loopback`, and it refuses a request whose `Host`
//! header is not an IP literal or `localhost` (the DNS-rebinding defence Node
//! uses). Put a tunnel in front of it, as for any debugger port.
//!
//! **Threading.** Isolates belong to no thread (see `pool`): a turn locks the
//! isolate on whichever Tokio worker holds the slot's permit. A message from a
//! socket therefore becomes a turn of its own: [`Target::push`] queues it and
//! schedules one turn of the owning slot, in which the isolate's inspector
//! drains the queue. A paused isolate is the exception. It sits inside a turn
//! that never returns until the debugger resumes, so that turn drains the
//! queue itself; see `js::inspector` for how that stays safe.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::time::Duration;

use anyhow::{bail, Context as _, Result};
use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use tokio::sync::mpsc;

use crate::pool::Slot;

/// The operator's inspector settings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub addr: SocketAddr,
}

impl Config {
    /// Parse `HOST:PORT`. `HOST` is an IP literal or `localhost`; anything
    /// that is not loopback needs `allow_non_loopback`.
    pub fn parse(value: &str, allow_non_loopback: bool) -> Result<Self> {
        let value = value.trim();
        let addr = match value.parse::<SocketAddr>() {
            Ok(addr) => addr,
            Err(_) => {
                let port = value
                    .strip_prefix("localhost:")
                    .with_context(|| {
                        format!("--inspector-addr must be IP:PORT or localhost:PORT, not {value:?}")
                    })?
                    .parse::<u16>()
                    .with_context(|| format!("--inspector-addr has an invalid port: {value:?}"))?;
                SocketAddr::new(IpAddr::from([127, 0, 0, 1]), port)
            }
        };
        if !addr.ip().is_loopback() && !allow_non_loopback {
            bail!(
                "--inspector-addr {addr} is not a loopback address. The inspector gives full \
                 code execution in every cell and has no authentication; bind loopback and \
                 tunnel to it, or pass --inspector-allow-non-loopback to accept the exposure"
            );
        }
        Ok(Self { addr })
    }
}

static CONFIG: OnceLock<Config> = OnceLock::new();

/// Record the configuration the command line or environment selected. Called
/// once, before the node starts; a second call is ignored.
pub fn configure(config: Config) {
    let _ = CONFIG.set(config);
}

/// Whether isolates should create inspector targets. Cheap: it gates every
/// isolate load.
pub fn enabled() -> bool {
    CONFIG.get().is_some()
}

/// What an isolate is for, which decides how its target is named.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetKind {
    /// An isolate of a deployment's script pool: it hosts cells, or serves
    /// the script's stateless requests.
    Pool,
    /// The isolate of a dynamic Worker (Worker Loader). It hosts exactly one
    /// Worker and no cells of its own.
    Dynamic,
}

struct TargetInfo {
    kind: TargetKind,
    script: String,
    classes: Vec<String>,
    cells: BTreeSet<String>,
}

/// One protocol message headed for an isolate's inspector.
pub enum Inbound {
    Connect {
        session: u64,
        out: mpsc::UnboundedSender<String>,
    },
    Message {
        session: u64,
        text: String,
    },
    Disconnect {
        session: u64,
    },
}

type Job = Box<dyn FnOnce() + Send + 'static>;

/// A thread that runs an isolate's turns while a CPU profile is recording.
///
/// V8 samples an isolate by signalling the thread that started the profile,
/// and a sample is taken only while that thread is running the isolate. celld
/// moves an isolate's turns between Tokio workers, so a profile started on one
/// worker would mostly miss them. While a target has a profile recording, its
/// turns run here instead: [`PinnedThread::run`] hands the turn to this thread
/// and the calling worker waits, which is the time it would have spent running
/// the turn itself.
pub struct PinnedThread {
    jobs: Mutex<Option<std::sync::mpsc::Sender<Job>>>,
}

impl PinnedThread {
    fn spawn(name: &str) -> Arc<Self> {
        let (jobs, queue) = std::sync::mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name(name.to_string())
            // V8 sizes its stack limit from the thread; match a Tokio worker's
            // headroom for deep JavaScript.
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                while let Ok(job) = queue.recv() {
                    job();
                }
            })
            .expect("spawn the inspector profiling thread");
        Arc::new(Self {
            jobs: Mutex::new(Some(jobs)),
        })
    }

    /// Run `f` on the pinned thread and wait for its result. A panic in `f`
    /// resumes on the caller.
    pub fn run<T>(&self, f: impl FnOnce() -> T) -> T {
        let (done, result) = std::sync::mpsc::sync_channel::<std::thread::Result<T>>(1);
        let call: Box<dyn FnOnce() + '_> = Box::new(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            let _ = done.send(outcome);
        });
        // SAFETY: this function does not return until the job has finished and
        // sent its result, so everything `f` borrows outlives the job, as with
        // `std::thread::scope`. `Send` is asserted rather than proven: the
        // closure is the body of an isolate turn, which already runs on
        // whichever Tokio worker holds the turn permit, and the permit and the
        // isolate lock are held by the caller for the whole call.
        let call: Job = unsafe { std::mem::transmute::<Box<dyn FnOnce() + '_>, Job>(call) };
        let sent = self
            .jobs
            .lock()
            .expect("pinned thread poisoned")
            .as_ref()
            .map(|jobs| jobs.send(call).is_ok());
        assert_eq!(sent, Some(true), "the profiling thread is gone");
        match result.recv().expect("the profiling thread dropped a job") {
            Ok(value) => value,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
}

impl Drop for PinnedThread {
    fn drop(&mut self) {
        // Closing the queue ends the thread's loop.
        self.jobs.lock().expect("pinned thread poisoned").take();
    }
}

/// One inspectable isolate.
pub struct Target {
    id: String,
    info: Mutex<TargetInfo>,
    inbox: Mutex<VecDeque<Inbound>>,
    wake: Condvar,
    pump_scheduled: AtomicBool,
    slot: OnceLock<Weak<Slot>>,
    closed: AtomicBool,
    paused: AtomicBool,
    /// Sessions with a CPU profile recording, and the thread that runs this
    /// isolate's turns while any does.
    profiling: Mutex<BTreeSet<u64>>,
    pinned: Mutex<Option<Arc<PinnedThread>>>,
}

static HOST_WORKERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many Tokio workers the node runs, as measured when the listener
/// started. Zero when the inspector is not running.
pub fn host_workers() -> usize {
    HOST_WORKERS.load(Ordering::Relaxed)
}

static NEXT_TARGET: AtomicU64 = AtomicU64::new(1);
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

fn registry() -> &'static Mutex<BTreeMap<String, Arc<Target>>> {
    static REGISTRY: OnceLock<Mutex<BTreeMap<String, Arc<Target>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Keep an id to characters a URL path carries without escaping, because a
/// relay copies the id out of `webSocketDebuggerUrl` verbatim.
fn id_component(value: &str) -> String {
    let cleaned = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    if cleaned.is_empty() {
        "worker".to_string()
    } else {
        cleaned
    }
}

impl Target {
    /// Make a target. It is listed only once [`Target::attach`] gives it the
    /// slot that can run its turns.
    pub fn new(kind: TargetKind, script: &str, classes: &[String]) -> Arc<Self> {
        let sequence = NEXT_TARGET.fetch_add(1, Ordering::Relaxed);
        let scope = match kind {
            TargetKind::Pool => "core:user",
            TargetKind::Dynamic => "core:dynamic",
        };
        Arc::new(Self {
            id: format!("{scope}:{}:isolate-{sequence}", id_component(script)),
            info: Mutex::new(TargetInfo {
                kind,
                script: script.to_string(),
                classes: classes.to_vec(),
                cells: BTreeSet::new(),
            }),
            inbox: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
            pump_scheduled: AtomicBool::new(false),
            slot: OnceLock::new(),
            closed: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            profiling: Mutex::new(BTreeSet::new()),
            pinned: Mutex::new(None),
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Give the target the slot that runs its turns and list it.
    pub fn attach(self: &Arc<Self>, slot: &Arc<Slot>) {
        if self.slot.set(Arc::downgrade(slot)).is_err() {
            return;
        }
        if !self.closed.load(Ordering::Acquire) {
            registry()
                .lock()
                .expect("inspector registry poisoned")
                .insert(self.id.clone(), self.clone());
        }
    }

    /// The isolate is gone: unlist the target and release any thread waiting
    /// for it.
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        registry()
            .lock()
            .expect("inspector registry poisoned")
            .remove(&self.id);
        self.wake.notify_all();
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// A cell entered or left this isolate.
    pub fn cell_adopted(&self, cell: &str, present: bool) {
        let mut info = self.info.lock().expect("inspector target poisoned");
        if present {
            info.cells.insert(cell.to_string());
        } else {
            info.cells.remove(cell);
        }
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
    }

    /// The `title` of the target. Isolates are shared by cells, and a
    /// breakpoint or a pause in one stops all of them, so the title lists
    /// who is inside.
    pub fn title(&self) -> String {
        const LISTED: usize = 6;
        let info = self.info.lock().expect("inspector target poisoned");
        let mut title = match info.kind {
            TargetKind::Pool => format!("{} (celld isolate", info.script),
            TargetKind::Dynamic => format!("{} (celld dynamic worker isolate", info.script),
        };
        if !info.classes.is_empty() {
            title.push_str(&format!("; classes {}", info.classes.join(", ")));
        }
        if info.kind == TargetKind::Pool {
            if info.cells.is_empty() {
                title.push_str("; no cells now, serves stateless requests");
            } else {
                let listed = info
                    .cells
                    .iter()
                    .take(LISTED)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                title.push_str(&format!(
                    "; shared by {} cell{}: {listed}",
                    info.cells.len(),
                    if info.cells.len() == 1 { "" } else { "s" }
                ));
                if info.cells.len() > LISTED {
                    title.push_str(&format!(" +{} more", info.cells.len() - LISTED));
                }
            }
        }
        title.push(')');
        if self.paused.load(Ordering::Acquire) {
            title.push_str(" [paused]");
        }
        title
    }

    fn description(&self) -> String {
        "celld V8 isolate. An isolate runs every cell placed in it on one heap, so a \
         breakpoint, a pause or a profile here covers all of them."
            .to_string()
    }

    fn script(&self) -> String {
        self.info
            .lock()
            .expect("inspector target poisoned")
            .script
            .clone()
    }

    /// The thread this isolate's turns must run on, while a profile records.
    pub fn pinned_thread(&self) -> Option<Arc<PinnedThread>> {
        self.pinned.lock().expect("pinned thread poisoned").clone()
    }

    /// Track which sessions record a CPU profile. A session that starts one
    /// pins the isolate's turns to a single thread before the message that
    /// starts the profile is dispatched, so the sampler targets the thread the
    /// isolate will keep running on. The pin is released once no session
    /// records, after the turn that stopped the last one.
    fn note_profiling(&self, message: &Inbound) {
        let mut profiling = self.profiling.lock().expect("inspector profiling poisoned");
        match message {
            Inbound::Message { session, text } if text.contains("\"Profiler.start\"") => {
                profiling.insert(*session);
                let mut pinned = self.pinned.lock().expect("pinned thread poisoned");
                if pinned.is_none() {
                    *pinned = Some(PinnedThread::spawn("celld-inspector-profile"));
                }
            }
            Inbound::Message { session, text } if text.contains("\"Profiler.stop\"") => {
                profiling.remove(session);
            }
            Inbound::Disconnect { session } => {
                profiling.remove(session);
            }
            _ => {}
        }
    }

    /// Release the pinned thread if no session records any longer. Called by a
    /// turn after it drained the queue.
    pub fn release_pin_if_idle(&self) {
        let profiling = self.profiling.lock().expect("inspector profiling poisoned");
        if profiling.is_empty() {
            self.pinned.lock().expect("pinned thread poisoned").take();
        }
    }

    /// Queue a message for the isolate and make sure a turn will drain it.
    pub fn push(self: &Arc<Self>, message: Inbound) {
        self.note_profiling(&message);
        self.inbox
            .lock()
            .expect("inspector inbox poisoned")
            .push_back(message);
        self.wake.notify_all();
        self.schedule_pump();
    }

    /// Run one turn of the owning slot that drains the queue. At most one is
    /// pending: the turn clears the flag before it drains, so a message that
    /// arrives after the drain schedules the next one.
    fn schedule_pump(self: &Arc<Self>) {
        if self.is_closed() || self.pump_scheduled.swap(true, Ordering::AcqRel) {
            return;
        }
        let Some(slot) = self.slot.get().and_then(Weak::upgrade) else {
            self.pump_scheduled.store(false, Ordering::Release);
            return;
        };
        let target = self.clone();
        tokio::spawn(async move {
            // `None` is a slot freed while the pump waited: nothing to drain.
            let _ = slot
                .turn_if_live(|worker| {
                    target.pump_scheduled.store(false, Ordering::Release);
                    worker.drain_inspector();
                    target.release_pin_if_idle();
                })
                .await;
            target.pump_scheduled.store(false, Ordering::Release);
        });
    }

    /// The next queued message, if any. Called only by the isolate's turn.
    pub fn try_pop(&self) -> Option<Inbound> {
        self.inbox
            .lock()
            .expect("inspector inbox poisoned")
            .pop_front()
    }

    /// The next queued message, waiting up to `timeout`. Called only by a
    /// paused isolate's turn.
    pub fn pop_wait(&self, timeout: Duration) -> Option<Inbound> {
        let mut inbox = self.inbox.lock().expect("inspector inbox poisoned");
        if inbox.is_empty() && !self.is_closed() {
            inbox = self
                .wake
                .wait_timeout(inbox, timeout)
                .expect("inspector inbox poisoned")
                .0;
        }
        inbox.pop_front()
    }
}

fn targets() -> Vec<Arc<Target>> {
    registry()
        .lock()
        .expect("inspector registry poisoned")
        .values()
        .cloned()
        .collect()
}

fn lookup(id: &str) -> Option<Arc<Target>> {
    registry()
        .lock()
        .expect("inspector registry poisoned")
        .get(id)
        .cloned()
}

/// Bind the configured address and serve until the process exits. Returns the
/// bound address, because `--inspector-addr 127.0.0.1:0` picks a port.
pub async fn start_if_configured() -> Result<Option<SocketAddr>> {
    let Some(config) = CONFIG.get() else {
        return Ok(None);
    };
    let listener = tokio::net::TcpListener::bind(config.addr)
        .await
        .with_context(|| format!("bind the inspector listener {}", config.addr))?;
    let bound = listener.local_addr()?;
    HOST_WORKERS.store(
        tokio::runtime::Handle::current().metrics().num_workers(),
        Ordering::Relaxed,
    );
    tracing::warn!(
        event = "inspector_listening",
        address = %bound,
        "V8 inspector listening on {bound}: connections run arbitrary code in every cell, and \
         there is no authentication"
    );
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            tokio::spawn(async move {
                let io = hyper_util::rt::TokioIo::new(stream);
                let service = hyper::service::service_fn(handle);
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .with_upgrades()
                    .await;
            });
        }
    });
    Ok(Some(bound))
}

/// Node's rule: the `Host` header must be an IP literal or `localhost`. A
/// browser tricked into resolving an attacker's name to this address sends
/// that name, and is refused.
fn host_allowed(host: &str) -> bool {
    let host = host.trim();
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        match host.rsplit_once(':') {
            Some((name, port)) if port.chars().all(|c| c.is_ascii_digit()) => name,
            _ => host,
        }
    };
    name.eq_ignore_ascii_case("localhost") || name.parse::<IpAddr>().is_ok()
}

fn text_response(status: StatusCode, content_type: &str, body: String) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, content_type)
        .header(hyper::header::CACHE_CONTROL, "no-cache")
        .body(Full::new(Bytes::from(body)))
        .expect("static response is valid")
}

fn json_response(value: serde_json::Value) -> Response<Full<Bytes>> {
    text_response(
        StatusCode::OK,
        "application/json; charset=UTF-8",
        serde_json::to_string_pretty(&value).expect("JSON serializes"),
    )
}

fn list_json(host: &str) -> serde_json::Value {
    serde_json::Value::Array(
        targets()
            .into_iter()
            .map(|target| {
                serde_json::json!({
                    "description": target.description(),
                    "devtoolsFrontendUrl": format!(
                        "devtools://devtools/bundled/js_app.html?experiments=true&v8only=true&ws={host}/{}",
                        target.id()
                    ),
                    "faviconUrl": "",
                    "id": target.id(),
                    "title": target.title(),
                    "type": "node",
                    "url": format!("file:///{}", target.script()),
                    "webSocketDebuggerUrl": format!("ws://{host}/{}", target.id()),
                })
            })
            .collect(),
    )
}

async fn handle(request: Request<Incoming>) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    Ok(route(request))
}

fn route(mut request: Request<Incoming>) -> Response<Full<Bytes>> {
    let host = request
        .headers()
        .get(hyper::header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let Some(host) = host.filter(|host| host_allowed(host)) else {
        return text_response(
            StatusCode::FORBIDDEN,
            "text/plain",
            "the inspector accepts only an IP address or localhost as Host".to_string(),
        );
    };
    if request.method() != Method::GET {
        return text_response(
            StatusCode::METHOD_NOT_ALLOWED,
            "text/plain",
            "GET only".to_string(),
        );
    }
    let path = request.uri().path().trim_end_matches('/').to_string();
    let path = percent_encoding::percent_decode_str(&path)
        .decode_utf8_lossy()
        .into_owned();
    match path.as_str() {
        "/json/version" => json_response(serde_json::json!({
            "Browser": format!("celld/{}", env!("CARGO_PKG_VERSION")),
            "Protocol-Version": "1.3",
            "V8-Version": v8::V8::get_version(),
        })),
        "/json" | "/json/list" => json_response(list_json(&host)),
        other => {
            let Some(target) = other.strip_prefix('/').and_then(lookup) else {
                return text_response(
                    StatusCode::NOT_FOUND,
                    "text/plain",
                    "no such inspector target; GET /json/list names them".to_string(),
                );
            };
            if !fastwebsockets::upgrade::is_upgrade_request(&request) {
                return text_response(
                    StatusCode::BAD_REQUEST,
                    "text/plain",
                    "connect to a target with a WebSocket upgrade".to_string(),
                );
            }
            let (response, upgrade) = match fastwebsockets::upgrade::upgrade(&mut request) {
                Ok(upgraded) => upgraded,
                Err(error) => {
                    return text_response(
                        StatusCode::BAD_REQUEST,
                        "text/plain",
                        format!("WebSocket upgrade failed: {error}"),
                    )
                }
            };
            tokio::spawn(async move {
                if let Ok(socket) = upgrade.await {
                    run_session(target, socket).await;
                }
            });
            let (parts, _) = response.into_parts();
            Response::from_parts(parts, Full::new(Bytes::new()))
        }
    }
}

/// Carry one debugger connection: client frames become [`Inbound`] messages,
/// and everything the isolate's inspector sends comes back as text frames.
async fn run_session<S>(target: Arc<Target>, mut socket: fastwebsockets::WebSocket<S>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use fastwebsockets::{FragmentCollectorRead, Frame, OpCode, Payload};

    socket.set_auto_close(false);
    // A heap snapshot chunk is far smaller than this; a client message that
    // is not must not exhaust memory.
    socket.set_max_message_size(64 * 1024 * 1024);
    let (reader, writer) = socket.split(tokio::io::split);
    let writer = Arc::new(tokio::sync::Mutex::new(writer));
    let mut reader = FragmentCollectorRead::new(reader);

    let session = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
    let (out, mut outputs) = mpsc::unbounded_channel::<String>();
    target.push(Inbound::Connect { session, out });

    let output_writer = writer.clone();
    let output_task = tokio::spawn(async move {
        while let Some(text) = outputs.recv().await {
            let frame = Frame::text(Payload::Owned(text.into_bytes()));
            if output_writer.lock().await.write_frame(frame).await.is_err() {
                return;
            }
        }
        // The isolate dropped the session: it is gone or was retired.
        let _ = output_writer
            .lock()
            .await
            .write_frame(Frame::close(1001, b"inspector target closed"))
            .await;
    });

    let obligated_writer = writer.clone();
    let mut obligated = move |frame: Frame<'_>| {
        let writer = obligated_writer.clone();
        let reply = Frame::new(
            frame.fin,
            frame.opcode,
            None,
            Payload::Owned(frame.payload.to_vec()),
        );
        async move { writer.lock().await.write_frame(reply).await }
    };
    let mut peer_closed = false;
    while let Ok(frame) = reader.read_frame(&mut obligated).await {
        match frame.opcode {
            OpCode::Text => target.push(Inbound::Message {
                session,
                text: String::from_utf8_lossy(&frame.payload).into_owned(),
            }),
            OpCode::Close => {
                peer_closed = true;
                break;
            }
            _ => {}
        }
    }
    target.push(Inbound::Disconnect { session });
    if peer_closed {
        let _ = writer
            .lock()
            .await
            .write_frame(Frame::close(1000, b""))
            .await;
    }
    output_task.abort();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_is_the_default_boundary() {
        assert!(Config::parse("127.0.0.1:9229", false).is_ok());
        assert!(Config::parse("[::1]:9229", false).is_ok());
        assert!(Config::parse("localhost:9229", false).is_ok());
        assert!(Config::parse("0.0.0.0:9229", false).is_err());
        assert!(Config::parse("10.0.0.5:9229", false).is_err());
        assert!(Config::parse("0.0.0.0:9229", true).is_ok());
        assert!(Config::parse("example.com:9229", true).is_err());
        assert!(Config::parse("9229", false).is_err());
    }

    #[test]
    fn host_header_must_be_an_ip_or_localhost() {
        assert!(host_allowed("127.0.0.1:9229"));
        assert!(host_allowed("localhost"));
        assert!(host_allowed("[::1]:9229"));
        assert!(!host_allowed("evil.example:9229"));
        assert!(!host_allowed("127.0.0.1.evil.example"));
    }

    #[test]
    fn target_ids_are_path_safe_and_unique() {
        let a = Target::new(TargetKind::Pool, "my app/worker@1", &[]);
        let b = Target::new(TargetKind::Pool, "my app/worker@1", &[]);
        assert_ne!(a.id(), b.id());
        assert!(a
            .id()
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '_' | '.')));
    }

    #[test]
    fn title_names_the_cells_that_share_the_isolate() {
        let target = Target::new(TargetKind::Pool, "app", &["Counter".to_string()]);
        assert!(target.title().contains("stateless"));
        target.cell_adopted("Counter:a", true);
        target.cell_adopted("Counter:b", true);
        let title = target.title();
        assert!(title.contains("shared by 2 cells"), "{title}");
        assert!(title.contains("Counter:a"), "{title}");
        target.cell_adopted("Counter:a", false);
        assert!(target.title().contains("shared by 1 cell:"));
    }
}
