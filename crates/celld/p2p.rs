// Copyright 2026 Deno Land Inc. Apache-2.0 license.

// This module is a host adapter: it owns a UDP endpoint, a key file and the
// tasks that serve them, none of which belong to the simulated execution
// domain. The cell side of the feature lives in `js/p2p.rs`.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

//! The node's Iroh endpoint and the host half of the `p2p` binding.
//!
//! One endpoint per node, identified by an Ed25519 key that lives in a file
//! and so survives restarts. Cells reach it through `env.<P2P binding>`:
//!
//! * **Outbound.** `connect(nodeId)` dials the peer (or reuses the node's QUIC
//!   connection to it for that ALPN) and opens one bidirectional stream.
//! * **Inbound.** A binding that declares `accept` names a Durable Object. The
//!   endpoint accepts that binding's ALPN, and each bidirectional stream the
//!   peer opens becomes one event in the DO: the host registers the stream,
//!   mints a one-time claim token, and sends the cell an internal `fetch`
//!   whose URL carries the token. The harness redeems the token for the stream
//!   and calls `onp2pstream(stream)`. The token is unguessable and is checked
//!   against the cell it was minted for, so a request that merely imitates the
//!   URL reaches the application's own `fetch` unchanged.
//!
//! A stream opened or claimed by a Durable Object belongs to the cell, not to
//! the event that happened to open it. It lives until both directions have
//! finished, a side closes or resets it, its connection is lost, or the cell
//! leaves this node (eviction, handoff, reset, restore, a stopped facet): those
//! lifecycle events close it with [`CODE_CELL_ENDED`]. While it is open it
//! pins its cell resident exactly as an outbound WebSocket does: [`adopt`]
//! registers it with the core as a `WebSocketKind::Outbound` transport, so the
//! core's own accounting (`is_active`, the per-cell cap and the node's pin
//! budget) applies and refuses at the budget. A stream opened by a plain Worker
//! has no cell and stays with its request, like a `connect()` TCP socket.
//!
//! The node identity is the node's, not the cell's. Only the node that owns
//! the accepting cell serves its inbound streams; a stream that arrives at any
//! other node is reset with [`CODE_NOT_OWNER`]. See `docs/services/p2p.md`.

use crate::actor::AppHandle;
use anyhow::{anyhow, bail, ensure, Context as _};
use iroh::endpoint::{presets, Connection, RecvStream, SendStream, VarInt};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl, SecretKey};
use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// The stream was reset because no deployment on this node accepts the ALPN,
/// or the accepting class does not implement `onp2pstream`.
pub const CODE_NO_HANDLER: u32 = 1;
/// The stream was reset because the accepting cell is owned by another node.
pub const CODE_NOT_OWNER: u32 = 2;
/// The stream was reset because the accepting handler threw or the cell could
/// not run it.
pub const CODE_HANDLER_FAILED: u32 = 3;
/// The stream was reset because the node is at its stream limit, or the core
/// would not pin its cell for another transport (the per-cell cap or the
/// node's pin budget).
pub const CODE_BUSY: u32 = 4;
/// The stream was reset because its cell left this node or was reset: an
/// eviction, a handoff, a generation swap, a restore, a failed durability
/// proof, or a stopped facet.
pub const CODE_CELL_ENDED: u32 = 5;

/// The path of the internal request that delivers an inbound stream. The final
/// segment is the claim token.
pub const ACCEPT_URL_PREFIX: &str = "https://p2p.celld.invalid/__celld/p2p/accept/";

/// Set on every stream id so the retirement path can tell it from a TCP
/// socket id without a lookup. TCP ids count up from one and never reach it.
/// It is bit 52, not higher: ids cross into JavaScript as numbers, which hold
/// integers exactly only up to 2^53.
const STREAM_ID_BIT: u64 = 1 << 52;

const DEFAULT_MAX_STREAMS: usize = 4096;
const DIAL_TIMEOUT: Duration = Duration::from_secs(30);
const CLAIM_TTL: Duration = Duration::from_secs(60);
const READ_CHUNK: usize = 64 * 1024;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Which relay servers the endpoint uses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RelaySetting {
    /// n0's public production relays.
    #[default]
    Default,
    /// No relay: direct UDP only. Peers must be dialed by address.
    Disabled,
    /// These relays only.
    Custom(Vec<String>),
}

/// Which address-lookup services resolve a bare node id to addresses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Discovery {
    /// n0's DNS and pkarr services (`iroh.link`): publishes this node's home
    /// relay and resolves peers'.
    pub dns: bool,
    /// Multicast DNS on the local network.
    pub mdns: bool,
}

/// Node-level settings, from flags and `CELLD_P2P_*` variables.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    /// `--p2p-key-file` / `CELLD_P2P_KEY_FILE`: 32 raw bytes, created mode 0600
    /// when missing. Without one the identity is generated per process.
    pub key_file: Option<PathBuf>,
    /// `--p2p-relay` / `CELLD_P2P_RELAY`.
    pub relay: RelaySetting,
    /// `--p2p-discovery` / `CELLD_P2P_DISCOVERY`. `None` picks the default:
    /// n0 DNS with the default relays, nothing otherwise.
    pub discovery: Option<Discovery>,
    /// `--p2p-bind` / `CELLD_P2P_BIND`: the one local UDP address to bind
    /// instead of every interface on a random port.
    pub bind: Option<SocketAddr>,
}

impl Settings {
    /// The settings the environment alone selects.
    pub fn from_environment() -> anyhow::Result<Self> {
        let env = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let mut settings = Self::default();
        if let Some(value) = env("CELLD_P2P_KEY_FILE") {
            settings.key_file = Some(PathBuf::from(value));
        }
        if let Some(value) = env("CELLD_P2P_RELAY") {
            settings.set_relay(&value)?;
        }
        if let Some(value) = env("CELLD_P2P_DISCOVERY") {
            settings.set_discovery(&value)?;
        }
        if let Some(value) = env("CELLD_P2P_BIND") {
            settings.set_bind(&value)?;
        }
        Ok(settings)
    }

    /// `default`, `disabled`, or a comma-separated list of relay URLs.
    pub fn set_relay(&mut self, value: &str) -> anyhow::Result<()> {
        self.relay = match value.trim() {
            "default" => RelaySetting::Default,
            "disabled" | "none" | "off" => RelaySetting::Disabled,
            list => {
                let urls: Vec<String> = list
                    .split(',')
                    .map(str::trim)
                    .filter(|url| !url.is_empty())
                    .map(str::to_string)
                    .collect();
                ensure!(!urls.is_empty(), "--p2p-relay needs a value");
                for url in &urls {
                    RelayUrl::from_str(url)
                        .with_context(|| format!("--p2p-relay: invalid relay URL {url:?}"))?;
                }
                RelaySetting::Custom(urls)
            }
        };
        Ok(())
    }

    /// A comma-separated list of `dns`, `mdns`, or the single value `none`.
    pub fn set_discovery(&mut self, value: &str) -> anyhow::Result<()> {
        let mut discovery = Discovery::default();
        for part in value.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match part {
                "dns" | "n0" => discovery.dns = true,
                "mdns" => discovery.mdns = true,
                "none" => {}
                other => bail!("--p2p-discovery: unknown service {other:?} (dns, mdns, none)"),
            }
        }
        self.discovery = Some(discovery);
        Ok(())
    }

    pub fn set_bind(&mut self, value: &str) -> anyhow::Result<()> {
        self.bind = Some(
            value
                .trim()
                .parse()
                .with_context(|| format!("--p2p-bind: {value:?} is not an IP:PORT address"))?,
        );
        Ok(())
    }

    fn discovery(&self) -> Discovery {
        self.discovery.unwrap_or(Discovery {
            dns: self.relay == RelaySetting::Default,
            mdns: false,
        })
    }

    fn relay_mode(&self) -> anyhow::Result<RelayMode> {
        Ok(match &self.relay {
            RelaySetting::Default => iroh::endpoint::default_relay_mode(),
            RelaySetting::Disabled => RelayMode::Disabled,
            RelaySetting::Custom(urls) => RelayMode::custom(
                urls.iter()
                    .map(|url| RelayUrl::from_str(url).map_err(|e| anyhow!("{url}: {e}")))
                    .collect::<anyhow::Result<Vec<_>>>()?,
            ),
        })
    }
}

/// The help text for the node-level flags, so `--help` documents them where
/// the other options are.
pub const HELP: &str =
    "  --p2p-key-file PATH    Iroh identity: 32 raw bytes, created mode 0600 when
                         missing. Defines this node's id for the `p2p` binding
                         (or CELLD_P2P_KEY_FILE). Without it the id is
                         generated afresh on every start
  --p2p-relay MODE       default (n0 public relays), disabled (direct UDP
                         only), or comma-separated relay URLs
                         (or CELLD_P2P_RELAY)
  --p2p-discovery LIST   dns, mdns, or none. Default: dns with the default
                         relays, none otherwise (or CELLD_P2P_DISCOVERY)
  --p2p-bind IP:PORT     Bind this one UDP address instead of every interface
                         (or CELLD_P2P_BIND)";

// ---------------------------------------------------------------------------
// Binding declarations
// ---------------------------------------------------------------------------

/// A DO that receives the inbound streams of a binding's ALPN.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accept {
    pub class: String,
    pub name: String,
}

/// One `p2p` entry of the deployed config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub binding: String,
    pub alpn: String,
    pub accept: Option<Accept>,
}

impl Binding {
    /// Read a deployed manifest's `{"type": "p2p", ...}` entry.
    pub fn from_manifest(value: &serde_json::Value) -> Option<Self> {
        let accept = value.get("accept").and_then(|accept| {
            Some(Accept {
                class: accept.get("class")?.as_str()?.to_string(),
                name: accept.get("name")?.as_str()?.to_string(),
            })
        });
        Some(Self {
            binding: value.get("name")?.as_str()?.to_string(),
            alpn: value.get("alpn")?.as_str()?.to_string(),
            accept,
        })
    }
}

/// An ALPN is a byte string on the wire; celld keeps it text so a deployment
/// config can carry it, and bounds it to what a TLS ALPN entry can hold.
pub fn valid_alpn(alpn: &str) -> bool {
    !alpn.is_empty()
        && alpn.len() <= 255
        && alpn
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_./:+".contains(&byte))
}

fn registry() -> &'static Mutex<BTreeMap<String, Vec<Binding>>> {
    static REGISTRY: OnceLock<Mutex<BTreeMap<String, Vec<Binding>>>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

/// Record the `p2p` bindings a script's deployment declares, replacing what an
/// earlier load of the same script declared. Called wherever a deployment's
/// manifest is read, so the binding table is whatever was last loaded.
pub fn declare(script: &str, bindings: Vec<Binding>) {
    {
        let mut scripts = registry().lock().unwrap();
        if bindings.is_empty() {
            scripts.remove(script);
        } else {
            scripts.insert(script.to_string(), bindings);
        }
    }
    refresh();
}

/// The bindings a script declares.
pub fn bindings_for(script: &str) -> Vec<Binding> {
    registry()
        .lock()
        .unwrap()
        .get(script)
        .cloned()
        .unwrap_or_default()
}

fn declared_any() -> bool {
    !registry().lock().unwrap().is_empty()
}

/// Where an ALPN's inbound streams go.
#[derive(Clone, Debug)]
struct AcceptTarget {
    binding: String,
    script: String,
    class: String,
    name: String,
}

impl AcceptTarget {
    /// The cell's scope: the same derivation `getByName` performs in the
    /// harness, so the host reaches the object the application means.
    fn scope(&self) -> String {
        let namespace = crate::engine_api::namespace_key(&self.script, &self.class);
        let id = crate::engine_api::durable_object_id_for_name(&namespace, &self.name);
        format!(
            "{}:{}",
            self.class,
            crate::engine_api::durable_object_id_hex(&id)
        )
    }
}

fn accept_targets() -> BTreeMap<String, AcceptTarget> {
    let mut targets = BTreeMap::new();
    for (script, bindings) in registry().lock().unwrap().iter() {
        for binding in bindings {
            let Some(accept) = &binding.accept else {
                continue;
            };
            let target = AcceptTarget {
                binding: binding.binding.clone(),
                script: script.clone(),
                class: accept.class.clone(),
                name: accept.name.clone(),
            };
            if let Some(first) = targets.get(&binding.alpn) {
                let first: &AcceptTarget = first;
                tracing::warn!(
                    alpn = %binding.alpn,
                    kept = %first.script,
                    ignored = %script,
                    "two deployments accept the same p2p ALPN; the first keeps it"
                );
                continue;
            }
            targets.insert(binding.alpn.clone(), target);
        }
    }
    targets
}

// ---------------------------------------------------------------------------
// The node
// ---------------------------------------------------------------------------

struct Installed {
    app: AppHandle,
    settings: Settings,
}

static INSTALLED: OnceLock<Installed> = OnceLock::new();
static NODE: OnceLock<Arc<Node>> = OnceLock::new();
static STARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A snapshot of the endpoint for status output.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Status {
    /// The node's public key, lowercase hex.
    pub node_id: String,
    pub relay: String,
    pub discovery: Vec<&'static str>,
    pub addrs: Vec<String>,
    pub accepts: Vec<String>,
    pub streams: usize,
}

/// The pooled connection for one (peer, ALPN), or none while it is dialed.
type PoolSlot = Arc<tokio::sync::Mutex<Option<Connection>>>;

pub struct Node {
    endpoint: Endpoint,
    settings: Settings,
    app: AppHandle,
    /// One QUIC connection per (peer, ALPN) that this node dialed. The slot
    /// serializes dialing, so concurrent `connect()` calls share one handshake.
    pool: Mutex<HashMap<(EndpointId, Vec<u8>), PoolSlot>>,
    streams: Mutex<HashMap<u64, Arc<Stream>>>,
    /// The runtime the endpoint runs on, for work a synchronous op starts.
    handle: tokio::runtime::Handle,
    claims: Mutex<HashMap<String, Claim>>,
    next_stream: AtomicU64,
    max_streams: usize,
}

/// Wire the node to its application handle and start the endpoint when it is
/// needed: a key file was configured, or a loaded deployment declares a `p2p`
/// binding. Called once at startup, after the first deployment loaded.
pub async fn install(app: AppHandle, settings: Settings) -> anyhow::Result<()> {
    let _ = INSTALLED.set(Installed { app, settings });
    ensure_started().await
}

/// Start the endpoint now if something asks for it. A deployment that gains a
/// `p2p` binding after boot, through a reload, reaches here from `declare`.
fn refresh() {
    if let Some(node) = NODE.get() {
        node.endpoint.set_alpns(alpns());
        return;
    }
    if INSTALLED.get().is_some() && declared_any() {
        tokio::spawn(async {
            if let Err(error) = ensure_started().await {
                tracing::error!(%error, "p2p endpoint failed to start after a deployment reload");
            }
        });
    }
}

async fn ensure_started() -> anyhow::Result<()> {
    let _starting = STARTING.lock().await;
    if NODE.get().is_some() {
        return Ok(());
    }
    let Some(installed) = INSTALLED.get() else {
        return Ok(());
    };
    if installed.settings.key_file.is_none() && !declared_any() {
        return Ok(());
    }
    let node = Node::bind(installed.app.clone(), installed.settings.clone()).await?;
    let node = Arc::new(node);
    let _ = NODE.set(node.clone());
    node.spawn_accept_loop();
    Ok(())
}

fn alpns() -> Vec<Vec<u8>> {
    accept_targets()
        .keys()
        .map(|alpn| alpn.as_bytes().to_vec())
        .collect()
}

/// The running node, if any.
pub fn node() -> Option<Arc<Node>> {
    NODE.get().cloned()
}

/// The node id, once the endpoint is up.
pub fn node_id() -> Option<String> {
    NODE.get().map(|node| node.endpoint.id().to_string())
}

pub fn status() -> Option<Status> {
    NODE.get().map(|node| node.status())
}

/// Read the node's secret key from `path`, creating the file when it does not
/// exist. The file is 32 raw bytes, the representation Vibestudio's server
/// persists, so one identity file works for both. An existing file that is not
/// exactly 32 bytes is an error and is never replaced: a node that silently
/// changed its id would orphan everything that pinned the old one.
pub fn load_or_create_key(path: &Path) -> anyhow::Result<SecretKey> {
    use std::io::Write as _;
    #[cfg(unix)]
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};

    match std::fs::read(path) {
        Ok(bytes) => {
            let bytes: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
                anyhow!(
                    "p2p key file {} must contain exactly 32 bytes, found {}",
                    path.display(),
                    bytes.len()
                )
            })?;
            #[cfg(unix)]
            {
                let mode = std::fs::metadata(path)?.permissions().mode();
                if mode & 0o077 != 0 {
                    tracing::warn!(path = %path.display(), mode = format!("{mode:o}"),
                        "p2p key file was readable by others; restricting it to 0600");
                    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
                }
            }
            return Ok(SecretKey::from_bytes(&bytes));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        if !parent.exists() {
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt as _;
                builder.mode(0o700);
            }
            builder
                .create(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
    }
    let key = SecretKey::generate();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("p2p.key");
    let temporary = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("create {}", temporary.display()))?;
    file.write_all(&key.to_bytes())?;
    file.sync_all()?;
    drop(file);
    // A hard link refuses to replace, so two nodes started together on one
    // path agree on whichever created the file first.
    match std::fs::hard_link(&temporary, path) {
        Ok(()) => {
            let _ = std::fs::remove_file(&temporary);
            Ok(key)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = std::fs::remove_file(&temporary);
            load_or_create_key(path)
        }
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            Err(error).with_context(|| format!("create {}", path.display()))
        }
    }
}

impl Node {
    async fn bind(app: AppHandle, settings: Settings) -> anyhow::Result<Self> {
        let (key, persistent) = match &settings.key_file {
            Some(path) => (load_or_create_key(path)?, true),
            None => (SecretKey::generate(), false),
        };
        let discovery = settings.discovery();
        let mut builder = Endpoint::builder(presets::Minimal)
            .secret_key(key)
            .alpns(alpns())
            .relay_mode(settings.relay_mode()?);
        if discovery.dns {
            use iroh::address_lookup::{DnsAddressLookup, PkarrPublisher, PkarrResolver};
            builder = builder
                .address_lookup(PkarrPublisher::n0_dns())
                .address_lookup(PkarrResolver::n0_dns())
                .address_lookup(DnsAddressLookup::n0_dns());
        }
        if discovery.mdns {
            builder =
                builder.address_lookup(iroh_mdns_address_lookup::MdnsAddressLookup::builder());
        }
        if let Some(bind) = settings.bind {
            builder = builder
                .clear_ip_transports()
                .bind_addr(bind)
                .map_err(|error| anyhow!("--p2p-bind {bind}: {error}"))?;
        }
        let endpoint = builder
            .bind()
            .await
            .map_err(|error| anyhow!("bind the p2p endpoint: {error}"))?;
        let max_streams =
            crate::env_vars::positive_or("CELLD_P2P_MAX_STREAMS", DEFAULT_MAX_STREAMS as u64)?
                as usize;
        let node = Self {
            endpoint,
            settings,
            app,
            pool: Mutex::new(HashMap::new()),
            streams: Mutex::new(HashMap::new()),
            handle: tokio::runtime::Handle::current(),
            claims: Mutex::new(HashMap::new()),
            next_stream: AtomicU64::new(1),
            max_streams,
        };
        let status = node.status();
        crate::cli_output::Output::new(crate::cli_output::Format::Text).line(format_args!(
            "celld p2p node {} (relay={}, discovery={}, key={})",
            status.node_id,
            status.relay,
            if status.discovery.is_empty() {
                "none".to_string()
            } else {
                status.discovery.join("+")
            },
            if persistent { "file" } else { "ephemeral" },
        ))?;
        if !persistent {
            tracing::warn!(
                "the p2p node id is generated for this process only; set --p2p-key-file \
                 so peers can keep dialing the same id after a restart"
            );
        }
        Ok(node)
    }

    pub fn id(&self) -> EndpointId {
        self.endpoint.id()
    }

    fn status(&self) -> Status {
        let discovery = self.settings.discovery();
        let mut kinds = Vec::new();
        if discovery.dns {
            kinds.push("dns");
        }
        if discovery.mdns {
            kinds.push("mdns");
        }
        Status {
            node_id: self.endpoint.id().to_string(),
            relay: match &self.settings.relay {
                RelaySetting::Default => "default".to_string(),
                RelaySetting::Disabled => "disabled".to_string(),
                RelaySetting::Custom(urls) => urls.join(","),
            },
            discovery: kinds,
            addrs: self
                .endpoint
                .bound_sockets()
                .into_iter()
                .map(|addr| addr.to_string())
                .collect(),
            accepts: accept_targets().into_keys().collect(),
            streams: self.streams.lock().unwrap().len(),
        }
    }

    fn spawn_accept_loop(self: &Arc<Self>) {
        let node = self.clone();
        tokio::spawn(async move {
            while let Some(incoming) = node.endpoint.accept().await {
                let node = node.clone();
                tokio::spawn(async move {
                    // The full handshake, never early data: the peer's id is
                    // authenticated before a single stream is looked at.
                    match incoming.await {
                        Ok(connection) => node.serve_connection(connection).await,
                        Err(error) => tracing::debug!(%error, "p2p handshake failed"),
                    }
                });
            }
        });
    }

    async fn serve_connection(self: &Arc<Self>, connection: Connection) {
        let remote = connection.remote_id();
        let alpn = connection.alpn().to_vec();
        let Some(alpn_text) = std::str::from_utf8(&alpn).ok().map(str::to_string) else {
            connection.close(VarInt::from_u32(CODE_NO_HANDLER), b"unknown protocol");
            return;
        };
        loop {
            let (send, recv) = match connection.accept_bi().await {
                Ok(pair) => pair,
                Err(_) => return,
            };
            let node = self.clone();
            let connection = connection.clone();
            let alpn_text = alpn_text.clone();
            tokio::spawn(async move {
                node.dispatch_inbound(connection, remote, alpn_text, send, recv)
                    .await;
            });
        }
    }

    async fn dispatch_inbound(
        self: &Arc<Self>,
        connection: Connection,
        remote: EndpointId,
        alpn: String,
        send: SendStream,
        recv: RecvStream,
    ) {
        let reset = |mut send: SendStream, mut recv: RecvStream, code: u32| {
            let _ = send.reset(VarInt::from_u32(code));
            let _ = recv.stop(VarInt::from_u32(code));
        };
        let Some(target) = accept_targets().remove(&alpn) else {
            return reset(send, recv, CODE_NO_HANDLER);
        };
        let scope = target.scope();
        let stream = Arc::new(Stream::new(connection, remote, alpn.clone(), send, recv));
        let Some(id) = self.insert_stream(stream.clone()) else {
            tracing::warn!(%remote, "p2p stream refused: node stream limit reached");
            return stream.abort(CODE_BUSY).await;
        };
        let token = claim_token();
        self.claims.lock().unwrap().insert(
            token.clone(),
            Claim {
                id,
                scope: scope.clone(),
                binding: target.binding.clone(),
                created: std::time::Instant::now(),
            },
        );
        self.prune_claims();
        let outcome = self.deliver(&target, &scope, &token).await;
        // A claimed stream now belongs to the accepting cell and outlives the
        // event that delivered it. One whose token is still here was never
        // claimed (the event did not start, or never reached the handler),
        // and nobody else can ever take it.
        let claimed = self.claims.lock().unwrap().remove(&token).is_none();
        let code = match outcome {
            Ok(()) => None,
            Err(error) => {
                tracing::warn!(%remote, %alpn, error = %format!("{error:#}"),
                    "p2p inbound stream was not served");
                Some(match error.downcast_ref::<NotOwner>() {
                    Some(_) => CODE_NOT_OWNER,
                    None => CODE_HANDLER_FAILED,
                })
            }
        };
        match code {
            Some(code) => {
                self.remove_stream(id);
                stream.abort(code).await;
            }
            None if !claimed => {
                self.remove_stream(id);
                stream.close_graceful().await;
            }
            None => {}
        }
    }

    /// Send the accepting cell the internal fetch that carries the claim
    /// token, and wait for its event to end. The event lasts as long as the
    /// handler (and its `waitUntil` work) does, which bounds the stream.
    async fn deliver(&self, target: &AcceptTarget, scope: &str, token: &str) -> anyhow::Result<()> {
        use crate::cell_dispatch::{dispatch_call_attempt, CallAttempt, CallContext, CallPayload};
        use crate::engine_api::RequestBody;
        let payload = CallPayload {
            name: Some(target.name.clone()),
            url: format!("{ACCEPT_URL_PREFIX}{token}"),
            method: "POST".to_string(),
            body: RequestBody::Bytes(bytes::Bytes::new()),
            headers: Vec::new(),
        };
        let mut call = CallContext {
            scope: scope.to_string(),
            request_id: None,
            parent: None,
            deliver_abort_to_handler: false,
            cancel: None,
            order: None,
            body_guard: crate::http_streams::RequestBodyGuard::of(&payload.body),
        };
        match dispatch_call_attempt(&self.app, &mut call, payload, || {}).await {
            CallAttempt::Local(Ok(response)) if response.status == 200 => Ok(()),
            CallAttempt::Local(Ok(response)) => bail!(
                "the accepting cell answered {}: {}",
                response.status,
                String::from_utf8_lossy(&response.body)
            ),
            CallAttempt::Local(Err(error))
            | CallAttempt::RouteError(error)
            | CallAttempt::Unstarted(error) => Err(error),
            CallAttempt::RefusedWebSocket { error, .. } => Err(error),
            CallAttempt::Remote { node, .. } => Err(anyhow::Error::new(NotOwner).context(format!(
                "cell {scope} is owned by node {node}; p2p streams are served only by \
                     the owning node"
            ))),
        }
    }

    fn prune_claims(&self) {
        self.claims
            .lock()
            .unwrap()
            .retain(|_, claim| claim.created.elapsed() < CLAIM_TTL);
    }

    fn insert_stream(&self, stream: Arc<Stream>) -> Option<u64> {
        let mut streams = self.streams.lock().unwrap();
        if streams.len() >= self.max_streams {
            return None;
        }
        let id = STREAM_ID_BIT | self.next_stream.fetch_add(1, Ordering::Relaxed);
        streams.insert(id, stream.clone());
        drop(streams);
        // A peer that vanishes takes the connection with it, and nothing else
        // would tell a cell-owned stream that nobody is reading it: release
        // the stream (and the cell it pins) when the connection dies.
        let connection = stream._connection.clone();
        let done = stream.closed.clone();
        self.handle.spawn(async move {
            crate::asyncrt::select! {
                _ = done.cancelled() => {}
                reason = connection.closed() => {
                    if let Some(stream) = self::stream(id) {
                        *stream.failure.lock().unwrap() =
                            Some(format!("connection lost: {reason}"));
                    }
                    close(id, None);
                }
            }
        });
        Some(id)
    }

    /// Forget a stream, and tell the core its cell no longer holds the
    /// transport. Every path that ends a stream comes through here, so the
    /// pin cannot outlive the stream.
    fn remove_stream(&self, id: u64) -> Option<Arc<Stream>> {
        let stream = self.streams.lock().unwrap().remove(&id)?;
        if let Some(owner) = stream.owner.lock().unwrap().take() {
            self.app.websocket_closed(owner.cell, id);
        }
        Some(stream)
    }

    /// Open one bidirectional stream to `remote` on `alpn`, over the node's
    /// connection for that pair when it has a live one.
    pub async fn open(
        &self,
        alpn: &str,
        remote: EndpointId,
        addrs: Vec<SocketAddr>,
        relay: Option<RelayUrl>,
    ) -> anyhow::Result<u64> {
        ensure!(
            remote != self.endpoint.id(),
            "a node cannot open a p2p stream to itself"
        );
        let mut address = EndpointAddr::new(remote);
        for addr in addrs {
            address = address.with_ip_addr(addr);
        }
        if let Some(relay) = relay {
            address = address.with_relay_url(relay);
        }
        let slot = self
            .pool
            .lock()
            .unwrap()
            .entry((remote, alpn.as_bytes().to_vec()))
            .or_default()
            .clone();
        let mut last = None;
        for _ in 0..2 {
            let connection = {
                let mut guard = slot.lock().await;
                match guard.as_ref().filter(|c| c.close_reason().is_none()) {
                    Some(connection) => connection.clone(),
                    None => {
                        let connection = tokio::time::timeout(
                            DIAL_TIMEOUT,
                            self.endpoint.connect(address.clone(), alpn.as_bytes()),
                        )
                        .await
                        .map_err(|_| anyhow!("timed out connecting to {remote}"))?
                        .map_err(|error| anyhow!("connect to {remote}: {error}"))?;
                        *guard = Some(connection.clone());
                        connection
                    }
                }
            };
            match connection.open_bi().await {
                Ok((send, recv)) => {
                    let stream = Arc::new(Stream::new(
                        connection,
                        remote,
                        alpn.to_string(),
                        send,
                        recv,
                    ));
                    return self
                        .insert_stream(stream)
                        .ok_or_else(|| anyhow!("the node's p2p stream limit is reached"));
                }
                Err(error) => {
                    // The pooled connection died between the liveness check
                    // and the open; drop it and dial once more.
                    let mut guard = slot.lock().await;
                    if guard
                        .as_ref()
                        .is_some_and(|c| c.stable_id() == connection.stable_id())
                    {
                        *guard = None;
                    }
                    last = Some(error);
                }
            }
        }
        Err(anyhow!(
            "open a stream to {remote}: {}",
            last.map(|e| e.to_string()).unwrap_or_default()
        ))
    }

    /// Close every connection and the endpoint, so peers see an orderly close.
    pub async fn shutdown(&self) {
        self.endpoint.close().await;
    }
}

#[derive(Debug)]
struct NotOwner;

impl std::fmt::Display for NotOwner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the accepting cell is owned by another node")
    }
}

impl std::error::Error for NotOwner {}

struct Claim {
    id: u64,
    scope: String,
    binding: String,
    created: std::time::Instant,
}

fn claim_token() -> String {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).expect("the operating system provides randomness");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// What a redeemed claim tells the harness.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Claimed {
    pub id: u64,
    pub remote_node_id: String,
    pub alpn: String,
    pub binding: String,
}

/// Redeem an inbound stream's claim token for the cell the token was minted
/// for. The token is single-use; a wrong cell or a replay finds nothing.
pub fn claim(token: &str, scope: &str) -> Option<Claimed> {
    let node = NODE.get()?;
    let mut claims = node.claims.lock().unwrap();
    if claims.get(token).is_none_or(|claim| claim.scope != scope) {
        return None;
    }
    let claim = claims.remove(token)?;
    drop(claims);
    let stream = node.streams.lock().unwrap().get(&claim.id)?.clone();
    Some(Claimed {
        id: claim.id,
        remote_node_id: stream.remote.to_string(),
        alpn: stream.alpn.clone(),
        binding: claim.binding,
    })
}

// ---------------------------------------------------------------------------
// Streams
// ---------------------------------------------------------------------------

/// One bidirectional QUIC stream. The halves sit behind separate async locks
/// so a pending read never blocks a write, as a TCP socket's do.
pub struct Stream {
    /// The cell that owns the stream, once it has been adopted.
    owner: Mutex<Option<Owner>>,
    /// Why the stream ended when the host ended it, not a side: a read parked
    /// on a lost connection reports this instead of a clean end-of-stream.
    failure: Mutex<Option<String>>,
    send: tokio::sync::Mutex<Option<SendStream>>,
    recv: tokio::sync::Mutex<Option<RecvStream>>,
    pub remote: EndpointId,
    pub alpn: String,
    /// Cancelled by `close`, so a read or write parked on the wire returns
    /// instead of holding its half forever.
    closed: CancellationToken,
    /// Keeps the connection alive for as long as one of its streams is.
    _connection: Connection,
}

pub fn is_stream_id(id: u64) -> bool {
    id & STREAM_ID_BIT != 0
}

/// The cell a stream belongs to. `cell` is the root cell the node keeps
/// resident for it; `scope` is the object that opened it, which differs from
/// `cell` for a facet.
#[derive(Clone, Debug)]
struct Owner {
    cell: String,
    scope: String,
}

impl Stream {
    fn new(
        connection: Connection,
        remote: EndpointId,
        alpn: String,
        send: SendStream,
        recv: RecvStream,
    ) -> Self {
        Self {
            owner: Mutex::new(None),
            failure: Mutex::new(None),
            send: tokio::sync::Mutex::new(Some(send)),
            recv: tokio::sync::Mutex::new(Some(recv)),
            remote,
            alpn,
            closed: CancellationToken::new(),
            _connection: connection,
        }
    }

    /// The cell and the object that hold this stream, once adopted.
    pub fn owner(&self) -> Option<(String, String)> {
        self.owner
            .lock()
            .unwrap()
            .as_ref()
            .map(|owner| (owner.cell.clone(), owner.scope.clone()))
    }

    /// Up to one chunk. An empty result is the peer finishing its side.
    pub async fn read(&self) -> Result<Vec<u8>, String> {
        let mut guard = self.recv.lock().await;
        let Some(recv) = guard.as_mut() else {
            return Ok(Vec::new());
        };
        let mut buffer = vec![0_u8; READ_CHUNK];
        crate::asyncrt::select! {
            _ = self.closed.cancelled() => match self.failure.lock().unwrap().clone() {
                Some(failure) => Err(failure),
                None => Ok(Vec::new()),
            },
            read = recv.read(&mut buffer) => match read {
                Ok(Some(n)) => {
                    buffer.truncate(n);
                    Ok(buffer)
                }
                Ok(None) => Ok(Vec::new()),
                Err(error) => Err(describe_read_error(&error)),
            },
        }
    }

    pub async fn write(&self, bytes: &[u8]) -> Result<(), String> {
        let mut guard = self.send.lock().await;
        let send = guard.as_mut().ok_or("stream is closed")?;
        crate::asyncrt::select! {
            _ = self.closed.cancelled() => Err("stream is closed".to_string()),
            written = send.write_all(bytes) => written.map_err(|error| describe_write_error(&error)),
        }
    }

    /// Finish the send half: the peer's reads end after what was written.
    pub async fn shutdown(&self) -> Result<(), String> {
        let mut guard = self.send.lock().await;
        let send = guard.as_mut().ok_or("stream is closed")?;
        send.finish().map_err(|_| "stream is closed".to_string())
    }

    /// Orderly close of both halves: finish what was written, stop reading.
    pub async fn close_graceful(&self) {
        self.closed.cancel();
        // `closed` makes any read or write parked on a half return, so these
        // locks are only ever held briefly.
        if let Some(mut send) = self.send.lock().await.take() {
            let _ = send.finish();
        }
        if let Some(mut recv) = self.recv.lock().await.take() {
            let _ = recv.stop(VarInt::from_u32(0));
        }
    }

    /// Abrupt close: the peer's reads fail with `code`, its writes too.
    pub async fn abort(&self, code: u32) {
        self.closed.cancel();
        if let Some(mut send) = self.send.lock().await.take() {
            let _ = send.reset(VarInt::from_u32(code));
        }
        if let Some(mut recv) = self.recv.lock().await.take() {
            let _ = recv.stop(VarInt::from_u32(code));
        }
    }
}

fn describe_read_error(error: &iroh::endpoint::ReadError) -> String {
    use iroh::endpoint::ReadError;
    match error {
        ReadError::Reset(code) => format!("stream reset by peer (code {})", code.into_inner()),
        other => format!("stream read failed: {other}"),
    }
}

fn describe_write_error(error: &iroh::endpoint::WriteError) -> String {
    use iroh::endpoint::WriteError;
    match error {
        WriteError::Stopped(code) => {
            format!("peer closed the stream (code {})", code.into_inner())
        }
        other => format!("stream write failed: {other}"),
    }
}

/// Look up a stream by id for the ops.
pub fn stream(id: u64) -> Option<Arc<Stream>> {
    NODE.get()?.streams.lock().unwrap().get(&id).cloned()
}

/// Close and forget a stream. `code` resets it (the peer sees an error);
/// without one the close is orderly. Idempotent, and synchronous for the
/// caller: the wire work runs on the node's own tasks.
pub fn close(id: u64, code: Option<u32>) {
    let Some(node) = NODE.get() else { return };
    if let Some(stream) = node.remove_stream(id) {
        node.handle.spawn(async move {
            match code {
                Some(code) => stream.abort(code).await,
                None => stream.close_graceful().await,
            }
        });
    }
}

/// Make a stream the cell's: register it with the core as an outbound
/// transport of `cell`, so it pins the cell and is counted against the same
/// per-cell cap and node pin budget as an outbound WebSocket. `scope` is the
/// object that holds it (a facet's own scope, or the cell's).
///
/// The owner is recorded before the core is asked, so a lifecycle close that
/// lands in between still finds the stream. A refusal leaves nothing behind;
/// the caller closes the stream.
pub async fn adopt(id: u64, cell: &str, scope: &str) -> anyhow::Result<()> {
    let node = NODE
        .get()
        .ok_or_else(|| anyhow!("the p2p endpoint is not running on this node"))?;
    let stream = node
        .streams
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| anyhow!("the p2p stream is closed"))?;
    {
        let mut owner = stream.owner.lock().unwrap();
        ensure!(owner.is_none(), "the p2p stream already has an owner");
        *owner = Some(Owner {
            cell: cell.to_string(),
            scope: scope.to_string(),
        });
    }
    if let Err(error) = node
        .app
        .websocket_opened(cell.to_string(), id, celld_logic::WebSocketKind::Outbound)
        .await
    {
        *stream.owner.lock().unwrap() = None;
        return Err(error);
    }
    // The stream may have ended while the core was answering; the core's
    // entry would then pin the cell for a stream nobody holds.
    if !node.streams.lock().unwrap().contains_key(&id) {
        node.app.websocket_closed(cell.to_string(), id);
        bail!("the p2p stream closed while its cell was adopting it");
    }
    Ok(())
}

/// Reset every stream `cell` owns, because the cell left this node or was
/// reset (see [`CODE_CELL_ENDED`]). Facets of the cell are included.
pub fn close_cell(cell: &str) {
    close_where(|owner| owner.cell == cell);
}

/// Reset the streams one object opened, because the isolate that held them
/// gave the object back or took it in again. A stream is bound to the
/// instance that holds its reader and writer; when that instance is replaced
/// (an abort, a crash, a reset, a swap) nothing is left to use it.
pub fn close_object(scope: &str) {
    close_where(|owner| owner.scope == scope);
}

fn close_where(matches: impl Fn(&Owner) -> bool) {
    let Some(node) = NODE.get() else { return };
    let ids: Vec<u64> = node
        .streams
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, stream)| stream.owner.lock().unwrap().as_ref().is_some_and(&matches))
        .map(|(id, _)| *id)
        .collect();
    for id in ids {
        close(id, Some(CODE_CELL_ENDED));
    }
}

/// How many streams `cell` owns. For status output and tests.
pub fn owned_by(cell: &str) -> usize {
    NODE.get().map_or(0, |node| {
        node.streams
            .lock()
            .unwrap()
            .values()
            .filter(|stream| {
                stream
                    .owner
                    .lock()
                    .unwrap()
                    .as_ref()
                    .is_some_and(|owner| owner.cell == cell)
            })
            .count()
    })
}

/// What `env.<binding>.connect()` asks of the node.
pub struct Dial {
    pub alpn: String,
    pub node_id: EndpointId,
    pub addrs: Vec<SocketAddr>,
    pub relay: Option<RelayUrl>,
}

impl Dial {
    /// Validate what a cell passed to `connect()`.
    pub fn parse(
        alpn: &str,
        node_id: &str,
        addrs: &[String],
        relay: Option<&str>,
    ) -> anyhow::Result<Self> {
        ensure!(
            node_id.len() == 64
                && node_id
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
            "connect(): nodeId must be 64 lowercase hex characters"
        );
        ensure!(addrs.len() <= 16, "connect(): at most 16 addrs");
        let addrs = addrs
            .iter()
            .map(|addr| {
                addr.parse::<SocketAddr>()
                    .map_err(|_| anyhow!("connect(): {addr:?} is not an IP:PORT address"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let relay = relay
            .map(|url| RelayUrl::from_str(url).map_err(|_| anyhow!("connect(): invalid relay URL")))
            .transpose()?;
        Ok(Self {
            alpn: alpn.to_string(),
            node_id: EndpointId::from_str(node_id)
                .map_err(|_| anyhow!("connect(): nodeId is not a valid public key"))?,
            addrs,
            relay,
        })
    }
}

/// Dial and register one stream, returning its id and the peer's id.
pub async fn connect(dial: Dial) -> anyhow::Result<(u64, String)> {
    let node = NODE
        .get()
        .ok_or_else(|| anyhow!("the p2p endpoint is not running on this node"))?;
    let id = node
        .open(&dial.alpn, dial.node_id, dial.addrs, dial.relay)
        .await?;
    Ok((id, dial.node_id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_file_is_created_once_and_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("p2p.key");
        let first = load_or_create_key(&path).unwrap();
        let second = load_or_create_key(&path).unwrap();
        assert_eq!(first.public(), second.public(), "the id must be stable");
        assert_eq!(std::fs::read(&path).unwrap().len(), 32);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn key_file_of_the_wrong_length_is_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p2p.key");
        std::fs::write(&path, b"short").unwrap();
        assert!(load_or_create_key(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"short");
    }

    #[cfg(unix)]
    #[test]
    fn a_loose_key_file_is_tightened() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p2p.key");
        let key = load_or_create_key(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(load_or_create_key(&path).unwrap().public(), key.public());
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn settings_parse() {
        let mut settings = Settings::default();
        settings.set_relay("disabled").unwrap();
        assert_eq!(settings.relay, RelaySetting::Disabled);
        settings
            .set_relay("https://relay.example.com./, https://r2.example.com")
            .unwrap();
        assert!(matches!(&settings.relay, RelaySetting::Custom(urls) if urls.len() == 2));
        assert!(settings.set_relay("not a url").is_err());
        settings.set_discovery("dns,mdns").unwrap();
        assert_eq!(
            settings.discovery,
            Some(Discovery {
                dns: true,
                mdns: true
            })
        );
        assert!(settings.set_discovery("gossip").is_err());
        settings.set_bind("127.0.0.1:4000").unwrap();
        assert!(settings.set_bind("4000").is_err());
    }

    #[test]
    fn discovery_defaults_follow_the_relay() {
        let mut settings = Settings::default();
        assert_eq!(
            settings.discovery(),
            Discovery {
                dns: true,
                mdns: false
            }
        );
        settings.set_relay("disabled").unwrap();
        assert_eq!(settings.discovery(), Discovery::default());
    }

    #[test]
    fn dial_validates_its_input() {
        let id = SecretKey::generate().public().to_string();
        assert!(Dial::parse("a/1", &id, &["127.0.0.1:1".into()], None).is_ok());
        assert!(Dial::parse("a/1", "nope", &[], None).is_err());
        assert!(Dial::parse("a/1", &id.to_uppercase(), &[], None).is_err());
        assert!(Dial::parse("a/1", &id, &["localhost".into()], None).is_err());
    }

    #[test]
    fn alpn_validation() {
        assert!(valid_alpn("chronolog/1"));
        assert!(!valid_alpn(""));
        assert!(!valid_alpn("has space"));
        assert!(!valid_alpn(&"a".repeat(256)));
    }

    #[test]
    fn manifest_round_trip() {
        let value = serde_json::json!({
            "type": "p2p", "name": "P2P", "alpn": "x/1",
            "accept": { "class": "Node", "name": "node" }
        });
        let binding = Binding::from_manifest(&value).unwrap();
        assert_eq!(binding.accept.unwrap().class, "Node");
    }
}
