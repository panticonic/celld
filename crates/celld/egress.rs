// Copyright 2026 Deno Land Inc. Apache-2.0 license.

// Egress is network transport outside the Actor execution domain: it dials
// the node's egress proxy with the real clock, the real RNG, and the real
// filesystem (the secret file), exactly as `ws_client` does for peers.
#![allow(clippy::disallowed_methods)]

//! The outbound policy hook: one node-level egress proxy for every isolate.
//!
//! When a node is configured with `--egress-proxy URL` (or
//! `CELLD_EGRESS_PROXY`) and an HMAC secret, no isolate dials a destination
//! itself. Every outbound `fetch` (each redirect hop included), outbound
//! WebSocket and `connect()` socket is sent to the proxy instead, with the
//! caller's identity attached and authenticated. The proxy decides whether to
//! admit the request and where it really goes. Destination DNS is therefore
//! the proxy's business: celld resolves only the proxy's own address.
//!
//! This module is V8-free. `crate::js::egress` decides, per op, whether a
//! request is `Direct`, `Proxy`, a `Broker` (Fetcher `globalOutbound`) or
//! denied, and captures the identity; this module owns the wire format.
//!
//! # Wire format
//!
//! | traffic | request sent to the proxy |
//! | --- | --- |
//! | `fetch` | `METHOD <absolute-url> HTTP/1.1` in plaintext (forward-proxy, absolute form), for `http://` and `https://` targets alike. The proxy terminates the upstream TLS. |
//! | outbound WebSocket | `GET ws://host/path HTTP/1.1` (or `wss://`) with the `Upgrade` handshake headers, absolute form. |
//! | `connect()` | `CONNECT host:port HTTP/1.1`. `secure: true` / `startTls()` run TLS *through* the tunnel, end to end. |
//!
//! Every one of them carries three headers:
//!
//! * `x-celld-egress-identity`: base64url (no padding) of the identity JSON,
//!   see [`EgressIdentity`].
//! * `x-celld-egress-nonce`: base64url of 16 fresh random bytes.
//! * `x-celld-egress-mac`: base64url of
//!   `HMAC-SHA256(secret, "celld-egress/v1\n" kind "\n" method "\n" target "\n" identity "\n" nonce "\n")`
//!   where `kind` is `fetch`, `websocket` or `connect`, `method` is the HTTP
//!   method on the request line, `target` is the request-line target exactly
//!   as sent (absolute URL, or `host:port` for CONNECT), and `identity` and
//!   `nonce` are the two header values verbatim.
//!
//! The MAC is bound to the request and carries no timestamp, so there is no
//! clock to skew and nothing expires. A captured header triple cannot be moved
//! to a different destination, method or channel. A proxy that wants replay
//! protection can remember nonces; it never has to.
//!
//! A Worker cannot choose its identity: the identity is read from the isolate
//! and the running event by the host, any `x-celld-egress-*` header the
//! Worker supplies is removed, and only the Worker Loader of a dynamic worker
//! can contribute to its `egressIdentity` chain.

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use futures_util::StreamExt as _;
use hmac::{Hmac, Mac};
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::BodyExt as _;
use hyper::body::Frame;
use hyper_util::rt::TokioIo;
use sha2::Sha256;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

pub use crate::engine_api::HttpChunkStream;

pub const IDENTITY_HEADER: &str = "x-celld-egress-identity";
pub const NONCE_HEADER: &str = "x-celld-egress-nonce";
pub const MAC_HEADER: &str = "x-celld-egress-mac";
/// Prefix of every header this module owns. Worker-supplied headers with it
/// are dropped before the real ones are added.
const HEADER_PREFIX: &str = "x-celld-egress-";
/// Version tag mixed into every MAC. A future scheme bumps it.
pub const MAC_DOMAIN: &str = "celld-egress/v1";
const MIN_SECRET_BYTES: usize = 16;
const PROXY_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const MAX_RESPONSE_HEAD: usize = 16 * 1024;
const MAX_REDIRECTS: usize = 10;

pub trait Io: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Io for T {}
pub type BoxIo = Box<dyn Io>;

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
}

// ---------------------------------------------------------------------------
// Node-level configuration
// ---------------------------------------------------------------------------

/// What the operator typed: the proxy URL and where the secret comes from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NodeSettings {
    pub proxy: Option<String>,
    pub secret: Option<String>,
    pub secret_file: Option<PathBuf>,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

impl NodeSettings {
    /// `CELLD_EGRESS_PROXY`, `CELLD_EGRESS_SECRET`, `CELLD_EGRESS_SECRET_FILE`.
    pub fn from_environment() -> Self {
        Self {
            proxy: env("CELLD_EGRESS_PROXY"),
            secret: env("CELLD_EGRESS_SECRET"),
            secret_file: env("CELLD_EGRESS_SECRET_FILE").map(PathBuf::from),
        }
    }

    /// Validate and install the node proxy. `Ok(false)` when no proxy is
    /// configured, which leaves every isolate on direct egress.
    pub fn install(&self) -> Result<bool> {
        let endpoint = self.endpoint()?;
        let installed = endpoint.is_some();
        set_node_proxy(endpoint.map(Arc::new));
        Ok(installed)
    }

    /// Validate the settings and build the endpoint they describe, without
    /// installing it.
    pub fn endpoint(&self) -> Result<Option<ProxyEndpoint>> {
        let Some(url) = self.proxy.as_deref() else {
            if self.secret.is_some() || self.secret_file.is_some() {
                bail!("an egress secret needs --egress-proxy or CELLD_EGRESS_PROXY");
            }
            return Ok(None);
        };
        let secret = match (&self.secret, &self.secret_file) {
            (Some(_), Some(_)) => bail!(
                "set only one of the egress secret and the egress secret file \
                 (--egress-secret, --egress-secret-file, CELLD_EGRESS_SECRET, \
                 CELLD_EGRESS_SECRET_FILE)"
            ),
            (Some(secret), None) => secret.trim_end_matches(['\r', '\n']).as_bytes().to_vec(),
            (None, Some(path)) => std::fs::read(path)
                .with_context(|| format!("read the egress secret file {}", path.display()))?
                .trim_ascii_end()
                .to_vec(),
            (None, None) => bail!(
                "--egress-proxy requires an egress secret: --egress-secret-file PATH, \
                 CELLD_EGRESS_SECRET_FILE, CELLD_EGRESS_SECRET or --egress-secret"
            ),
        };
        ProxyEndpoint::new(url, secret).map(Some)
    }
}

/// The node's egress proxy and the secret that authenticates identities to it.
pub struct ProxyEndpoint {
    url: String,
    tls: bool,
    host: String,
    port: u16,
    secret: Vec<u8>,
}

impl std::fmt::Debug for ProxyEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyEndpoint")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

impl ProxyEndpoint {
    pub fn new(url: &str, secret: Vec<u8>) -> Result<Self> {
        let parsed = url::Url::parse(url).with_context(|| format!("egress proxy URL {url:?}"))?;
        let tls = match parsed.scheme() {
            "http" => false,
            "https" => true,
            other => bail!("egress proxy URL must be http:// or https://, not {other}://"),
        };
        if !parsed.username().is_empty() || parsed.password().is_some() {
            bail!("egress proxy URL must not carry credentials; use the egress secret");
        }
        let host = parsed
            .host_str()
            .context("egress proxy URL has no host")?
            .to_string();
        let port = parsed.port_or_known_default().context("egress proxy port")?;
        if secret.len() < MIN_SECRET_BYTES {
            bail!("the egress secret must be at least {MIN_SECRET_BYTES} bytes");
        }
        Ok(Self {
            url: url.to_string(),
            tls,
            host,
            port,
            secret,
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

static NODE_PROXY: OnceLock<RwLock<Option<Arc<ProxyEndpoint>>>> = OnceLock::new();

fn node_proxy_cell() -> &'static RwLock<Option<Arc<ProxyEndpoint>>> {
    NODE_PROXY.get_or_init(Default::default)
}

/// Install (or, with `None`, remove) the node-wide egress proxy. Startup calls
/// this once; tests call it freely.
pub fn set_node_proxy(endpoint: Option<Arc<ProxyEndpoint>>) {
    *node_proxy_cell().write().unwrap() = endpoint;
}

pub fn node_proxy() -> Option<Arc<ProxyEndpoint>> {
    node_proxy_cell().read().unwrap().clone()
}

// ---------------------------------------------------------------------------
// Identity and MAC
// ---------------------------------------------------------------------------

/// Who is calling. Serialized as the JSON of `x-celld-egress-identity`.
///
/// Every field is always present (`null` when unknown), so a verifier reads a
/// fixed shape. The MAC covers the header's exact bytes, so a verifier never
/// re-serializes.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EgressIdentity {
    /// Scheme version, `1`.
    pub v: u8,
    /// The deployment: the fleet-wide version string of the deployment that
    /// holds this isolate.
    pub deployment: String,
    /// The deployed script. For a dynamic worker, the deployed script at the
    /// root of its loader chain (never the loader-minted `__loader:N` name).
    pub script: String,
    /// True when the code runs in a Worker Loader isolate.
    pub loaded: bool,
    /// The Durable Object class whose cell is running, when one is.
    pub class: Option<String>,
    /// The cell scope (`Class:id`) whose event raised the request, if any.
    pub cell: Option<String>,
    /// For a Durable Object facet, the root cell that owns it.
    pub root: Option<String>,
    /// The `celld: { egressIdentity }` strings the loaders of a dynamic worker
    /// supplied, outermost loader first. A nested worker's chain always starts
    /// with its parent's, so a loaded worker cannot shed or rewrite the
    /// identity its loader gave it. Empty for a deployed worker.
    pub egress_identity: Vec<String>,
}

/// The kind of traffic a MAC is bound to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Fetch,
    WebSocket,
    Connect,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Fetch => "fetch",
            Kind::WebSocket => "websocket",
            Kind::Connect => "connect",
        }
    }
}

fn mac_for(
    secret: &[u8],
    kind: Kind,
    method: &str,
    target: &str,
    identity: &str,
    nonce: &str,
) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC takes any key length");
    for part in [MAC_DOMAIN, kind.as_str(), method, target, identity, nonce] {
        mac.update(part.as_bytes());
        mac.update(b"\n");
    }
    mac
}

/// Check a MAC the way a proxy would. Constant time. Exposed so tests, and
/// Rust proxies, share the one definition.
pub fn verify_mac(
    secret: &[u8],
    kind: Kind,
    method: &str,
    target: &str,
    identity: &str,
    nonce: &str,
    mac: &str,
) -> bool {
    let Ok(mac) = b64().decode(mac) else {
        return false;
    };
    mac_for(secret, kind, method, target, identity, nonce)
        .verify_slice(&mac)
        .is_ok()
}

/// A request's route through the node proxy: the endpoint plus the identity,
/// already encoded. Cheap to clone; one is made per op.
#[derive(Clone)]
pub struct ProxyRoute {
    endpoint: Arc<ProxyEndpoint>,
    identity: Arc<EgressIdentity>,
    identity_header: Arc<str>,
}

impl ProxyRoute {
    pub fn new(endpoint: Arc<ProxyEndpoint>, identity: EgressIdentity) -> Self {
        let json = serde_json::to_vec(&identity).expect("identity serializes");
        Self {
            endpoint,
            identity_header: Arc::from(b64().encode(json)),
            identity: Arc::new(identity),
        }
    }

    pub fn identity(&self) -> &EgressIdentity {
        &self.identity
    }

    /// The three authentication headers for one request. A fresh nonce each
    /// call; `target` and `method` must be exactly what goes on the request line.
    pub fn headers(&self, kind: Kind, method: &str, target: &str) -> [(&'static str, String); 3] {
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).expect("the OS random source is available");
        let nonce = b64().encode(nonce);
        let mac = mac_for(
            &self.endpoint.secret,
            kind,
            method,
            target,
            &self.identity_header,
            &nonce,
        )
        .finalize()
        .into_bytes();
        [
            (IDENTITY_HEADER, self.identity_header.to_string()),
            (NONCE_HEADER, nonce),
            (MAC_HEADER, b64().encode(mac)),
        ]
    }
}

/// Whether a header is one of ours, to be dropped from Worker input.
pub fn is_reserved_header(name: &str) -> bool {
    name.len() >= HEADER_PREFIX.len()
        && name[..HEADER_PREFIX.len()].eq_ignore_ascii_case(HEADER_PREFIX)
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

fn tls_connector() -> &'static tokio_rustls::TlsConnector {
    static CONNECTOR: OnceLock<tokio_rustls::TlsConnector> = OnceLock::new();
    CONNECTOR.get_or_init(|| {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        tokio_rustls::TlsConnector::from(Arc::new(config))
    })
}

/// Open a connection to the proxy. The returned address is the local end of
/// the TCP connection, for `Socket.opened`.
pub async fn dial(route: &ProxyRoute) -> Result<(BoxIo, Option<String>)> {
    let endpoint = &route.endpoint;
    let tcp = tokio::time::timeout(
        PROXY_CONNECT_TIMEOUT,
        tokio::net::TcpStream::connect((endpoint.host.as_str(), endpoint.port)),
    )
    .await
    .map_err(|_| anyhow!("timed out connecting to the egress proxy"))?
    .with_context(|| {
        format!(
            "connect to the egress proxy {}:{}",
            endpoint.host, endpoint.port
        )
    })?;
    let _ = tcp.set_nodelay(true);
    let local = tcp.local_addr().ok().map(|address| address.to_string());
    if !endpoint.tls {
        return Ok((Box::new(tcp), local));
    }
    let name = rustls::pki_types::ServerName::try_from(endpoint.host.clone())
        .map_err(|_| anyhow!("invalid egress proxy TLS name {}", endpoint.host))?;
    let tls = tls_connector()
        .connect(name, tcp)
        .await
        .context("TLS handshake with the egress proxy")?;
    Ok((Box::new(tls), local))
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 255
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':' | b'[' | b']'))
}

/// `connect()` through the proxy: `CONNECT host:port`, then hand back the
/// tunnel. Bytes the proxy sent after its response head belong to the tunnel;
/// the head is read a byte at a time so none are consumed.
pub async fn connect_tunnel(
    route: &ProxyRoute,
    host: &str,
    port: u16,
) -> Result<(BoxIo, Option<String>), String> {
    if !valid_host(host) {
        return Err(format!("connect(): invalid hostname {host:?}"));
    }
    let authority = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let (mut io, local) = dial(route).await.map_err(|error| format!("{error:#}"))?;
    let mut head = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
    for (name, value) in route.headers(Kind::Connect, "CONNECT", &authority) {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    io.write_all(head.as_bytes())
        .await
        .map_err(|error| format!("write to the egress proxy failed: {error}"))?;
    io.flush()
        .await
        .map_err(|error| format!("write to the egress proxy failed: {error}"))?;
    let mut response = Vec::with_capacity(128);
    let mut byte = [0u8; 1];
    while !response.ends_with(b"\r\n\r\n") {
        if response.len() >= MAX_RESPONSE_HEAD {
            return Err("the egress proxy's CONNECT response is too large".into());
        }
        match io.read(&mut byte).await {
            Ok(0) => {
                return Err(format!(
                    "connection to {authority} failed: the egress proxy closed the connection"
                ))
            }
            Ok(_) => response.push(byte[0]),
            Err(error) => {
                return Err(format!(
                    "connection to {authority} failed: reading the egress proxy's reply: {error}"
                ))
            }
        }
    }
    let line = String::from_utf8_lossy(&response)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    let status = line
        .split_whitespace()
        .nth(1)
        .and_then(|status| status.parse::<u16>().ok());
    match status {
        Some(200..=299) => Ok((io, local)),
        _ => Err(format!(
            "connection to {authority} refused by the egress proxy: {line}"
        )),
    }
}

/// Request-line target and `Host` for a WebSocket dialed through the proxy.
/// `fetch(url, {Upgrade})` accepts the HTTP schemes as their WebSocket twins.
pub fn websocket_target(url: &url::Url) -> Result<(String, String)> {
    let scheme = match url.scheme() {
        "ws" | "http" => "ws",
        "wss" | "https" => "wss",
        other => bail!("not a WebSocket scheme: {other}"),
    };
    let authority = authority_of(url)?;
    let mut target = format!("{scheme}://{authority}{}", url.path());
    if let Some(query) = url.query() {
        target.push('?');
        target.push_str(query);
    }
    Ok((target, authority))
}

fn authority_of(url: &url::Url) -> Result<String> {
    let host = url.host_str().context("URL has no host")?;
    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

// ---------------------------------------------------------------------------
// fetch
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedirectMode {
    Follow,
    Manual,
    Error,
}

pub enum FetchBody {
    None,
    Bytes(bytes::Bytes),
    Stream(HttpChunkStream),
}

pub struct FetchRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: FetchBody,
}

pub struct FetchResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: HttpChunkStream,
}

type RequestBody = UnsyncBoxBody<bytes::Bytes, std::io::Error>;

fn header_is(name: &str, wanted: &str) -> bool {
    name.eq_ignore_ascii_case(wanted)
}

/// Send a `fetch` through the proxy, following redirects in celld. Each hop is
/// a fresh forward-proxy request with its own MAC, so a redirect never
/// reaches a destination the proxy did not see. `on_redirect` is called before
/// each followed hop and may refuse it (the subrequest budget). `timeout`
/// bounds the wait for the final response head; the body is bounded by its
/// reader.
pub async fn fetch(
    route: &ProxyRoute,
    request: FetchRequest,
    redirect: RedirectMode,
    timeout: std::time::Duration,
    on_redirect: impl FnMut() -> Result<(), String>,
) -> Result<FetchResponse, String> {
    tokio::time::timeout(timeout, fetch_hops(route, request, redirect, on_redirect))
        .await
        .map_err(|_| {
            format!(
                "fetch: the request timed out after {}s",
                timeout.as_secs()
            )
        })?
}

async fn fetch_hops(
    route: &ProxyRoute,
    request: FetchRequest,
    redirect: RedirectMode,
    mut on_redirect: impl FnMut() -> Result<(), String>,
) -> Result<FetchResponse, String> {
    let mut url = url::Url::parse(&request.url).map_err(|error| format!("fetch: {error}"))?;
    let mut method = hyper::Method::from_bytes(request.method.as_bytes())
        .map_err(|_| format!("fetch: invalid method {:?}", request.method))?;
    let mut headers = request.headers;
    let (mut bytes, mut stream) = match request.body {
        FetchBody::None => (None, None),
        FetchBody::Bytes(bytes) => (Some(bytes), None),
        FetchBody::Stream(stream) => (None, Some(stream)),
    };
    let mut stream_sent = false;
    let mut hops = 0;
    loop {
        let body = match (&bytes, stream.take()) {
            (Some(bytes), _) => Body::Bytes(bytes.clone()),
            (None, Some(stream)) => {
                stream_sent = true;
                Body::Stream(stream)
            }
            (None, None) => Body::None,
        };
        let response = send_once(route, &method, &url, &headers, body).await?;
        let status = response.status;
        if redirect == RedirectMode::Manual || !matches!(status, 301 | 302 | 303 | 307 | 308) {
            return Ok(response);
        }
        let Some(location) = response
            .headers
            .iter()
            .find(|(name, _)| header_is(name, "location"))
            .map(|(_, value)| value.clone())
        else {
            return Ok(response);
        };
        if redirect == RedirectMode::Error {
            return Err("fetch: redirect mode is error".into());
        }
        let Ok(next) = url.join(&location) else {
            return Ok(response);
        };
        if !matches!(next.scheme(), "http" | "https") {
            return Err(format!(
                "fetch: redirect to unsupported scheme {}",
                next.scheme()
            ));
        }
        let preserves_body = matches!(status, 307 | 308);
        if preserves_body && stream_sent {
            // The body went out and cannot be replayed. The redirect is the
            // answer, which is what a client that cannot follow returns.
            return Ok(response);
        }
        if hops >= MAX_REDIRECTS {
            return Err("fetch: too many redirects".into());
        }
        on_redirect()?;
        hops += 1;
        let to_get = match status {
            303 => method != hyper::Method::GET && method != hyper::Method::HEAD,
            301 | 302 => method == hyper::Method::POST,
            _ => false,
        };
        if to_get {
            method = hyper::Method::GET;
            bytes = None;
            headers.retain(|(name, _)| {
                !["content-length", "content-type", "content-encoding", "transfer-encoding"]
                    .iter()
                    .any(|framing| header_is(name, framing))
            });
        }
        let origin = |url: &url::Url| (url.scheme().to_string(), url.host_str().map(str::to_string), url.port_or_known_default());
        if origin(&next) != origin(&url) {
            headers.retain(|(name, _)| {
                !["authorization", "cookie", "proxy-authorization", "www-authenticate"]
                    .iter()
                    .any(|secret| header_is(name, secret))
            });
        }
        url = next;
        // The redirect's own body is abandoned: dropping it closes the hop.
        drop(response);
    }
}

enum Body {
    None,
    Bytes(bytes::Bytes),
    Stream(HttpChunkStream),
}

async fn send_once(
    route: &ProxyRoute,
    method: &hyper::Method,
    url: &url::Url,
    headers: &[(String, String)],
    body: Body,
) -> Result<FetchResponse, String> {
    let mut url = url.clone();
    url.set_fragment(None);
    let userinfo = (!url.username().is_empty() || url.password().is_some()).then(|| {
        format!(
            "{}:{}",
            url.username(),
            url.password().unwrap_or_default()
        )
    });
    let _ = url.set_username("");
    let _ = url.set_password(None);
    let target = url.as_str().to_string();
    let authority = authority_of(&url).map_err(|error| format!("fetch: {error:#}"))?;

    let mut builder = hyper::Request::builder()
        .method(method.clone())
        .uri(target.as_str());
    let has_framing = headers.iter().any(|(name, _)| {
        header_is(name, "content-length") || header_is(name, "transfer-encoding")
    });
    let mut has_authorization = false;
    for (name, value) in headers {
        let lower = name.to_ascii_lowercase();
        // The proxy owns `Host`; a request for another destination's headers
        // never reaches it. `Proxy-*` would be read by the proxy as addressed
        // to itself.
        if lower == "host" || lower.starts_with("proxy-") || is_reserved_header(&lower) {
            continue;
        }
        has_authorization |= lower == "authorization";
        builder = builder.header(name.as_str(), value.as_str());
    }
    builder = builder.header("host", authority);
    if let (Some(userinfo), false) = (userinfo, has_authorization) {
        builder = builder.header(
            "authorization",
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(userinfo)
            ),
        );
    }
    for (name, value) in route.headers(Kind::Fetch, method.as_str(), &target) {
        builder = builder.header(name, value);
    }
    let empty_needs_length = matches!(&body, Body::Bytes(bytes) if bytes.is_empty()) && !has_framing;
    if empty_needs_length {
        builder = builder.header("content-length", "0");
    }
    let body: RequestBody = match body {
        Body::None => http_body_util::Empty::new()
            .map_err(|never| match never {})
            .boxed_unsync(),
        Body::Bytes(bytes) => http_body_util::Full::new(bytes)
            .map_err(|never| match never {})
            .boxed_unsync(),
        Body::Stream(stream) => http_body_util::StreamBody::new(stream.map(|chunk| {
            chunk
                .map(|data| Frame::data(bytes::Bytes::from(data)))
                .map_err(std::io::Error::other)
        }))
        .boxed_unsync(),
    };
    let request = builder
        .body(body)
        .map_err(|error| format!("fetch: invalid request: {error}"))?;

    let (io, _) = dial(route)
        .await
        .map_err(|error| format!("fetch: {error:#}"))?;
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(io))
        .await
        .map_err(|error| format!("fetch: egress proxy handshake: {error}"))?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let response = sender
        .send_request(request)
        .await
        .map_err(|error| format!("fetch: egress proxy request failed: {error}"))?;
    let (parts, incoming) = response.into_parts();
    let headers = parts
        .headers
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    // The sender rides along so the connection lives exactly as long as the
    // body is being read; dropping the stream closes it, which is how an
    // abandoned response stops the transfer.
    let data = incoming.into_data_stream();
    let body = futures_util::stream::unfold((data, sender), |(mut data, sender)| async move {
        match data.next().await {
            None => None,
            Some(Ok(chunk)) => Some((Ok(chunk.to_vec()), (data, sender))),
            Some(Err(error)) => Some((Err(format!("response stream: {error}")), (data, sender))),
        }
    });
    Ok(FetchResponse {
        status: parts.status.as_u16(),
        headers,
        body: Box::pin(body),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tokio::io::{AsyncBufReadExt as _, BufReader};
    use tokio::net::TcpListener;

    const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";
    const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

    fn identity() -> EgressIdentity {
        EgressIdentity {
            v: 1,
            deployment: "v7".into(),
            script: "app".into(),
            loaded: false,
            class: Some("Counter".into()),
            cell: Some("Counter:abc".into()),
            root: None,
            egress_identity: vec![],
        }
    }

    fn route_to(port: u16) -> ProxyRoute {
        let endpoint =
            ProxyEndpoint::new(&format!("http://127.0.0.1:{port}"), SECRET.to_vec()).unwrap();
        ProxyRoute::new(Arc::new(endpoint), identity())
    }

    #[test]
    fn mac_is_bound_to_the_request() {
        let route = route_to(1);
        let headers = route.headers(Kind::Fetch, "GET", "https://example.com/a");
        let [(_, identity), (_, nonce), (_, mac)] = headers;
        let ok = |kind, method, target: &str| {
            verify_mac(SECRET, kind, method, target, &identity, &nonce, &mac)
        };
        assert!(ok(Kind::Fetch, "GET", "https://example.com/a"));
        assert!(!ok(Kind::Fetch, "GET", "https://example.com/b"));
        assert!(!ok(Kind::Fetch, "POST", "https://example.com/a"));
        assert!(!ok(Kind::WebSocket, "GET", "https://example.com/a"));
        assert!(!verify_mac(
            b"another secret of enough length",
            Kind::Fetch,
            "GET",
            "https://example.com/a",
            &identity,
            &nonce,
            &mac
        ));
        // A forged identity does not verify under the real one's MAC.
        let forged = b64().encode(br#"{"v":1}"#);
        assert!(!verify_mac(
            SECRET,
            Kind::Fetch,
            "GET",
            "https://example.com/a",
            &forged,
            &nonce,
            &mac
        ));
    }

    #[test]
    fn nonces_are_fresh() {
        let route = route_to(1);
        let a = route.headers(Kind::Connect, "CONNECT", "h:1");
        let b = route.headers(Kind::Connect, "CONNECT", "h:1");
        assert_ne!(a[1].1, b[1].1);
        assert_ne!(a[2].1, b[2].1);
    }

    #[test]
    fn reserved_headers_are_recognized() {
        assert!(is_reserved_header("X-Celld-Egress-Identity"));
        assert!(is_reserved_header("x-celld-egress-anything"));
        assert!(!is_reserved_header("x-celld-other"));
    }

    #[test]
    fn settings_validate() {
        let ok = NodeSettings {
            proxy: Some("http://127.0.0.1:9".into()),
            secret: Some("0123456789abcdef".into()),
            secret_file: None,
        };
        assert!(ok.endpoint().unwrap().is_some());
        let short = NodeSettings {
            secret: Some("short".into()),
            ..ok.clone()
        };
        assert!(short.endpoint().is_err());
        let missing = NodeSettings {
            secret: None,
            ..ok.clone()
        };
        assert!(missing.endpoint().is_err());
        let both = NodeSettings {
            secret_file: Some("/nonexistent".into()),
            ..ok.clone()
        };
        assert!(both.endpoint().is_err());
        let scheme = NodeSettings {
            proxy: Some("socks5://127.0.0.1:1".into()),
            ..ok
        };
        assert!(scheme.endpoint().is_err());
        assert!(NodeSettings::default().endpoint().unwrap().is_none());
    }

    /// One received request head: request line and header map.
    #[derive(Clone, Debug)]
    struct Seen {
        line: String,
        headers: Vec<(String, String)>,
    }

    impl Seen {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        }
    }

    async fn read_head(reader: &mut BufReader<tokio::net::TcpStream>) -> Option<Seen> {
        let mut line = String::new();
        if reader.read_line(&mut line).await.ok()? == 0 {
            return None;
        }
        let line = line.trim_end().to_string();
        let mut headers = Vec::new();
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).await.ok()?;
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            let (name, value) = header.split_once(':')?;
            headers.push((name.to_string(), value.trim().to_string()));
        }
        Some(Seen { line, headers })
    }

    /// A one-connection-at-a-time stand-in proxy. `/start` redirects to
    /// another host; everything else answers 200 with the request line.
    async fn fake_proxy() -> (u16, Arc<Mutex<Vec<Seen>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    return;
                };
                let log = log.clone();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(socket);
                    let Some(head) = read_head(&mut reader).await else {
                        return;
                    };
                    log.lock().unwrap().push(head.clone());
                    let socket = reader.get_mut();
                    if head.line.starts_with("CONNECT ") {
                        socket
                            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\nhello")
                            .await
                            .unwrap();
                        return;
                    }
                    let response = if head.line.contains("/start") {
                        "HTTP/1.1 302 Found\r\nlocation: http://other.example/final\r\ncontent-length: 0\r\n\r\n".to_string()
                    } else {
                        let body = head.line.clone();
                        format!(
                            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{body}",
                            body.len()
                        )
                    };
                    socket.write_all(response.as_bytes()).await.unwrap();
                });
            }
        });
        (port, seen)
    }

    async fn collect(mut body: HttpChunkStream) -> String {
        let mut out = Vec::new();
        while let Some(chunk) = body.next().await {
            out.extend(chunk.unwrap());
        }
        String::from_utf8(out).unwrap()
    }

    fn request(method: &str, url: &str) -> FetchRequest {
        FetchRequest {
            method: method.into(),
            url: url.into(),
            headers: vec![
                ("x-celld-egress-identity".into(), "forged".into()),
                ("authorization".into(), "Bearer t".into()),
                ("x-app".into(), "1".into()),
            ],
            body: FetchBody::None,
        }
    }

    #[tokio::test]
    async fn fetch_uses_absolute_form_and_signs_every_hop() {
        let (port, seen) = fake_proxy().await;
        let route = route_to(port);
        let mut redirects = 0;
        let response = fetch(
            &route,
            request("GET", "https://first.example/start?q=1#frag"),
            RedirectMode::Follow,
            TIMEOUT,
            || {
                redirects += 1;
                Ok(())
            },
        )
        .await
        .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(redirects, 1);
        // The redirect target was fetched through the proxy as well.
        assert_eq!(
            collect(response.body).await,
            "GET http://other.example/final HTTP/1.1"
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].line, "GET https://first.example/start?q=1 HTTP/1.1");
        assert_eq!(seen[0].header("host"), Some("first.example"));
        assert_eq!(seen[1].header("host"), Some("other.example"));
        for (hop, target) in [
            (&seen[0], "https://first.example/start?q=1"),
            (&seen[1], "http://other.example/final"),
        ] {
            assert!(verify_mac(
                SECRET,
                Kind::Fetch,
                "GET",
                target,
                hop.header(IDENTITY_HEADER).unwrap(),
                hop.header(NONCE_HEADER).unwrap(),
                hop.header(MAC_HEADER).unwrap(),
            ));
        }
        // The Worker's forged identity header was replaced, and its credential
        // header does not cross to the other origin.
        assert_ne!(seen[0].header(IDENTITY_HEADER), Some("forged"));
        assert_eq!(seen[0].header("authorization"), Some("Bearer t"));
        assert_eq!(seen[1].header("authorization"), None);
        assert_eq!(seen[1].header("x-app"), Some("1"));
    }

    #[tokio::test]
    async fn manual_and_error_redirect_modes() {
        let (port, seen) = fake_proxy().await;
        let route = route_to(port);
        let manual = fetch(
            &route,
            request("GET", "http://a.example/start"),
            RedirectMode::Manual,
            TIMEOUT,
            || panic!("manual never follows"),
        )
        .await
        .unwrap();
        assert_eq!(manual.status, 302);
        let error = fetch(
            &route,
            request("GET", "http://a.example/start"),
            RedirectMode::Error,
            TIMEOUT,
            || panic!("error never follows"),
        )
        .await;
        assert!(error.is_err());
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn redirect_budget_can_refuse() {
        let (port, _) = fake_proxy().await;
        let error = fetch(
            &route_to(port),
            request("GET", "http://a.example/start"),
            RedirectMode::Follow,
            TIMEOUT,
            || Err("Worker exceeded subrequest limit of 1".into()),
        )
        .await;
        assert_eq!(error.err().unwrap(), "Worker exceeded subrequest limit of 1");
    }

    #[tokio::test]
    async fn connect_tunnel_signs_and_keeps_early_bytes() {
        let (port, seen) = fake_proxy().await;
        let (mut io, _) = connect_tunnel(&route_to(port), "db.example", 5432)
            .await
            .unwrap();
        let mut early = [0u8; 5];
        io.read_exact(&mut early).await.unwrap();
        assert_eq!(&early, b"hello");
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].line, "CONNECT db.example:5432 HTTP/1.1");
        assert!(verify_mac(
            SECRET,
            Kind::Connect,
            "CONNECT",
            "db.example:5432",
            seen[0].header(IDENTITY_HEADER).unwrap(),
            seen[0].header(NONCE_HEADER).unwrap(),
            seen[0].header(MAC_HEADER).unwrap(),
        ));
        drop(seen);
        assert!(connect_tunnel(&route_to(port), "bad\r\nhost", 1)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn unreachable_proxy_is_an_error_not_a_direct_dial() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let error = fetch(
            &route_to(port),
            request("GET", "http://127.0.0.1:1/"),
            RedirectMode::Follow,
            TIMEOUT,
            || Ok(()),
        )
        .await;
        assert!(error.err().unwrap().contains("egress proxy"));
    }
}
