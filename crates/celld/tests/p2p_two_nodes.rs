// Copyright 2026 Deno Land Inc. Apache-2.0 license.

//! Two `celld dev` nodes on one machine talking over Iroh.
//!
//! Each node runs `examples/p2p` with the relay and discovery off, bound to a
//! fixed loopback UDP port, so the test needs no network and dials by direct
//! address. It checks both directions, the authenticated node id the accepting
//! Durable Object sees, close propagation each way, `closed` settling on a
//! clean end and on a reset, and a restart of one node followed by a
//! reconnect: once killed without a goodbye, once stopped with SIGTERM.
//!
//! A second test checks that a stream belongs to its Durable Object and not to
//! the event that opened it: an alarm dials, later requests keep using the
//! stream, the object stays in memory while the stream is open, the per-cell
//! budget refuses another one, every way a stream ends releases the object,
//! and a forced redeploy resets the stream with the cell-ended code.
//!
//! The deploy step bundles with `esbuild`. Set `CELLD_ESBUILD`, or put
//! `esbuild` on `PATH`; without one the test says so and returns. Set
//! `CELLD_REQUIRE_P2P_E2E=1` to make a missing esbuild a failure.
#![cfg(unix)]
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn find_esbuild() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CELLD_ESBUILD") {
        return Some(PathBuf::from(path));
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join("esbuild"))
            .find(|candidate| candidate.is_file())
    })
}

fn free_tcp_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn free_udp_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn copy_example(destination: &Path) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("p2p");
    std::fs::create_dir_all(destination).unwrap();
    for file in ["index.js", "wrangler.jsonc"] {
        std::fs::copy(source.join(file), destination.join(file)).unwrap();
    }
}

struct Node {
    child: Option<Child>,
    project: PathBuf,
    http_port: u16,
    udp_port: u16,
    log: PathBuf,
    esbuild: PathBuf,
    env: Vec<(&'static str, &'static str)>,
}

impl Node {
    fn new(root: &Path, name: &str, esbuild: &Path) -> Self {
        let project = root.join(name);
        copy_example(&project);
        Self {
            child: None,
            project,
            http_port: free_tcp_port(),
            udp_port: free_udp_port(),
            log: root.join(format!("{name}.log")),
            esbuild: esbuild.to_path_buf(),
            env: Vec::new(),
        }
    }

    fn with_env(mut self, env: &[(&'static str, &'static str)]) -> Self {
        self.env = env.to_vec();
        self
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.http_port)
    }

    fn direct_addr(&self) -> String {
        format!("127.0.0.1:{}", self.udp_port)
    }

    fn start(&mut self) {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)
            .unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_celld"))
            .args(["dev"])
            .arg(&self.project)
            .args(["--port", &self.http_port.to_string(), "--logs"])
            .env("CELLD_ESBUILD", &self.esbuild)
            .env("CELLD_P2P_RELAY", "disabled")
            .env("CELLD_P2P_DISCOVERY", "none")
            .env("CELLD_P2P_BIND", self.direct_addr())
            .envs(self.env.iter().copied())
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .expect("start celld dev");
        self.child = Some(child);
    }

    /// SIGKILL the node: no goodbye to its peers.
    fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// SIGTERM the node and wait for it to exit: it closes its endpoint, so
    /// its peers see an orderly close.
    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // SAFETY: a plain signal to a child this test spawned.
            unsafe {
                libc::kill(child.id() as i32, libc::SIGTERM);
            }
            let deadline = Instant::now() + Duration::from_secs(40);
            while Instant::now() < deadline {
                if child.try_wait().unwrap().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn log_text(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// The operator listener the node announced in its startup output.
    fn internal_origin(&self) -> String {
        let log = self.log_text();
        let line = log
            .lines()
            .rev()
            .find(|line| line.starts_with("celld internal listening on "))
            .expect("the node announced its internal listener");
        let address = line
            .strip_prefix("celld internal listening on ")
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        format!("http://{address}")
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        self.kill();
    }
}

async fn get_json(client: &reqwest::Client, url: &str) -> Result<Value, String> {
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    let status = response.status();
    let body = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("{url}: {status}: {body}"));
    }
    serde_json::from_str(&body).map_err(|e| format!("{url}: {e}: {body}"))
}

async fn wait_ready(client: &reqwest::Client, node: &Node) -> String {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        match get_json(client, &format!("{}/id", node.base())).await {
            Ok(value) => return value["nodeId"].as_str().unwrap().to_string(),
            Err(error) => {
                assert!(
                    Instant::now() < deadline,
                    "node never became ready: {error}\n--- log ---\n{}",
                    node.log_text()
                );
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        }
    }
}

fn dial_url(from: &Node, to: &Node, to_id: &str, msgs: &str, finish: &str) -> String {
    format!(
        "{}/dial?peer={to_id}&addr={}&msgs={msgs}&finish={finish}",
        from.base(),
        to.direct_addr()
    )
}

/// Poll `/seen` until it holds at least `count` sessions.
async fn seen(client: &reqwest::Client, node: &Node, count: usize) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let value = get_json(client, &format!("{}/seen", node.base()))
            .await
            .unwrap();
        let sessions = value.as_array().unwrap().clone();
        if sessions.len() >= count || Instant::now() > deadline {
            return sessions;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_nodes_exchange_streams_and_survive_a_restart() {
    let Some(esbuild) = find_esbuild() else {
        if std::env::var_os("CELLD_REQUIRE_P2P_E2E").is_some() {
            panic!("CELLD_REQUIRE_P2P_E2E is set but no esbuild was found");
        }
        eprintln!("skipping: set CELLD_ESBUILD or put esbuild on PATH");
        return;
    };
    let root = tempfile::tempdir().unwrap();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .build()
        .unwrap();
    let mut a = Node::new(root.path(), "a", &esbuild);
    let mut b = Node::new(root.path(), "b", &esbuild);
    a.start();
    b.start();
    let a_id = wait_ready(&client, &a).await;
    let b_id = wait_ready(&client, &b).await;
    assert_eq!(a_id.len(), 64);
    assert_ne!(a_id, b_id);

    // The node id is in the startup output, where an operator reads it.
    assert!(
        a.log_text().contains(&format!("celld p2p node {a_id}")),
        "node id missing from startup output:\n{}",
        a.log_text()
    );

    // A dials B: three messages out, three replies back, then A closes.
    let result = get_json(&client, &dial_url(&a, &b, &b_id, "one,two,three", "close"))
        .await
        .unwrap();
    assert_eq!(
        result["replies"],
        serde_json::json!(["echo:one", "echo:two", "echo:three"]),
        "{result}"
    );
    assert_eq!(result["remoteNodeId"], b_id);
    assert_eq!(result["end"], "closed");
    // A close the caller asked for is not an error.
    assert_eq!(result["closed"], "resolved", "{result}");

    // B's accepting Durable Object saw A's authenticated id, and learned of
    // A's close as end-of-stream.
    let sessions = seen(&client, &b, 1).await;
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    assert_eq!(sessions[0]["remote"], a_id);
    assert_eq!(
        sessions[0]["messages"],
        serde_json::json!(["one", "two", "three"])
    );
    assert_eq!(sessions[0]["end"], "eof");

    // The other direction: B dials A, and A's handler sees B's id.
    let result = get_json(&client, &dial_url(&b, &a, &a_id, "ping", "close"))
        .await
        .unwrap();
    assert_eq!(
        result["replies"],
        serde_json::json!(["echo:ping"]),
        "{result}"
    );
    let sessions = seen(&client, &a, 1).await;
    assert_eq!(sessions[0]["remote"], b_id);

    // The accepting side finishes the stream: A reads its reply, then EOF.
    let result = get_json(&client, &dial_url(&a, &b, &b_id, "x", "done"))
        .await
        .unwrap();
    assert_eq!(
        result["replies"],
        serde_json::json!(["echo:x", "done:ok"]),
        "{result}"
    );
    // Both directions finished cleanly (the reply and end-of-stream in, our
    // finish out): `closed` resolves without anyone calling `close()`.
    assert_eq!(result["closed"], "resolved", "{result}");
    let sessions = seen(&client, &b, 2).await;
    assert_eq!(sessions[1]["end"], "finished");

    // The accepting side resets the stream: A's next read fails with the code.
    let result = get_json(&client, &dial_url(&a, &b, &b_id, "y", "bye"))
        .await
        .unwrap();
    let end = result["end"].as_str().unwrap();
    assert!(
        end.contains("stream reset by peer (code 300)"),
        "expected a reset with code 300, got {result}"
    );
    // A reset rejects `closed` with the same reason.
    assert!(
        result["closed"]
            .as_str()
            .unwrap()
            .starts_with("rejected: stream reset by peer (code 300)"),
        "{result}"
    );
    let sessions = seen(&client, &b, 3).await;
    assert_eq!(sessions[2]["end"], "closed-by-handler");

    // The operator listener reports the endpoint: node id, accepted ALPNs.
    let status = get_json(&client, &format!("{}/p2p", a.internal_origin()))
        .await
        .unwrap();
    assert_eq!(status["node_id"], a_id);
    assert_eq!(status["relay"], "disabled");
    assert_eq!(
        status["accepts"],
        serde_json::json!(["celld-example/echo/1"])
    );

    // Restart B without a goodbye (SIGKILL). Its key file keeps its id; its DO
    // keeps its durable record of earlier streams; A reconnects to the same id.
    b.kill();
    b.start();
    assert_eq!(
        wait_ready(&client, &b).await,
        b_id,
        "the node id must survive a restart"
    );
    let attempts = reconnect(&client, &a, &b, &b_id, "again").await;
    eprintln!("after SIGKILL of B, A reconnected on attempt {attempts}");
    let sessions = seen(&client, &b, 4).await;
    assert_eq!(
        sessions.len(),
        4,
        "earlier sessions must survive: {sessions:?}"
    );
    assert_eq!(sessions[3]["remote"], a_id);
    assert_eq!(sessions[3]["messages"], serde_json::json!(["again"]));

    // Restart B with a goodbye (SIGTERM): it closes its endpoint, and A, which
    // pooled a connection to the previous process, redials.
    b.stop();
    b.start();
    assert_eq!(wait_ready(&client, &b).await, b_id);
    let attempts = reconnect(&client, &a, &b, &b_id, "once more").await;
    eprintln!("after SIGTERM of B, A reconnected on attempt {attempts}");
    let sessions = seen(&client, &b, 5).await;
    assert_eq!(sessions.len(), 5, "{sessions:?}");
    assert_eq!(sessions[4]["messages"], serde_json::json!(["once-more"]));
}

/// Dial `to` from `from` until a round trip succeeds, and say how many
/// attempts it took. The first attempt after a peer vanished may fail on the
/// connection the dialer had pooled; the next dials afresh.
async fn reconnect(
    client: &reqwest::Client,
    from: &Node,
    to: &Node,
    to_id: &str,
    message: &str,
) -> usize {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut attempts = 0;
    loop {
        attempts += 1;
        let msgs = message.replace(' ', "-");
        let result = get_json(client, &dial_url(from, to, to_id, &msgs, "close"))
            .await
            .unwrap();
        if result["replies"] == serde_json::json!([format!("echo:{msgs}")]) {
            return attempts;
        }
        eprintln!("attempt {attempts} did not round-trip: {result}");
        assert!(
            Instant::now() < deadline,
            "never reconnected: {result}\n--- from ---\n{}\n--- to ---\n{}",
            from.log_text(),
            to.log_text()
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

// ---------------------------------------------------------------------------
// Streams owned by the cell
// ---------------------------------------------------------------------------

/// The environment of the nodes in the cell-ownership test: an idle object
/// leaves memory after two seconds, a cell may hold three outbound
/// transports, and a redeploy forces objects onto the new generation at once.
const OWNERSHIP_ENV: &[(&str, &str)] = &[
    ("CELLD_IDLE_EVICT_S", "2"),
    ("CELLD_MAX_OUTBOUND_WEBSOCKETS", "3"),
    ("CELLD_DEPLOY_MAX_AGE_S", "0"),
];

/// The scope of the node's only Durable Object, from the operator listener.
async fn cell_scope(client: &reqwest::Client, node: &Node) -> String {
    let state = get_json(client, &format!("{}/state", node.internal_origin()))
        .await
        .unwrap();
    state["deployment"]["cells"]
        .as_object()
        .and_then(|cells| cells.keys().find(|key| key.starts_with("Node:")))
        .unwrap_or_else(|| panic!("the node has no resident Node cell: {state}"))
        .clone()
}

async fn evict_status(client: &reqwest::Client, node: &Node, scope: &str) -> u16 {
    client
        .post(format!("{}/evict/{scope}", node.internal_origin()))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

/// Streams the node's endpoint holds, from the operator listener.
async fn open_streams(client: &reqwest::Client, node: &Node) -> u64 {
    get_json(client, &format!("{}/p2p", node.internal_origin()))
        .await
        .unwrap()["streams"]
        .as_u64()
        .unwrap()
}

/// Poll until the node holds `want` streams: a stream's slot is freed on the
/// node's own tasks, a moment after the event that ended it.
async fn wait_streams(client: &reqwest::Client, node: &Node, want: u64, why: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let have = open_streams(client, node).await;
        if have == want {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{why}: the node holds {have} streams, expected {want}\n{}",
            node.log_text()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Arm `from`'s alarm to dial `to`, and wait until it has dialed.
async fn arm_and_wait(client: &reqwest::Client, from: &Node, to: &Node, to_id: &str) -> Value {
    get_json(
        client,
        &format!(
            "{}/arm?peer={to_id}&addr={}&delay=200",
            from.base(),
            to.direct_addr()
        ),
    )
    .await
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let session = get_json(client, &format!("{}/session", from.base()))
            .await
            .unwrap();
        if !session["dialed"].is_null() {
            return session;
        }
        assert!(
            Instant::now() < deadline,
            "the alarm never dialed: {session}\n{}",
            from.log_text()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Poll `/seen` on `node` until an alarm-opened session (one whose first line
/// is the alarm's hello) has ended as `end` says, `count` times over. Sessions
/// of other streams (the extras the budget step opens) are in the list too, so
/// they are matched, not counted by position.
async fn seen_alarm_session(
    client: &reqwest::Client,
    node: &Node,
    end: &str,
    count: usize,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let sessions = seen(client, node, 0).await;
        let found: Vec<&Value> = sessions
            .iter()
            .filter(|session| {
                session["messages"][0] == "hello from the alarm"
                    && session["end"].as_str().is_some_and(|got| got.contains(end))
            })
            .collect();
        if found.len() >= count {
            return found[count - 1].clone();
        }
        assert!(
            Instant::now() < deadline,
            "no alarm session ended with {end:?}: {sessions:?}\n{}",
            node.log_text()
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_belongs_to_its_cell_not_to_the_event_that_opened_it() {
    let Some(esbuild) = find_esbuild() else {
        if std::env::var_os("CELLD_REQUIRE_P2P_E2E").is_some() {
            panic!("CELLD_REQUIRE_P2P_E2E is set but no esbuild was found");
        }
        eprintln!("skipping: set CELLD_ESBUILD or put esbuild on PATH");
        return;
    };
    let root = tempfile::tempdir().unwrap();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .build()
        .unwrap();
    let mut a = Node::new(root.path(), "a", &esbuild).with_env(OWNERSHIP_ENV);
    let mut b = Node::new(root.path(), "b", &esbuild).with_env(OWNERSHIP_ENV);
    a.start();
    b.start();
    wait_ready(&client, &a).await;
    let b_id = wait_ready(&client, &b).await;

    // An alarm dials. The event that opened the stream is long over by the
    // time the next request arrives, and the stream is still open.
    let session = arm_and_wait(&client, &a, &b, &b_id).await;
    assert_eq!(
        session["dialed"]["reply"], "echo:hello from the alarm",
        "{session}"
    );
    assert_eq!(session["open"], true, "{session}");
    assert_eq!(session["closed"], "pending", "{session}");
    let born = session["born"].clone();

    // It is a transport the core holds for the cell, like an outbound
    // WebSocket, so it keeps the object in memory: an operator eviction is
    // refused, and idle eviction (two seconds here) does not take it. Nothing
    // observable happens while we wait for the latter, so the wait is a fixed,
    // generous window.
    let a_scope = cell_scope(&client, &a).await;
    assert_eq!(evict_status(&client, &a, &a_scope).await, 409);
    tokio::time::sleep(Duration::from_secs(6)).await;
    let session = get_json(&client, &format!("{}/session", a.base()))
        .await
        .unwrap();
    assert_eq!(session["born"], born, "the object was evicted: {session}");
    assert_eq!(session["open"], true, "{session}");

    // Later requests, each its own event, use the same stream, both ways.
    for message in ["ping", "pong"] {
        let reply = get_json(&client, &format!("{}/send?msg={message}", a.base()))
            .await
            .unwrap();
        assert_eq!(reply["reply"], format!("echo:{message}"), "{reply}");
    }

    // An inbound stream outlives its handler too: B's handler returns after
    // `keep:`, and requests to B read from and write to the stream.
    get_json(&client, &format!("{}/send?msg=keep:one&reply=0", a.base()))
        .await
        .unwrap();
    let kept = seen_alarm_session(&client, &b, "kept", 1).await;
    assert_eq!(
        kept["messages"],
        serde_json::json!(["hello from the alarm", "ping", "pong", "keep:one"])
    );
    get_json(&client, &format!("{}/send?msg=from-a&reply=0", a.base()))
        .await
        .unwrap();
    let pulled = get_json(&client, &format!("{}/pull?name=one", b.base()))
        .await
        .unwrap();
    assert_eq!(pulled["line"], "from-a", "{pulled}");
    get_json(&client, &format!("{}/push?name=one&msg=from-b", b.base()))
        .await
        .unwrap();
    let received = get_json(&client, &format!("{}/recv", a.base()))
        .await
        .unwrap();
    assert_eq!(received["line"], "pushed:from-b", "{received}");

    // The budget: this node lets a cell hold three transports. The session is
    // one; two more open, and the next dial is refused as a fourth outbound
    // WebSocket would be. Closing them gives the budget back.
    let open_url = format!("{}/open?peer={b_id}&addr={}", a.base(), b.direct_addr());
    let opened = get_json(&client, &format!("{open_url}&n=3")).await.unwrap();
    let outcomes = opened["outcomes"].as_array().unwrap();
    assert_eq!(outcomes[0], "open", "{opened}");
    assert_eq!(outcomes[1], "open", "{opened}");
    assert!(
        outcomes[2].as_str().unwrap().starts_with("refused: "),
        "the third extra stream must be refused: {opened}"
    );
    wait_streams(&client, &a, 3, "after the refused dial").await;
    let closed = get_json(&client, &format!("{}/open?close=1", a.base()))
        .await
        .unwrap();
    assert_eq!(closed["closed"], 2, "{closed}");
    wait_streams(&client, &a, 1, "after the extras were closed").await;
    let again = get_json(&client, &format!("{open_url}&n=1")).await.unwrap();
    assert_eq!(again["outcomes"], serde_json::json!(["open"]), "{again}");
    get_json(&client, &format!("{}/open?close=1", a.base()))
        .await
        .unwrap();

    // A closes its stream; B drops the kept one. No stream is left, and the
    // object is free to leave: an operator eviction now succeeds.
    let hangup = get_json(&client, &format!("{}/hangup?how=close", a.base()))
        .await
        .unwrap();
    assert_eq!(hangup["closed"], "resolved", "{hangup}");
    get_json(&client, &format!("{}/drop?name=one", b.base()))
        .await
        .unwrap();
    wait_streams(&client, &a, 0, "A closed its stream").await;
    wait_streams(&client, &b, 0, "B dropped its stream").await;
    assert_eq!(evict_status(&client, &a, &a_scope).await, 200);
    let after = get_json(&client, &format!("{}/id", a.base()))
        .await
        .unwrap();
    assert_ne!(after["born"], born, "the object was not rebuilt");

    // A reset by A ends B's handler with the code, and frees B's stream.
    arm_and_wait(&client, &a, &b, &b_id).await;
    let hangup = get_json(&client, &format!("{}/hangup?how=reset", a.base()))
        .await
        .unwrap();
    assert_eq!(hangup["closed"], "resolved", "{hangup}");
    seen_alarm_session(&client, &b, "stream reset by peer (code 301)", 1).await;
    wait_streams(&client, &b, 0, "after A reset the stream").await;
    wait_streams(&client, &a, 0, "after A reset the stream").await;

    // A finishes its side only: B's handler reads end-of-stream and closes its
    // own. A reads end-of-stream in turn, both directions are done, and
    // `closed` resolves with no one calling close().
    arm_and_wait(&client, &a, &b, &b_id).await;
    get_json(&client, &format!("{}/hangup?how=finish", a.base()))
        .await
        .unwrap();
    seen_alarm_session(&client, &b, "eof", 1).await;
    let received = get_json(&client, &format!("{}/recv", a.base()))
        .await
        .unwrap();
    assert!(received["line"].is_null(), "{received}");
    let session = get_json(&client, &format!("{}/session", a.base()))
        .await
        .unwrap();
    assert_eq!(session["closed"], "resolved", "{session}");
    wait_streams(&client, &a, 0, "after both directions finished").await;
    wait_streams(&client, &b, 0, "after both directions finished").await;

    // The object being reset ends its streams: `ctx.abort()` replaces the
    // instance that held them, and B's handler sees the cell-ended code. (An
    // eviction, a handoff and a swap end them the same way, through the core;
    // `celld dev` restarts the process on a redeploy, so none of those is
    // observable one stream at a time here.)
    arm_and_wait(&client, &a, &b, &b_id).await;
    let _ = client.get(format!("{}/abort", a.base())).send().await;
    seen_alarm_session(&client, &b, "stream reset by peer (code 5)", 1).await;
    wait_streams(&client, &a, 0, "after the object was reset").await;
    wait_streams(&client, &b, 0, "after the object was reset").await;

    // A node that stops ends its cells' streams: the drain closes what pins
    // the cell, B's handler sees the same cell-ended code, and B's own cell is
    // released.
    arm_and_wait(&client, &a, &b, &b_id).await;
    a.stop();
    let ended = seen_alarm_session(&client, &b, "stream reset by peer (code 5)", 2).await;
    eprintln!("B saw the session end as {}", ended["end"]);
    wait_streams(&client, &b, 0, "after A stopped").await;
}
