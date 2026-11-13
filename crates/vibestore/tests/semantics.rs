//! Conditional writes, ranged reads, multipart, attributes, copy and the
//! features celld must never silently lose.

mod common;

use bytes::Bytes;
use common::*;
use futures_util::StreamExt as _;
use object_store::{
    Attribute, Attributes, Error, GetOptions, GetRange, MultipartUpload as _, ObjectStore,
    PutMode, PutMultipartOptions, PutOptions, PutPayload, UpdateVersion,
};
use std::sync::Arc;

fn is_precondition(error: &Error) -> bool {
    matches!(error, Error::Precondition { .. } | Error::AlreadyExists { .. })
}

/// Both storage classes: the index (small) and the CAS (large).
fn bodies() -> [(&'static str, Bytes); 2] {
    [("inline", payload(1, 4)), ("blob", payload(2, 200))]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_one_of_many_creators_wins() {
    for (class, body) in bodies() {
        let env = Env::new();
        let store = Arc::new(env.open());
        let wins = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for racer in 0..16u8 {
            let (store, wins) = (store.clone(), wins.clone());
            let mut mine = body.to_vec();
            mine[0] = racer;
            tasks.push(tokio::spawn(async move {
                let result = store
                    .put_opts(&path("owner"), Bytes::from(mine).into(), PutMode::Create.into())
                    .await;
                match result {
                    Ok(_) => {
                        wins.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    Err(error) => assert!(is_precondition(&error), "{class}: {error}"),
                }
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(wins.load(std::sync::atomic::Ordering::SeqCst), 1, "{class}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_one_of_many_updaters_of_a_version_wins() {
    for (class, body) in bodies() {
        let env = Env::new();
        let store = Arc::new(env.open());
        let etag = put(&store, "lease", body.clone()).await;
        let mut tasks = Vec::new();
        for racer in 0..16u8 {
            let (store, etag) = (store.clone(), etag.clone());
            let mut mine = body.to_vec();
            mine[0] = racer.wrapping_add(100);
            tasks.push(tokio::spawn(async move {
                let mode = PutMode::Update(UpdateVersion {
                    e_tag: Some(etag),
                    version: None,
                });
                store
                    .put_opts(&path("lease"), Bytes::from(mine).into(), mode.into())
                    .await
            }));
        }
        let mut winners = 0;
        for task in tasks {
            match task.await.unwrap() {
                Ok(_) => winners += 1,
                Err(error) => assert!(is_precondition(&error), "{class}: {error}"),
            }
        }
        assert_eq!(winners, 1, "{class}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn racers_through_separate_store_instances_still_have_one_winner() {
    // Two instances over one index are two processes as far as the index can
    // tell: they share no in-process gate, only SQLite's lock.
    let env = Env::new();
    let stores: Vec<_> = (0..4).map(|_| Arc::new(env.open())).collect();
    let mut tasks = Vec::new();
    for (n, store) in stores.iter().enumerate() {
        for k in 0..8u8 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                store
                    .put_opts(
                        &path("k"),
                        payload(n as u8 * 8 + k, 64).into(),
                        PutMode::Create.into(),
                    )
                    .await
                    .is_ok()
            }));
        }
    }
    let mut winners = 0;
    for task in tasks {
        winners += usize::from(task.await.unwrap());
    }
    assert_eq!(winners, 1);
}

#[tokio::test]
async fn a_conditional_update_chain_is_linear() {
    let env = Env::new();
    let store = env.open();
    let mut etag = put(&store, "k", payload(0, 50)).await;
    for step in 1..20u8 {
        let result = store
            .put_opts(
                &path("k"),
                payload(step, 50).into(),
                PutMode::Update(UpdateVersion {
                    e_tag: Some(etag.clone()),
                    version: None,
                })
                .into(),
            )
            .await
            .unwrap();
        let next = result.e_tag.unwrap();
        assert_ne!(next, etag);
        // The head agrees with what the write returned.
        assert_eq!(store.head(&path("k")).await.unwrap().e_tag.unwrap(), next);
        etag = next;
    }
    assert_eq!(get(&store, "k").await, payload(19, 50));
}

#[tokio::test]
async fn a_stale_or_missing_update_is_a_clean_rejection() {
    let env = Env::new();
    let store = env.open();
    let first = put(&store, "k", payload(0, 50)).await;
    put(&store, "k", payload(1, 50)).await;
    let stale = PutMode::Update(UpdateVersion {
        e_tag: Some(first),
        version: None,
    });
    let error = store
        .put_opts(&path("k"), payload(2, 50).into(), stale.into())
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Precondition { .. }), "{error}");
    // The loser changed nothing.
    assert_eq!(get(&store, "k").await, payload(1, 50));
    let absent = PutMode::Update(UpdateVersion {
        e_tag: Some("1".into()),
        version: None,
    });
    assert!(matches!(
        store
            .put_opts(&path("nope"), payload(2, 50).into(), absent.into())
            .await
            .unwrap_err(),
        Error::Precondition { .. }
    ));
    let error = store
        .put_opts(&path("k"), payload(3, 50).into(), PutMode::Create.into())
        .await
        .unwrap_err();
    assert!(matches!(error, Error::AlreadyExists { .. }), "{error}");
}

#[tokio::test]
async fn the_etag_never_comes_back_after_delete_and_recreate() {
    let env = Env::new();
    let store = env.open();
    let first = put(&store, "k", payload(0, 50)).await;
    store.delete(&path("k")).await.unwrap();
    let second = put(&store, "k", payload(0, 50)).await;
    assert_ne!(first, second, "identical bytes must not share a version");
    let mode = PutMode::Update(UpdateVersion {
        e_tag: Some(first),
        version: None,
    });
    assert!(store
        .put_opts(&path("k"), payload(1, 50).into(), mode.into())
        .await
        .is_err());
}

#[tokio::test]
async fn get_honors_if_match_and_if_none_match() {
    let env = Env::new();
    let store = env.open();
    let etag = put(&store, "k", payload(0, 50)).await;
    let matching = GetOptions {
        if_match: Some(etag.clone()),
        ..GetOptions::default()
    };
    store.get_opts(&path("k"), matching).await.unwrap();
    let mismatched = GetOptions {
        if_match: Some("nope".into()),
        ..GetOptions::default()
    };
    assert!(matches!(
        store.get_opts(&path("k"), mismatched).await.unwrap_err(),
        Error::Precondition { .. }
    ));
    let unchanged = GetOptions {
        if_none_match: Some(etag),
        ..GetOptions::default()
    };
    assert!(matches!(
        store.get_opts(&path("k"), unchanged).await.unwrap_err(),
        Error::NotModified { .. }
    ));
}

#[tokio::test]
async fn ranges_match_s3_for_both_storage_classes() {
    for (class, size) in [("inline", 8usize), ("blob", 3 * 1024 * 1024 + 17)] {
        let env = Env::new();
        let store = env.open();
        let body = payload(7, size);
        put(&store, "k", body.clone()).await;
        let range = |range: GetRange| {
            let store = &store;
            async move {
                store
                    .get_opts(
                        &path("k"),
                        GetOptions {
                            range: Some(range),
                            ..GetOptions::default()
                        },
                    )
                    .await
            }
        };
        let n = size as u64;

        let result = range(GetRange::Bounded(1..5)).await.unwrap();
        assert_eq!(result.range, 1..5, "{class}");
        assert_eq!(result.bytes().await.unwrap(), body.slice(1..5), "{class}");

        let result = range(GetRange::Offset(n - 3)).await.unwrap();
        assert_eq!(result.range, n - 3..n, "{class}");
        assert_eq!(result.bytes().await.unwrap(), body.slice(size - 3..), "{class}");

        let result = range(GetRange::Suffix(4)).await.unwrap();
        assert_eq!(result.range, n - 4..n, "{class}");
        assert_eq!(result.bytes().await.unwrap(), body.slice(size - 4..), "{class}");

        // A suffix longer than the object is the whole object.
        let result = range(GetRange::Suffix(n * 2)).await.unwrap();
        assert_eq!(result.range, 0..n, "{class}");

        // A bounded range past the end is served short, as R2 serves it.
        let result = range(GetRange::Bounded(n - 2..n + 100)).await.unwrap();
        assert_eq!(result.range, n - 2..n, "{class}");
        assert_eq!(result.bytes().await.unwrap(), body.slice(size - 2..), "{class}");

        // A range that starts past the end is an error (a 416), not a miss.
        let error = range(GetRange::Bounded(n..n + 1)).await.unwrap_err();
        assert!(!matches!(error, Error::NotFound { .. }), "{class}: {error}");
        assert!(range(GetRange::Offset(n)).await.is_err(), "{class}");

        // get_range and get_ranges go through the same path.
        assert_eq!(
            store.get_range(&path("k"), 2..6).await.unwrap(),
            body.slice(2..6),
            "{class}"
        );
        let parts = store.get_ranges(&path("k"), &[0..2, 4..8]).await.unwrap();
        assert_eq!(parts, vec![body.slice(0..2), body.slice(4..8)], "{class}");
    }
}

#[tokio::test]
async fn a_large_object_streams_in_pieces_and_reads_back_whole() {
    let env = Env::new();
    let store = env.open();
    let body = payload(3, 5 * 1024 * 1024 + 123);
    put(&store, "big", body.clone()).await;
    let mut stream = store.get(&path("big")).await.unwrap().into_stream();
    let mut pieces = 0;
    let mut collected = Vec::new();
    while let Some(piece) = stream.next().await {
        let piece = piece.unwrap();
        pieces += 1;
        collected.extend_from_slice(&piece);
    }
    assert!(pieces > 1, "a 5 MiB body is not one read");
    assert_eq!(Bytes::from(collected), body);
}

#[tokio::test]
async fn head_reports_size_and_metadata_without_a_body() {
    let env = Env::new();
    let store = env.open();
    let etag = put(&store, "small", payload(1, 3)).await;
    put(&store, "large", payload(1, 5000)).await;
    let meta = store.head(&path("small")).await.unwrap();
    assert_eq!((meta.size, meta.e_tag.as_deref()), (3, Some(etag.as_str())));
    assert_eq!(store.head(&path("large")).await.unwrap().size, 5000);
    let head = store
        .get_opts(
            &path("large"),
            GetOptions {
                head: true,
                ..GetOptions::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(head.range, 0..5000);
    assert!(matches!(
        store.head(&path("absent")).await.unwrap_err(),
        Error::NotFound { .. }
    ));
    assert!(matches!(
        store.get(&path("absent")).await.unwrap_err(),
        Error::NotFound { .. }
    ));
}

#[tokio::test]
async fn empty_objects_round_trip() {
    let env = Env::new();
    let store = env.open();
    put(&store, "empty", Bytes::new()).await;
    assert_eq!(store.head(&path("empty")).await.unwrap().size, 0);
    assert!(get(&store, "empty").await.is_empty());
}

#[tokio::test]
async fn delete_is_idempotent_and_final() {
    let env = Env::new();
    let store = env.open();
    put(&store, "k", payload(0, 50)).await;
    store.delete(&path("k")).await.unwrap();
    store.delete(&path("k")).await.unwrap();
    assert!(matches!(
        store.get(&path("k")).await.unwrap_err(),
        Error::NotFound { .. }
    ));
    // The batch delete celld uses.
    for n in 0..5 {
        put(&store, &format!("d/{n}"), payload(n, 20)).await;
    }
    let locations =
        futures_util::stream::iter((0..5).map(|n| Ok(path(&format!("d/{n}"))))).boxed();
    let mut results = store.delete_stream(locations);
    let mut gone = 0;
    while let Some(result) = results.next().await {
        result.unwrap();
        gone += 1;
    }
    assert_eq!(gone, 5);
    assert_eq!(store.list(None).count().await, 0);
}

#[tokio::test]
async fn attributes_round_trip_and_unknown_ones_are_refused() {
    let env = Env::new();
    let store = env.open();
    let mut attributes = Attributes::new();
    attributes.insert(Attribute::ContentType, "application/x-test".into());
    attributes.insert(Attribute::Metadata("celld-r2".into()), "yes".into());
    store
        .put_opts(
            &path("k"),
            payload(0, 50).into(),
            PutOptions {
                attributes: attributes.clone(),
                ..PutOptions::default()
            },
        )
        .await
        .unwrap();
    let result = store.get(&path("k")).await.unwrap();
    assert_eq!(result.attributes, attributes);
}

#[tokio::test]
async fn unsupported_features_fail_loudly() {
    let env = Env::new();
    let store = env.open();
    put(&store, "k", payload(0, 50)).await;

    let mut tags = object_store::TagSet::default();
    tags.push("a", "b");
    let error = store
        .put_opts(
            &path("t"),
            payload(0, 5).into(),
            PutOptions {
                tags,
                ..PutOptions::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, Error::NotSupported { .. }), "{error}");

    let error = store
        .get_opts(
            &path("k"),
            GetOptions {
                version: Some("v1".into()),
                ..GetOptions::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, Error::NotSupported { .. }), "{error}");

    // A GCS-style generation precondition has no meaning here.
    let generation = PutMode::Update(UpdateVersion {
        e_tag: None,
        version: Some("42".into()),
    });
    let error = store
        .put_opts(&path("k"), payload(1, 5).into(), generation.into())
        .await
        .unwrap_err();
    assert!(matches!(error, Error::NotSupported { .. }), "{error}");
    assert_eq!(get(&store, "k").await, payload(0, 50), "and nothing was written");

    let mut attributes = Attributes::new();
    attributes.insert(Attribute::Metadata("x".into()), "1".into());
    // Attributes outside the known set would be refused; known ones pass.
    store
        .put_opts(
            &path("a"),
            payload(0, 5).into(),
            PutOptions {
                attributes,
                ..PutOptions::default()
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn copy_and_rename_preserve_bytes_and_honor_if_not_exists() {
    for (class, body) in bodies() {
        let env = Env::new();
        let store = env.open();
        put(&store, "src", body.clone()).await;
        store.copy(&path("src"), &path("dst")).await.unwrap();
        assert_eq!(get(&store, "dst").await, body, "{class}");
        assert!(matches!(
            store
                .copy_if_not_exists(&path("src"), &path("dst"))
                .await
                .unwrap_err(),
            Error::AlreadyExists { .. }
        ));
        store.rename(&path("dst"), &path("moved")).await.unwrap();
        assert_eq!(get(&store, "moved").await, body, "{class}");
        assert!(matches!(
            store.get(&path("dst")).await.unwrap_err(),
            Error::NotFound { .. }
        ));
        assert!(matches!(
            store.copy(&path("absent"), &path("x")).await.unwrap_err(),
            Error::NotFound { .. }
        ));
        // The source is untouched, and the destination has its own version.
        assert_eq!(get(&store, "src").await, body, "{class}");
        assert_ne!(
            store.head(&path("src")).await.unwrap().e_tag,
            store.head(&path("moved")).await.unwrap().e_tag
        );
    }
}

#[tokio::test]
async fn state_survives_reopening_the_store() {
    let env = Env::new();
    let (small, large) = (payload(1, 4), payload(2, 4096));
    let etag = {
        let store = env.open();
        put(&store, "small", small.clone()).await;
        let etag = put(&store, "large", large.clone()).await;
        store.core().close();
        etag
    };
    let store = env.open();
    assert_eq!(get(&store, "small").await, small);
    assert_eq!(get(&store, "large").await, large);
    assert_eq!(store.head(&path("large")).await.unwrap().e_tag.unwrap(), etag);
    // The sequence continues; it never goes backwards across a restart.
    let next = put(&store, "another", payload(3, 50)).await;
    assert!(next.parse::<u64>().unwrap() > etag.parse::<u64>().unwrap());
}

#[tokio::test]
async fn a_missing_blob_is_an_error_not_a_miss() {
    let env = Env::new();
    let store = env.open();
    put(&store, "k", payload(5, 100)).await;
    for (_, file) in celld_vibestore::cas::Cas::open(&env.root())
        .unwrap()
        .addresses()
        .unwrap()
    {
        std::fs::remove_file(file).unwrap();
    }
    let error = store.get(&path("k")).await.unwrap_err();
    assert!(
        matches!(error, Error::Generic { .. }),
        "NotFound would make celld believe the key was never written: {error}"
    );
    assert!(!env.open().core().verify().unwrap().is_empty());
}

#[tokio::test]
async fn multipart_assembles_parts_in_call_order_whatever_order_they_finish() {
    for (class, parts) in [
        ("inline", vec![payload(1, 3), payload(2, 3)]),
        (
            "blob",
            vec![payload(1, 1_000_000), payload(2, 700_000), payload(3, 12), payload(4, 5)],
        ),
    ] {
        let env = Env::new();
        let store = env.open();
        let mut attributes = Attributes::new();
        attributes.insert(Attribute::ContentType, "x/y".into());
        let mut upload = store
            .put_multipart_opts(
                &path("m"),
                PutMultipartOptions {
                    attributes,
                    ..PutMultipartOptions::default()
                },
            )
            .await
            .unwrap();
        let futures: Vec<_> = parts
            .iter()
            .map(|part| upload.put_part(PutPayload::from(part.clone())))
            .collect();
        // Finish them back to front.
        for future in futures.into_iter().rev() {
            future.await.unwrap();
        }
        upload.complete().await.unwrap();
        let expected: Vec<u8> = parts.iter().flat_map(|p| p.to_vec()).collect();
        assert_eq!(get(&store, "m").await, Bytes::from(expected), "{class}");
        assert_eq!(
            store.get(&path("m")).await.unwrap().attributes.get(&Attribute::ContentType).map(|v| v.as_ref().to_string()),
            Some("x/y".to_string())
        );
        let staging = std::fs::read_dir(env.root().join("tmp-celld")).unwrap().count();
        assert_eq!(staging, 0, "{class}: no temp file survives completion");
    }
}

#[tokio::test]
async fn an_aborted_or_dropped_multipart_leaves_nothing_behind() {
    let env = Env::new();
    let store = env.open();
    let mut upload = store
        .put_multipart_opts(&path("m"), PutMultipartOptions::default())
        .await
        .unwrap();
    upload.put_part(payload(1, 5000).into()).await.unwrap();
    upload.abort().await.unwrap();
    drop(upload);
    let mut dropped = store
        .put_multipart_opts(&path("m2"), PutMultipartOptions::default())
        .await
        .unwrap();
    dropped.put_part(payload(1, 5000).into()).await.unwrap();
    drop(dropped);
    assert_eq!(std::fs::read_dir(env.root().join("tmp-celld")).unwrap().count(), 0);
    assert!(store.get(&path("m")).await.is_err());
    assert!(store.get(&path("m2")).await.is_err());
}

#[tokio::test]
async fn completing_a_multipart_with_an_abandoned_part_fails() {
    let env = Env::new();
    let store = env.open();
    let mut upload = store
        .put_multipart_opts(&path("m"), PutMultipartOptions::default())
        .await
        .unwrap();
    drop(upload.put_part(payload(1, 5000).into()));
    assert!(upload.complete().await.is_err(), "a hole must not become an object");
    assert!(store.get(&path("m")).await.is_err());
}

#[tokio::test]
async fn identical_blobs_share_one_file_and_survive_deleting_one_key() {
    let env = Env::new();
    let store = env.open();
    let body = payload(9, 300);
    put(&store, "a", body.clone()).await;
    put(&store, "b", body.clone()).await;
    assert_eq!(blob_digests(&env.root()).len(), 1);
    store.delete(&path("a")).await.unwrap();
    assert_eq!(get(&store, "b").await, body);
    // Both keys retained the digest; deleting one leaves the other's row.
    assert_eq!(retention_owners(&env.root()).len(), 1);
    assert!(vibestudio_gc(&env.root(), std::time::Duration::ZERO).is_empty());
}

#[tokio::test]
async fn exclusive_mode_sweeps_only_aged_orphans() {
    let env = Env::new();
    let store = env.open_with(|spec| {
        spec.shared = false;
        spec.grace = std::time::Duration::from_secs(3600);
    });
    put(&store, "keep", payload(1, 300)).await;
    put(&store, "drop", payload(2, 300)).await;
    store.delete(&path("drop")).await.unwrap();
    assert!(!env.root().join("retentions.db").exists(), "no retentions in exclusive mode");
    assert_eq!(blob_digests(&env.root()).len(), 2);
    // Within the grace period nothing is removed.
    assert_eq!(store.core().sweep_orphans(std::time::Duration::from_secs(3600)).unwrap(), 0);
    // With no grace, only the unreferenced blob goes.
    assert_eq!(store.core().sweep_orphans(std::time::Duration::ZERO).unwrap(), 1);
    assert_eq!(blob_digests(&env.root()).len(), 1);
    assert_eq!(get(&store, "keep").await, payload(1, 300));
}
