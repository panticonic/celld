//! Host ops for `ctx.residency.hold()`.
//!
//! A hold asks the core to keep a cell's runtime in memory until the holder
//! releases it. The decision is the core's (`celld_logic::State`): this file
//! only carries the question out of the isolate and the answer back. Refusal
//! arrives as a rejected promise at `hold()` time, never later.
//!
//! The ops authorize twice and trust neither the harness nor the application:
//! a loaded Worker must have been granted `celld: { residency }` by its loader
//! (checked here), and the root cell's class must have declared `residency` in
//! the deployment (checked in the host loop, which owns the generation).

use super::*;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

/// Hold ids are unique node-wide, so the core can tell a stale handle of a
/// discarded runtime from a live hold of its successor by id alone.
static NEXT_HOLD: AtomicU64 = AtomicU64::new(1);

/// `__residency_hold(scope) -> Promise<"{hold, cell}">`.
///
/// `scope` is the calling object's scope; a facet's hold attaches to its root
/// cell, which is the thing the node keeps resident.
pub(super) fn op_residency_hold(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let caller = args.get(0).to_rust_string_lossy(scope);
    if actor_runtime_state(scope).residency_grant == Some(false) {
        rv.set(rejected_promise(
            scope,
            "ERR_RESIDENCY_NOT_GRANTED: this Worker Loader did not grant \
             `celld: { residency: true }` to the worker it loaded, so its objects \
             cannot hold residency",
        ));
        return;
    }
    let identity = match storage::storage_identity(&caller) {
        Ok(Some(identity)) => identity,
        _ => {
            rv.set(rejected_promise(
                scope,
                "ERR_RESIDENCY_NO_CELL: ctx.residency.hold() needs a running Durable Object; \
                 this scope has no storage",
            ));
            return;
        }
    };
    let Some(channel) = HOLD_TX.get() else {
        rv.set(rejected_promise(
            scope,
            "ERR_RESIDENCY_UNAVAILABLE: this process runs no cell host, so there is nothing \
             to hold a cell resident",
        ));
        return;
    };
    let hold = NEXT_HOLD.fetch_add(1, Ordering::Relaxed);
    let cell = identity.root_scope;
    let (reply, receive) = tokio::sync::oneshot::channel();
    let sent = channel
        .send(HoldReq::Acquire {
            cell: cell.clone(),
            hold,
            epoch: identity.epoch,
            reply,
        })
        .is_ok();
    let id = asyncrt::enqueue(async move {
        if !sent {
            return Err("ERR_RESIDENCY_UNAVAILABLE: the cell host stopped".to_string());
        }
        match receive.await {
            Ok(Ok(())) => {
                Ok::<String, String>(serde_json::json!({ "hold": hold, "cell": cell }).to_string())
            }
            Ok(Err(denied)) => Err(format!("{}: {}", denied.code, denied.message)),
            Err(_) => Err("ERR_RESIDENCY_UNAVAILABLE: the cell host stopped".to_string()),
        }
    });
    rv.set(promise_for(scope, id));
}

/// `__residency_release(cell, hold)`: fire and forget, idempotent.
pub(super) fn op_residency_release(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    _rv: v8::ReturnValue<v8::Value>,
) {
    let cell = args.get(0).to_rust_string_lossy(scope);
    let hold = args.get(1).number_value(scope).unwrap_or(0.0) as u64;
    if let Some(channel) = HOLD_TX.get() {
        let _ = channel.send(HoldReq::Release { cell, hold });
    }
}
