//! Host ops for the `p2p` binding: `env.<P2P>.connect()` and the stream an
//! inbound peer opens into a Durable Object.
//!
//! The endpoint, the connection pool and the stream registry live in
//! `crate::p2p`; this module is the V8 edge.
//!
//! A Durable Object's stream (one it dialed, or one a peer opened into its
//! `onp2pstream`) belongs to the cell: it is adopted into the core as an
//! outbound transport, pins the cell like an outbound WebSocket, and lives
//! until it is closed, finishes in both directions, or the cell leaves. A
//! Worker has no cell, so its stream stays with its request:
//! `IoContext::close_sockets` frees it at retirement, as it does a `connect()`
//! TCP socket. Writes and the dial wait on the output gate on the same
//! `Channel::Tcp` route a raw socket uses: both are bytes leaving over a
//! connection the application opened, and a write must not be visible to the
//! peer before the storage writes it follows are durable. A cell-owned stream
//! is written to from many events, so each write waits on the gate of the
//! event that performs it, and a write that no event frames (a callback after
//! its event retired) waits for everything the cell has committed so far.

use super::*;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectArgs {
    alpn: String,
    node_id: String,
    #[serde(default)]
    addrs: Vec<String>,
    #[serde(default)]
    relay: Option<String>,
    /// The Durable Object scope calling `connect()`, empty for a Worker.
    #[serde(default)]
    scope: String,
}

/// The cell and the object a scope's streams belong to, or none for a Worker
/// (and for any scope without storage, which has no cell to pin).
fn stream_owner(scope: &str) -> Option<(String, String)> {
    if scope.is_empty() {
        return None;
    }
    let identity = crate::storage::storage_identity(scope).ok().flatten()?;
    Some((identity.root_scope, scope.to_string()))
}

fn return_string(scope: &mut v8::PinScope, rv: &mut v8::ReturnValue<v8::Value>, value: &str) {
    let value = v8::String::new(scope, value).unwrap();
    rv.set(value.into());
}

/// `__p2p_node_id() -> string`: this node's id, or "" before the endpoint is up.
pub(super) fn op_p2p_node_id(
    scope: &mut v8::PinScope,
    _args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let id = crate::p2p::node_id().unwrap_or_default();
    return_string(scope, &mut rv, &id);
}

/// `__p2p_connect(json) -> Promise<json>`: dial a peer and open one stream.
pub(super) fn op_p2p_connect(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    // Dialing out is egress like any other: a worker that may not reach the
    // network may not reach a peer, and a brokered worker has no tunnel to
    // carry a QUIC connection through.
    match &actor_runtime_state(scope).egress {
        EgressPolicy::Allow => {}
        EgressPolicy::Deny => {
            return loader_throw(
                scope,
                "This worker is not permitted to access the internet via global functions.",
            );
        }
        EgressPolicy::Broker(_) => {
            return loader_throw(
                scope,
                "A globalOutbound Fetcher cannot broker a p2p connect() in celld.",
            );
        }
    }
    let raw = args.get(0).to_rust_string_lossy(scope);
    let request: ConnectArgs = match serde_json::from_str(&raw) {
        Ok(request) => request,
        Err(error) => return loader_throw(scope, &format!("connect(): {error}")),
    };
    let dial = match crate::p2p::Dial::parse(
        &request.alpn,
        &request.node_id,
        &request.addrs,
        request.relay.as_deref(),
    ) {
        Ok(dial) => dial,
        Err(error) => return loader_throw(scope, &format!("{error}")),
    };
    let context = current_context();
    let owner = stream_owner(&request.scope);
    let gate = stream_gate(scope, owner.as_ref());
    let async_id = asyncrt::enqueue(async move {
        await_egress_gate(gate).await?;
        let (id, remote) = crate::p2p::connect(dial)
            .await
            .map_err(|error| format!("{error:#}"))?;
        match owner {
            // A Durable Object's stream is the cell's. The core decides
            // whether the cell may hold it, as it does for an outbound
            // WebSocket, and a refusal fails the dial.
            Some((cell, scope)) => {
                if let Err(error) = crate::p2p::adopt(id, &cell, &scope).await {
                    crate::p2p::close(id, Some(crate::p2p::CODE_BUSY));
                    return Err(format!("{error:#}"));
                }
            }
            // The registration and the ownership record happen in one poll
            // of this operation, which the owning event drives, so a
            // retirement cannot fall between them and strand the stream.
            None => context.tcp_sockets.lock().unwrap().push(id),
        }
        Ok::<String, String>(serde_json::json!({ "id": id, "remoteNodeId": remote }).to_string())
    });
    rv.set(promise_for(scope, async_id));
}

/// `__p2p_claim(token, scope) -> string`: redeem an inbound stream's claim
/// token for the cell it was minted for. "" when the token is not valid.
pub(super) fn op_p2p_claim(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let token = args.get(0).to_rust_string_lossy(scope);
    let cell = args.get(1).to_rust_string_lossy(scope);
    let answer = match crate::p2p::claim(&token, &cell) {
        Some(claimed) => serde_json::to_string(&claimed).unwrap_or_default(),
        None => String::new(),
    };
    return_string(scope, &mut rv, &answer);
}

/// `__p2p_adopt(id, scope) -> Promise<undefined>`: make a claimed inbound
/// stream the accepting cell's. Rejects when the core will not pin the cell
/// for another transport; the stream is then reset with `BUSY` and the handler
/// never runs.
pub(super) fn op_p2p_adopt(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let id = stream_id(scope, args.get(0));
    let object = args.get(1).to_rust_string_lossy(scope);
    let Some((cell, object)) = stream_owner(&object) else {
        rv.set(rejected_promise(
            scope,
            "a p2p stream can only be accepted by a Durable Object with storage",
        ));
        return;
    };
    let async_id = asyncrt::enqueue(async move {
        if let Err(error) = crate::p2p::adopt(id, &cell, &object).await {
            crate::p2p::close(id, Some(crate::p2p::CODE_BUSY));
            return Err(format!("{error:#}"));
        }
        Ok::<String, String>(String::new())
    });
    rv.set(promise_for(scope, async_id));
}

/// `__p2p_close_object(scope)`: the object's instance is gone (an abort, a
/// failed critical section), so the streams it held end with the cell-ended
/// code. A fresh instance in the same isolate has none of them.
pub(super) fn op_p2p_close_object(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    _rv: v8::ReturnValue<v8::Value>,
) {
    let object = args.get(0).to_rust_string_lossy(scope);
    crate::p2p::close_object(&object);
}

/// The output gate for a write or a dial. An event that frames the call
/// answers for it. A cell-owned stream is also written from callbacks that
/// outlive their event, which no frame covers, and an unframed effect would
/// leave unchecked: they wait for everything the cell has committed so far.
fn stream_gate(scope: &mut v8::PinScope, owner: Option<&(String, String)>) -> EgressGate {
    let context = event_context(scope);
    let gate = egress_gate_request(&context, celld_logic::Channel::Tcp);
    match (gate, owner) {
        (EgressGate::NoCell, Some((cell, object))) if object == cell => {
            match storage::write_position(cell) {
                Ok(Some(position)) => EgressGate::Wrote(
                    cell.clone(),
                    celld_logic::Channel::Tcp,
                    position,
                    storage::activation_epoch(cell),
                ),
                Ok(None) => EgressGate::NoCell,
                Err(error) => EgressGate::Unpersisted(error.to_string()),
            }
        }
        (gate, _) => gate,
    }
}

fn stream_id(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> u64 {
    value.integer_value(scope).unwrap_or(0) as u64
}

pub(super) fn op_p2p_read(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let id = stream_id(scope, args.get(0));
    let stream = crate::p2p::stream(id);
    let async_id = asyncrt::enqueue(async move { stream.ok_or("stream is closed")?.read().await });
    rv.set(promise_for(scope, async_id));
}

pub(super) fn op_p2p_write(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let id = stream_id(scope, args.get(0));
    let Some(bytes) = view_bytes(args.get(1)) else {
        return loader_throw(scope, "p2p stream write needs bytes");
    };
    let stream = crate::p2p::stream(id);
    let owner = stream.as_ref().and_then(|stream| stream.owner());
    let gate = stream_gate(scope, owner.as_ref());
    let async_id = asyncrt::enqueue(async move {
        await_egress_gate(gate).await?;
        stream.ok_or("stream is closed")?.write(&bytes).await?;
        Ok::<String, String>(String::new())
    });
    rv.set(promise_for(scope, async_id));
}

/// `__p2p_shutdown(id)`: finish the send half. The peer's reads end after what
/// was written; the receive half stays open.
pub(super) fn op_p2p_shutdown(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let id = stream_id(scope, args.get(0));
    let stream = crate::p2p::stream(id);
    let owner = stream.as_ref().and_then(|stream| stream.owner());
    let gate = stream_gate(scope, owner.as_ref());
    let async_id = asyncrt::enqueue(async move {
        await_egress_gate(gate).await?;
        stream.ok_or("stream is closed")?.shutdown().await?;
        Ok::<String, String>(String::new())
    });
    rv.set(promise_for(scope, async_id));
}

/// `__p2p_close(id, code)`: close both halves. A negative `code` is an orderly
/// close; anything else resets the stream with that application error code.
pub(super) fn op_p2p_close(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    _rv: v8::ReturnValue<v8::Value>,
) {
    let id = stream_id(scope, args.get(0));
    let code = args.get(1).number_value(scope).unwrap_or(-1.0);
    let code = (code >= 0.0 && code <= u32::MAX as f64).then_some(code as u32);
    crate::p2p::close(id, code);
}

/// Free a stream the event owned, at retirement.
pub(super) fn free(id: u64) {
    if crate::p2p::is_stream_id(id) {
        crate::p2p::close(id, None);
    }
}
