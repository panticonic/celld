//! The outbound policy hook, isolate side.
//!
//! `op_fetch`, `op_ws_upgrade`, `op_ws_connect` and `op_tcp_connect` used to
//! each read the isolate's [`EgressPolicy`] and decide for themselves. They
//! now ask [`egress_route`] one question and act on its answer:
//!
//! | policy | node proxy | answer |
//! | --- | --- | --- |
//! | `Deny` | any | refused: "no network", for a loader's `globalOutbound: null` |
//! | `Broker` | any | `fetch` goes to the Fetcher; WebSocket and `connect()` are refused, as the service protocol has no tunnel |
//! | `Allow` | none | `Direct`: the op dials the destination itself |
//! | `Allow` | set | `Proxy`: the op sends the request to the proxy with the caller's identity |
//!
//! Because the proxy decision lives here and not in each op, a new outbound
//! channel gets the policy by calling this function, and cannot forget one of
//! three copies of it. The wire format is in [`crate::egress`].

use super::*;
use crate::egress::{EgressIdentity, ProxyRoute};

/// The three outbound channels. They differ only in the refusal message
/// (kept from before the hook, since callers match on them) and in whether a
/// Fetcher can stand in for the network.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Traffic {
    Fetch,
    WebSocket,
    Connect,
}

pub(super) enum EgressRoute {
    /// Dial the destination from this process.
    Direct,
    /// Send the request to the node's egress proxy, authenticated as the
    /// caller.
    Proxy(ProxyRoute),
    /// Route the request to the Worker Loader's `globalOutbound` Fetcher.
    Broker(OutboundBroker),
}

/// What identifies an isolate to the proxy, fixed when its Worker is built.
/// The cell and class are not here: one isolate serves many cells, so they
/// come from the running event.
#[derive(Clone, Default)]
pub(super) struct EgressBase {
    pub(super) deployment: Arc<str>,
    /// The deployed script. A loaded worker inherits its loader's, so a
    /// `__loader:N` name never reaches the proxy.
    pub(super) script: Arc<str>,
    pub(super) loaded: bool,
    /// `celld: { egressIdentity }` of every loader above this isolate,
    /// outermost first.
    pub(super) chain: Arc<[String]>,
}

impl EgressBase {
    pub(super) fn deployed(script: &str) -> Self {
        Self {
            deployment: Arc::from(""),
            script: Arc::from(script),
            loaded: false,
            chain: Arc::from(Vec::new()),
        }
    }

    /// The base of a worker that `self` loads. `identity` is the loader's
    /// `celld.egressIdentity`; without one the child keeps its loader's chain,
    /// and either way it can only extend it.
    pub(super) fn loaded_child(&self, identity: Option<String>) -> Self {
        let mut chain = self.chain.to_vec();
        chain.extend(identity);
        Self {
            deployment: self.deployment.clone(),
            script: self.script.clone(),
            loaded: true,
            chain: Arc::from(chain),
        }
    }
}

/// Read `celld.egressIdentity` from a WorkerCode, refusing anything that is
/// not a short string rather than ignoring it: a loader that asked for an
/// identity must not get a Worker running under another one.
pub(super) fn loader_egress_identity(
    code: &serde_json::Value,
) -> Result<Option<String>, String> {
    let Some(celld) = code.get("celld") else {
        return Ok(None);
    };
    let Some(value) = celld.get("egressIdentity") else {
        return Ok(None);
    };
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(identity) if !identity.is_empty() && identity.len() <= 512 => {
            Ok(Some(identity.clone()))
        }
        _ => Err(
            "worker loader: `celld.egressIdentity` must be a non-empty string of at most 512 bytes"
                .to_string(),
        ),
    }
}

const DENIED_FETCH: &str = "This worker is not permitted to access the internet via global \
     functions like fetch(). It must use capabilities (such as \
     bindings in 'env') to talk to the outside world.";
const DENIED_SOCKET: &str =
    "This worker is not permitted to access the internet via global functions.";

/// Decide how this isolate's outbound request leaves, or why it may not.
pub(super) fn egress_route(
    scope: &mut v8::PinScope,
    traffic: Traffic,
) -> Result<EgressRoute, &'static str> {
    let state = actor_runtime_state(scope);
    match &state.egress {
        EgressPolicy::Deny => Err(match traffic {
            Traffic::Fetch => DENIED_FETCH,
            Traffic::WebSocket | Traffic::Connect => DENIED_SOCKET,
        }),
        // A Fetcher is still an authority boundary for sockets. celld's
        // service protocol has no bidirectional tunnel, so refusing is
        // required: a direct dial here would silently bypass the broker.
        EgressPolicy::Broker(broker) => match traffic {
            Traffic::Fetch => Ok(EgressRoute::Broker(broker.clone())),
            Traffic::WebSocket => Err("A globalOutbound Fetcher cannot broker WebSockets in celld."),
            Traffic::Connect => Err("A globalOutbound Fetcher cannot broker connect() in celld."),
        },
        EgressPolicy::Allow => {
            let Some(endpoint) = crate::egress::node_proxy() else {
                return Ok(EgressRoute::Direct);
            };
            // The identity comes from the isolate and the running event, never
            // from anything the Worker passed to the op.
            let context = event_context(scope);
            let (cell, root) = match current_cell_event(&context) {
                Some(frame) => (Some(frame.storage), frame.root.map(|root| root.cell)),
                None => (None, None),
            };
            let class = cell
                .as_deref()
                .and_then(|cell| cell.split_once(':'))
                .map(|(class, _)| class.to_string());
            let base = &state.egress_base;
            Ok(EgressRoute::Proxy(ProxyRoute::new(
                endpoint,
                EgressIdentity {
                    v: 1,
                    deployment: base.deployment.to_string(),
                    script: base.script.to_string(),
                    loaded: base.loaded,
                    class,
                    cell,
                    root,
                    egress_identity: base.chain.to_vec(),
                },
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loaded_worker_can_only_extend_its_loaders_identity() {
        let deployed = EgressBase::deployed("app");
        assert!(!deployed.loaded);
        assert!(deployed.chain.is_empty());

        let tenant = deployed.loaded_child(Some("tenant-a".into()));
        assert!(tenant.loaded);
        assert_eq!(&*tenant.script, "app");
        assert_eq!(&*tenant.chain, ["tenant-a".to_string()]);

        // Silence inherits; naming extends. There is no way to drop or replace
        // the prefix a loader established.
        let silent = tenant.loaded_child(None);
        assert_eq!(&*silent.chain, ["tenant-a".to_string()]);
        let nested = tenant.loaded_child(Some("agent-7".into()));
        assert_eq!(
            &*nested.chain,
            ["tenant-a".to_string(), "agent-7".to_string()]
        );
    }

    #[test]
    fn loader_identity_is_validated_not_ignored() {
        let code = |value: serde_json::Value| {
            serde_json::json!({ "celld": { "egressIdentity": value } })
        };
        assert_eq!(loader_egress_identity(&serde_json::json!({})), Ok(None));
        assert_eq!(
            loader_egress_identity(&code(serde_json::json!("t"))),
            Ok(Some("t".to_string()))
        );
        assert_eq!(
            loader_egress_identity(&code(serde_json::Value::Null)),
            Ok(None)
        );
        for bad in [
            serde_json::json!(""),
            serde_json::json!(7),
            serde_json::json!({ "a": 1 }),
            serde_json::json!("x".repeat(513)),
        ] {
            assert!(loader_egress_identity(&code(bad)).is_err());
        }
    }
}
