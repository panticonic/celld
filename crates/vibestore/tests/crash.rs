#![allow(clippy::disallowed_methods)]

//! Crash safety and the Vibestudio GC contract.
//!
//! A simulated crash stops a write at a named point and runs no cleanup, which
//! is what a `kill -9` there leaves behind. The "restart" is a fresh `Core`
//! over the same files. At every point: the index never names a missing blob,
//! no live blob is ever unretained, and recovery needs nothing beyond orphan
//! cleanup.

mod common;

use celld_vibestore::{CrashPoint, VibeStore};
use common::*;
use object_store::{Error, ObjectStore, PutMode, UpdateVersion};
use std::time::Duration;

const NO_GRACE: Duration = Duration::ZERO;

async fn crashing_put(store: &VibeStore, point: CrashPoint, key: &str, tag: u8) -> Error {
    store.core().set_crash_point(Some(point));
    let error = store
        .put(&path(key), payload(tag, 100).into())
        .await
        .unwrap_err();
    store.core().set_crash_point(None);
    assert!(error.to_string().contains("simulated crash"), "{error}");
    error
}

#[tokio::test]
async fn a_crash_between_the_blob_and_the_index_leaves_only_an_orphan() {
    let env = Env::new();
    let store = env.open();
    crashing_put(&store, CrashPoint::AfterBlobInstalled, "k", 1).await;

    // The key was never created: the index is the only commit point.
    assert!(matches!(
        store.get(&path("k")).await.unwrap_err(),
        Error::NotFound { .. }
    ));
    assert_eq!(store.core().index().count().unwrap(), 0);
    // What the crash left is exactly one unreferenced blob.
    assert_eq!(blob_digests(&env.root()).len(), 1);
    assert!(retention_owners(&env.root()).is_empty());

    // Restart: nothing to repair. The orphan is unretained, so the host GC
    // collects it (and could have at any time).
    let recovery = env.open().core().recover().unwrap();
    assert_eq!(recovery.retentions_released, 0);
    assert_eq!(vibestudio_gc(&env.root(), NO_GRACE).len(), 1);
    assert!(blob_digests(&env.root()).is_empty());
}

#[tokio::test]
async fn a_crash_after_retaining_but_before_the_index_leaves_an_orphan_retention() {
    let env = Env::new();
    let store = env.open();
    crashing_put(&store, CrashPoint::AfterRetained, "k", 1).await;
    assert!(store.get(&path("k")).await.is_err());
    assert_eq!(retention_owners(&env.root()).len(), 1, "the row outlives the crash");
    // While the row exists the host GC leaves the blob alone: safe, if wasteful.
    assert!(vibestudio_gc(&env.root(), NO_GRACE).is_empty());

    // Reopening reconciles the row away, and then the blob is collectable.
    let recovered = env.open();
    assert!(retention_owners(&env.root()).is_empty());
    assert_eq!(vibestudio_gc(&env.root(), NO_GRACE).len(), 1);
    assert!(recovered.get(&path("k")).await.is_err());
}

#[tokio::test]
async fn a_crash_after_the_index_commit_keeps_the_new_value_and_both_blobs_until_recovery() {
    let env = Env::new();
    let store = env.open();
    put(&store, "k", payload(1, 100)).await;
    crashing_put(&store, CrashPoint::AfterIndexCommit, "k", 2).await;

    // The commit happened: the new value is what a reader sees.
    assert_eq!(get(&store, "k").await, payload(2, 100));
    // Both digests are retained, so a GC now removes nothing live.
    assert_eq!(retention_owners(&env.root()).len(), 2);
    assert!(vibestudio_gc(&env.root(), NO_GRACE).is_empty());
    assert_eq!(get(&store, "k").await, payload(2, 100));

    // Recovery releases the superseded digest; only then can it be collected.
    let recovered = env.open();
    assert_eq!(retention_owners(&env.root()).len(), 1);
    let collected = vibestudio_gc(&env.root(), NO_GRACE);
    assert_eq!(collected.len(), 1);
    assert_eq!(get(&recovered, "k").await, payload(2, 100));
}

#[tokio::test]
async fn a_crash_after_a_delete_commits_leaves_a_stale_retention_only() {
    let env = Env::new();
    let store = env.open();
    put(&store, "k", payload(1, 100)).await;
    store.core().set_crash_point(Some(CrashPoint::AfterIndexCommit));
    assert!(store.delete(&path("k")).await.is_err());
    store.core().set_crash_point(None);
    assert!(store.get(&path("k")).await.is_err(), "the delete committed");
    assert!(vibestudio_gc(&env.root(), NO_GRACE).is_empty(), "still retained");
    env.open();
    assert_eq!(vibestudio_gc(&env.root(), NO_GRACE).len(), 1);
}

#[tokio::test]
async fn a_rejected_conditional_write_takes_no_retention() {
    let env = Env::new();
    let store = env.open();
    let first = put(&store, "k", payload(1, 100)).await;
    put(&store, "k", payload(2, 100)).await;
    let stale = PutMode::Update(UpdateVersion {
        e_tag: Some(first),
        version: None,
    });
    assert!(store
        .put_opts(&path("k"), payload(3, 100).into(), stale.into())
        .await
        .is_err());
    // The precondition is checked before any retention row is taken, so the
    // loser leaves only an unretained orphan blob.
    let owners = retention_owners(&env.root());
    assert_eq!(owners.len(), 1);
    assert_eq!(blob_digests(&env.root()).len(), 3);
    assert_eq!(vibestudio_gc(&env.root(), NO_GRACE).len(), 2);
    assert_eq!(get(&store, "k").await, payload(2, 100));
}

#[tokio::test]
async fn rewriting_the_same_bytes_keeps_the_blob_retained() {
    let env = Env::new();
    let store = env.open();
    put(&store, "k", payload(1, 100)).await;
    put(&store, "k", payload(1, 100)).await;
    assert_eq!(retention_owners(&env.root()).len(), 1);
    assert!(vibestudio_gc(&env.root(), NO_GRACE).is_empty());
    assert_eq!(get(&store, "k").await, payload(1, 100));
}

#[tokio::test]
async fn the_host_gc_never_collects_a_live_object_and_collects_a_deleted_one() {
    let env = Env::new();
    let store = env.open();
    for n in 0..10u8 {
        put(&store, &format!("live/{n}"), payload(n, 100)).await;
    }
    put(&store, "doomed", payload(99, 100)).await;
    store.delete(&path("doomed")).await.unwrap();
    // A superseded value, too.
    put(&store, "rewritten", payload(50, 100)).await;
    put(&store, "rewritten", payload(51, 100)).await;

    let collected = vibestudio_gc(&env.root(), NO_GRACE);
    assert_eq!(collected.len(), 2, "the deleted blob and the superseded one");
    for n in 0..10u8 {
        assert_eq!(get(&store, &format!("live/{n}")).await, payload(n, 100));
    }
    assert_eq!(get(&store, "rewritten").await, payload(51, 100));
    assert_eq!(env.open().core().verify().unwrap(), Vec::<String>::new());
}

#[tokio::test]
async fn a_store_that_lost_its_retentions_rebuilds_them_from_the_index_on_open() {
    // Someone deleted retentions.db (or Vibestudio recreated it): the index is
    // the source of truth, so opening re-establishes every root.
    let env = Env::new();
    {
        let store = env.open();
        for n in 0..5u8 {
            put(&store, &format!("k{n}"), payload(n, 100)).await;
        }
    }
    std::fs::remove_file(env.root().join("retentions.db")).unwrap();
    let store = env.open();
    assert_eq!(retention_owners(&env.root()).len(), 5);
    assert!(vibestudio_gc(&env.root(), NO_GRACE).is_empty());
    assert_eq!(get(&store, "k3").await, payload(3, 100));
}

#[tokio::test]
async fn two_celld_namespaces_do_not_release_each_others_rows() {
    let env = Env::new();
    let a = env.open_with(|spec| {
        spec.namespace_id = "fleet-a".into();
        spec.index = env.dir.path().join("a.sqlite");
    });
    let b = env.open_with(|spec| {
        spec.namespace_id = "fleet-b".into();
        spec.index = env.dir.path().join("b.sqlite");
    });
    put(&a, "k", payload(1, 100)).await;
    put(&b, "k", payload(1, 100)).await;
    a.delete(&path("k")).await.unwrap();
    // b still needs the shared blob.
    assert!(vibestudio_gc(&env.root(), NO_GRACE).is_empty());
    assert_eq!(get(&b, "k").await, payload(1, 100));
    drop(a);
    // Reopening a must not drop b's row.
    let _a = env.open_with(|spec| {
        spec.namespace_id = "fleet-a".into();
        spec.index = env.dir.path().join("a.sqlite");
    });
    assert!(vibestudio_gc(&env.root(), NO_GRACE).is_empty());
}

#[tokio::test]
async fn the_age_grace_protects_a_blob_written_just_before_it_is_retained() {
    // The residual race with a GC that runs in another process: it can snapshot
    // the retention set between our blob write and our retention row. Its
    // minAge is what covers that interval.
    let env = Env::new();
    let store = env.open();
    store.core().set_crash_point(Some(CrashPoint::AfterBlobInstalled));
    let _ = store.put(&path("k"), payload(1, 100).into()).await;
    store.core().set_crash_point(None);
    assert!(
        vibestudio_gc(&env.root(), Duration::from_secs(60)).is_empty(),
        "a fresh unretained blob survives a GC with minAge"
    );
    assert_eq!(vibestudio_gc(&env.root(), NO_GRACE).len(), 1);
}

#[tokio::test]
async fn small_objects_never_touch_the_blob_root() {
    let env = Env::new();
    let store = env.open();
    for n in 0..20u8 {
        put(&store, &format!("lease/{n}"), payload(n, 4)).await;
        put(&store, &format!("lease/{n}"), payload(n + 1, 4)).await;
    }
    assert!(blob_digests(&env.root()).is_empty());
    assert!(retention_owners(&env.root()).is_empty());
}
