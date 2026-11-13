// Copyright 2026 the celld fork authors. Apache-2.0 license.

//! The V8 surface of whole-object fork and restore (`env.NS.fork`,
//! `env.NS.restore`; `crate::object_image`). The harness calls these from
//! the internal requests it answers before an object's instance exists
//! (`__celld/object-export`, `__celld/object-adopt`).
use super::*;

fn throw(scope: &mut v8::PinScope, message: &str) {
    let message = v8::String::new(scope, message).unwrap();
    let exception = v8::Exception::error(scope, message);
    scope.throw_exception(exception);
}

/// Send one request to the host loop, which owns the facet streams.
async fn host<T>(
    build: impl FnOnce(tokio::sync::oneshot::Sender<anyhow::Result<T>>) -> FacetReq,
) -> Result<T, String> {
    let sender = FACET_TX
        .get()
        .ok_or_else(|| "object fork and restore need a node with replication".to_string())?;
    let (reply, receive) = tokio::sync::oneshot::channel();
    sender
        .send(build(reply))
        .map_err(|_| "the facet channel closed".to_string())?;
    receive
        .await
        .map_err(|_| "the host dropped the request".to_string())?
        .map_err(|error| format!("{error:#}"))
}

/// `__object_export(cell, target, optionsJson)` — publish a copy of the
/// object this turn runs in, with its facets, as the new object `target`.
///
/// The root is read now, in the source's turn, outside any transaction.
/// The facets are read after it, each parent before its children, and each
/// facet's stream is proved durable past its read
/// (`CellRuntime::export_facets`). The publication waits behind the
/// source's output gate, which covers the root read, so no component of the
/// fork holds state the source could still lose. The target is claimed with
/// a conditional create (`object_image::publish_new`), so a fork never
/// overwrites an object. Resolves to `{ bytes, components }`.
pub(super) fn op_object_export(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let cell = args.get(0).to_rust_string_lossy(scope);
    let target = args.get(1).to_rust_string_lossy(scope);
    let options = args.get(2).to_rust_string_lossy(scope);
    let prepared = (|| -> Result<_, String> {
        let class = cell.split_once(':').map(|(class, _)| class).unwrap_or("");
        let same_class = target
            .split_once(':')
            .is_some_and(|(target_class, _)| target_class == class);
        if !same_class || !celld_logic::cell::valid_cell_scope(&target) || target == cell {
            return Err(format!("invalid fork target {target:?}"));
        }
        let options: serde_json::Value = if options.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(&options).map_err(|error| format!("fork options: {error}"))?
        };
        let at = options
            .get("at")
            .and_then(|at| at.as_str())
            .map(str::to_string);
        // A restore's export: the copy is a staging object for `staging`,
        // which only that object's restore reads and then deletes.
        let staging_for = options
            .get("staging")
            .and_then(|staging| staging.as_str())
            .map(str::to_string);
        if let Some(restored) = &staging_for {
            let same = restored
                .split_once(':')
                .is_some_and(|(restored_class, _)| restored_class == class);
            if !same || !celld_logic::cell::valid_cell_scope(restored) {
                return Err(format!("invalid restore target {restored:?}"));
            }
        }
        let root = storage::root_file(&cell)?;
        let (engine, image) = storage::export_root(&cell)?;
        if at.is_some() && engine == celld_ltx::Engine::Sqlite {
            return Err(
                "fork options.at names a Dolt commit, but this object's root is a \
                        SQLite database, which has no history: a fork copies its newest \
                        committed state (give a Dolt facet's commit under options.facets)"
                    .to_string(),
            );
        }
        let scratch = crate::object_image::scratch_beside(&root.path, "export-root");
        let (image, index) = crate::object_image::rewrite(engine, &image, &scratch, |c| {
            // Before the reset: a reset to an older commit can drop the rules
            // that name the local tables.
            crate::object_image::strip_identity(c, &target)?;
            if let Some(at) = &at {
                crate::dolt_cell::reset_hard(c, at)?;
            }
            crate::object_image::read_index(c)
        })
        .map_err(|error| format!("{error:#}"))?;
        tracing::info!(
            cell = %cell,
            target = %target,
            bytes = image.len(),
            facets = index.len(),
            "fork export: root read in its turn"
        );
        let root = crate::object_image::Component {
            path: Vec::new(),
            engine,
            image,
        };
        Ok((
            root,
            index,
            options,
            staging_for,
            storage::root_file(&cell)?.epoch,
        ))
    })();
    let gate = egress_gate_request(&event_context(scope), celld_logic::Channel::R2);
    let id = asyncrt::enqueue(async move {
        let (root, index, options, staging_for, epoch) = prepared?;
        let mut image = vec![root];
        if !index.is_empty() {
            let (root_scope, target_scope) = (cell.clone(), target.clone());
            image.extend(
                host(move |reply| FacetReq::Export {
                    root: root_scope,
                    epoch,
                    index,
                    options,
                    target: target_scope,
                    reply,
                })
                .await
                .map_err(|error| format!("fork: {error}"))?,
            );
        }
        // The root's output gate: every write the root image holds is
        // durable before anything is published.
        await_egress_gate(gate).await?;
        tracing::info!(cell = %cell, target = %target, "fork export: root proved; publishing");
        let bucket = crate::r2_store::store()
            .map_err(|_| "fork needs a fleet bucket (celld dev provides one)".to_string())?;
        let bytes = crate::object_image::publish_new(
            bucket,
            &target,
            &image,
            crate::object_image::Existing::Refuse,
            staging_for.as_deref(),
        )
        .await
        .map_err(|error| format!("{error:#}"))?;
        Ok(serde_json::json!({ "bytes": bytes, "components": image.len() }).to_string())
    });
    rv.set(promise_for(scope, id));
}

/// `__object_quiesced(cell, closed)` — why the root `cell` is not quiesced
/// as far as the host can see (`""` when it is): a WebSocket attached to it
/// (unless `closed`: a stop just closed them, and their close is under way),
/// or a facet running or being started. The harness checks the rest (an
/// instance, a residency hold, other events).
pub(super) fn op_object_quiesced(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let cell = args.get(0).to_rust_string_lossy(scope);
    let closed = args.get(1).boolean_value(scope);
    let reason = (|| {
        let sockets = crate::ws_registry::ws_registry()
            .lock()
            .unwrap()
            .metadata
            .values()
            .filter(|meta| meta.scope == cell)
            .count();
        if sockets > 0 && !closed {
            return format!("it has {sockets} open WebSocket(s)");
        }
        let epoch = storage::activation_epoch(&cell).unwrap_or(0);
        if let Some(entry) = root_facets().get(&(cell.clone(), epoch)) {
            if !entry.open.is_empty() {
                return "a facet of it is running".to_string();
            }
            if entry.pending > 0 {
                return "a call to one of its facets is in flight".to_string();
            }
        }
        if !storage::embedded_facets(&cell).is_empty() {
            return "a facet of it is running in its own isolate (stop it with \
                    ctx.facets.abort())"
                .to_string();
        }
        String::new()
    })();
    rv.set(v8::String::new(scope, &reason).unwrap().into());
}

/// `__object_restore_begin(cell, staging)` — record the restore's intent in
/// the root (`storage::restore_begin`).
pub(super) fn op_object_restore_begin(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    _rv: v8::ReturnValue<v8::Value>,
) {
    let cell = args.get(0).to_rust_string_lossy(scope);
    let staging = args.get(1).to_rust_string_lossy(scope);
    if let Err(error) = storage::restore_begin(&cell, &staging) {
        throw(scope, &format!("restore: {error:#}"));
    }
}

/// `__object_restore_pending(cell)` — the staging object of an unfinished
/// restore of the root `cell`, or `""`.
pub(super) fn op_object_restore_pending(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let cell = args.get(0).to_rust_string_lossy(scope);
    let pending = storage::restore_pending(&cell).unwrap_or_default();
    rv.set(v8::String::new(scope, &pending).unwrap().into());
}

/// `__object_adopt_streams(cell, staging)` — the host's half of a restore
/// (`CellRuntime::adopt_streams`). Resolves to `{ path, engine }`: the file
/// holding the root's image.
pub(super) fn op_object_adopt_streams(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let cell = args.get(0).to_rust_string_lossy(scope);
    let staging = args.get(1).to_rust_string_lossy(scope);
    let epoch = storage::root_file(&cell).map(|root| root.epoch);
    let id = asyncrt::enqueue(async move {
        let epoch = epoch?;
        let bucket = crate::r2_store::store()
            .map_err(|_| "restore needs a fleet bucket (celld dev provides one)".to_string())?;
        crate::object_image::check_staging(bucket, &staging, &cell)
            .await
            .map_err(|error| format!("restore: {error:#}"))?;
        let (path, engine) = host(move |reply| FacetReq::Adopt {
            root: cell,
            epoch,
            staging,
            reply,
        })
        .await
        .map_err(|error| format!("restore: {error}"))?;
        Ok(serde_json::json!({
            "path": path.to_string_lossy(),
            "engine": crate::object_image::engine_name(engine),
        })
        .to_string())
    });
    rv.set(promise_for(scope, id));
}

/// `__object_adopt_root(cell, path, engine)` — install the root image the
/// host left at `path` (`storage::adopt_root`), in the target's turn.
pub(super) fn op_object_adopt_root(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    _rv: v8::ReturnValue<v8::Value>,
) {
    let cell = args.get(0).to_rust_string_lossy(scope);
    let path = std::path::PathBuf::from(args.get(1).to_rust_string_lossy(scope));
    let engine = args.get(2).to_rust_string_lossy(scope);
    // The black-box crash point of a restore: its facets are replaced and
    // its root is not. The next activation must roll the restore forward.
    #[cfg(debug_assertions)]
    if std::env::var("CELLD_TEST_RESTORE_CRASH").as_deref() == Ok("1") {
        eprintln!("celld test: injected crash between a restore's streams and its root");
        std::process::exit(86);
    }
    let result = (|| -> anyhow::Result<()> {
        let engine = crate::object_image::parse_engine(&engine)?;
        let image = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        storage::adopt_root(&cell, image, engine)?;
        let _ = std::fs::remove_file(&path);
        Ok(())
    })();
    match result {
        Ok(()) => tracing::info!(cell = %cell, "restore: root installed"),
        Err(error) => throw(scope, &format!("restore: {error:#}")),
    }
}

/// `__object_prove(cell)` — prove every committed write of the root
/// `cell`'s own stream durable.
pub(super) fn op_object_prove(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let cell = args.get(0).to_rust_string_lossy(scope);
    let epoch = storage::root_file(&cell).map(|root| root.epoch);
    let id = asyncrt::enqueue(async move {
        let epoch = epoch?;
        prove_facet(cell, epoch)
            .await
            .map_err(|refusal| format!("restore: {refusal}"))?;
        Ok(String::new())
    });
    rv.set(promise_for(scope, id));
}

/// `__object_discard(cell, staging)` — delete the staging object of a
/// restore of the root `cell` once nothing needs it: the restore finished
/// (no intent is left) or never began. Only a staging object made for
/// `cell` qualifies (`object_image::check_staging`).
pub(super) fn op_object_discard(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let cell = args.get(0).to_rust_string_lossy(scope);
    let staging = args.get(1).to_rust_string_lossy(scope);
    let settled = match storage::root_file(&cell) {
        Err(error) => Err(error),
        Ok(_) if storage::restore_pending(&cell).as_deref() == Some(staging.as_str()) => {
            Err("the restore that reads this staging object has not finished".to_string())
        }
        Ok(_) => Ok(()),
    };
    let id = asyncrt::enqueue(async move {
        settled?;
        if !celld_logic::cell::valid_cell_scope(&staging) || !staging.contains(':') {
            return Err(format!("invalid staging object {staging:?}"));
        }
        let bucket = crate::r2_store::store().map_err(|error| error.to_string())?;
        crate::object_image::check_staging(bucket, &staging, &cell)
            .await
            .map_err(|error| format!("{error:#}"))?;
        crate::object_image::discard(bucket, &staging)
            .await
            .map_err(|error| format!("{error:#}"))?;
        Ok(String::new())
    });
    rv.set(promise_for(scope, id));
}

/// `__object_stop(cell)` — the host's half of a restore's `stop`: close the
/// root's WebSockets (1012) and the facets it runs in loaded Workers'
/// isolates, as a generation swap does, so the object is quiesced. A facet
/// in the root's own isolate (a `ctx.exports` class) is not closed here; the
/// quiescence check then refuses the restore.
pub(super) fn op_object_stop(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let cell = args.get(0).to_rust_string_lossy(scope);
    let epoch = storage::root_file(&cell).map(|root| root.epoch);
    let id = asyncrt::enqueue(async move {
        let epoch = epoch?;
        crate::ws_registry::ws_close_scope(&cell, 1012, "the Durable Object was restored");
        release_root_facets(&cell, epoch, RootRelease::Swap).await;
        tracing::info!(cell = %cell, "restore: stopped the running target");
        Ok(String::new())
    });
    rv.set(promise_for(scope, id));
}
