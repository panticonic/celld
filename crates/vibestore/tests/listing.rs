#![allow(clippy::disallowed_methods)]

//! Listing semantics, checked against `object_store`'s reference in-memory
//! store (which mirrors S3's path-segment prefixes) and against a transcription
//! of the delimiter/continuation rules `LocalStore` implements for
//! `PaginatedListStore`.

mod common;

use common::*;
use futures_util::TryStreamExt as _;
use object_store::list::{PaginatedListOptions, PaginatedListStore};
use object_store::memory::InMemory;
use object_store::path::Path;
use object_store::ObjectStore;

/// Unicode, a literal `%`, an `@`, an empty-segment encoding, numeric order
/// traps and keys that share prefixes without sharing a path segment.
fn keys() -> Vec<String> {
    let mut keys: Vec<String> = [
        "a",
        "a/b",
        "a/b/c",
        "a/b/d/e",
        "a/bb",
        "a/b-c",
        "a/b.c",
        "ab",
        "b/c/d",
        "cells/Counter:abc/ltx/e1/0000000000000001.ltx",
        "cells/Counter:abc/ltx/e1/0000000000000002.ltx",
        "cells/Counter:abd/ltx/e1/0000000000000001.ltx",
        "cells/Other:x/ltx/e2/0000000000000001.ltx",
        "cells/x@y/f",
        "deploy/current.json",
        "nodes/n1",
        "nodes/n2",
        "r2/bucket/%/empty",
        "z",
    ]
    .iter()
    .map(|k| k.to_string())
    .collect();
    // Percent-encoded forms, as `Path::from` makes them.
    keys.push(Path::from("přehled.html").to_string());
    keys.push(Path::from("dir/ünï cödé/x y").to_string());
    keys
}

async fn populate(store: &dyn ObjectStore) {
    for key in keys() {
        // Spread across inline and blob storage.
        let body = payload(key.len() as u8, if key.len() % 2 == 0 { 3 } else { 40 });
        store
            .put(&Path::parse(&key).unwrap(), body.into())
            .await
            .unwrap();
    }
}

fn prefixes() -> Vec<Option<Path>> {
    vec![
        None,
        Some(Path::default()),
        Some(Path::from("a")),
        Some(Path::from("a/b")),
        Some(Path::from("a/b/d")),
        Some(Path::from("b")),
        Some(Path::from("cells")),
        Some(Path::from("cells/Counter:abc")),
        Some(Path::from("nodes")),
        Some(Path::from("nope")),
        Some(Path::from("a/b/c")),
    ]
}

fn summarize(metas: &[object_store::ObjectMeta]) -> Vec<(String, u64)> {
    metas
        .iter()
        .map(|meta| (meta.location.to_string(), meta.size))
        .collect()
}

#[tokio::test]
async fn list_matches_the_reference_store() {
    let env = Env::new();
    let store = env.open();
    let reference = InMemory::new();
    populate(&store).await;
    populate(&reference).await;
    for prefix in prefixes() {
        let ours: Vec<_> = store.list(prefix.as_ref()).try_collect().await.unwrap();
        let theirs: Vec<_> = reference.list(prefix.as_ref()).try_collect().await.unwrap();
        assert_eq!(summarize(&ours), summarize(&theirs), "list {prefix:?}");
    }
}

#[tokio::test]
async fn list_with_delimiter_matches_the_reference_store() {
    let env = Env::new();
    let store = env.open();
    let reference = InMemory::new();
    populate(&store).await;
    populate(&reference).await;
    for prefix in prefixes() {
        let ours = store.list_with_delimiter(prefix.as_ref()).await.unwrap();
        let theirs = reference.list_with_delimiter(prefix.as_ref()).await.unwrap();
        assert_eq!(ours.common_prefixes, theirs.common_prefixes, "prefixes of {prefix:?}");
        assert_eq!(
            summarize(&ours.objects),
            summarize(&theirs.objects),
            "objects of {prefix:?}"
        );
    }
}

#[tokio::test]
async fn list_with_offset_matches_the_reference_store() {
    let env = Env::new();
    let store = env.open();
    let reference = InMemory::new();
    populate(&store).await;
    populate(&reference).await;
    let offsets = [
        Path::from("a/b"),
        Path::from("a/b/c"),
        Path::from("cells/Counter:abc/ltx/e1/0000000000000001.ltx"),
        Path::from("zzz"),
        Path::from("0"),
    ];
    for prefix in prefixes() {
        for offset in &offsets {
            let ours: Vec<_> = store
                .list_with_offset(prefix.as_ref(), offset)
                .try_collect()
                .await
                .unwrap();
            let theirs: Vec<_> = reference
                .list_with_offset(prefix.as_ref(), offset)
                .try_collect()
                .await
                .unwrap();
            assert_eq!(
                summarize(&ours),
                summarize(&theirs),
                "list {prefix:?} after {offset:?}"
            );
        }
    }
}

#[tokio::test]
async fn a_listing_larger_than_one_internal_page_is_complete_and_ordered() {
    let env = Env::new();
    let store = env.open();
    let mut expected = Vec::new();
    for n in 0..1300u32 {
        let key = format!("seg/{n:08}");
        store
            .put(&Path::from(key.as_str()), payload(1, 2).into())
            .await
            .unwrap();
        expected.push(key);
    }
    let listed: Vec<String> = store
        .list(Some(&Path::from("seg")))
        .map_ok(|meta| meta.location.to_string())
        .try_collect()
        .await
        .unwrap();
    assert_eq!(listed, expected);
    let delimited = store
        .list_with_delimiter(Some(&Path::from("seg")))
        .await
        .unwrap();
    assert_eq!(delimited.objects.len(), 1300);
    let after: Vec<String> = store
        .list_with_offset(Some(&Path::from("seg")), &Path::from("seg/00000600"))
        .map_ok(|meta| meta.location.to_string())
        .try_collect()
        .await
        .unwrap();
    assert_eq!(after, expected[601..]);
}

#[tokio::test]
async fn a_delimiter_listing_skips_a_prefix_with_many_children() {
    let env = Env::new();
    let store = env.open();
    for n in 0..1000u32 {
        store
            .put(&Path::from(format!("big/{n:05}/f")), payload(1, 2).into())
            .await
            .unwrap();
    }
    store
        .put(&Path::from("big-sibling"), payload(1, 2).into())
        .await
        .unwrap();
    let listing = store.list_with_delimiter(None).await.unwrap();
    assert_eq!(listing.common_prefixes, vec![Path::from("big")]);
    assert_eq!(listing.objects.len(), 1);
    let big = store.list_with_delimiter(Some(&Path::from("big"))).await.unwrap();
    assert_eq!(big.common_prefixes.len(), 1000);
}

// ---- PaginatedListStore ---------------------------------------------------

/// The page `LocalStore::list_paginated` returns, transcribed over a sorted
/// key list: the reference the SQL implementation must agree with.
fn model(
    all: &[String],
    prefix: &str,
    after: Option<&str>,
    delimiter: Option<&str>,
    limit: usize,
) -> (Vec<String>, Vec<String>, Option<String>) {
    // (last key, common prefix, object)
    let mut entries: Vec<(String, Option<String>, Option<String>)> = Vec::new();
    for key in all {
        let Some(remainder) = key.strip_prefix(prefix) else {
            continue;
        };
        if after.is_some_and(|after| key.as_str() <= after) {
            continue;
        }
        let common = delimiter.and_then(|d| {
            remainder
                .find(d)
                .map(|index| format!("{prefix}{}{d}", &remainder[..index]))
        });
        if let Some(common) = common {
            if let Some((last, Some(previous), _)) = entries.last_mut() {
                if *previous == common {
                    *last = key.clone();
                    continue;
                }
            }
            entries.push((key.clone(), Some(common), None));
        } else {
            entries.push((key.clone(), None, Some(key.clone())));
        }
    }
    let truncated = entries.len() > limit;
    entries.truncate(limit);
    let token = truncated
        .then(|| entries.last().map(|(last, _, _)| last.clone()))
        .flatten();
    let mut objects = Vec::new();
    let mut prefixes = Vec::new();
    for (_, common, object) in entries {
        if let Some(common) = common {
            prefixes.push(common.trim_end_matches('/').to_string());
        }
        if let Some(object) = object {
            objects.push(object);
        }
    }
    (objects, prefixes, token)
}

async fn page(
    store: &dyn PaginatedListStore,
    prefix: &str,
    after: Option<&str>,
    token: Option<String>,
    delimiter: Option<&'static str>,
    limit: usize,
) -> (Vec<String>, Vec<String>, Option<String>) {
    let result = store
        .list_paginated(
            Some(prefix),
            PaginatedListOptions {
                offset: after.map(str::to_string),
                delimiter: delimiter.map(Into::into),
                max_keys: Some(limit),
                page_token: token,
                ..PaginatedListOptions::default()
            },
        )
        .await
        .unwrap();
    (
        result
            .result
            .objects
            .iter()
            .map(|meta| meta.location.to_string())
            .collect(),
        result
            .result
            .common_prefixes
            .iter()
            .map(|p| p.to_string())
            .collect(),
        result.page_token,
    )
}

#[tokio::test]
async fn paginated_listing_agrees_with_the_localstore_rules_page_by_page() {
    let env = Env::new();
    let store = env.open();
    populate(&store).await;
    let mut all = keys();
    all.sort();
    let prefix_cases = ["", "a", "a/", "a/b", "a/b/", "cells/", "cells/Counter:", "nodes/", "none"];
    for prefix in prefix_cases {
        for delimiter in [None, Some("/")] {
            for limit in [1usize, 2, 3, 5, 100] {
                // Walk the whole listing by token and compare every page.
                let mut token: Option<String> = None;
                let mut guard = 0;
                loop {
                    let ours = page(&store, prefix, None, token.clone(), delimiter, limit).await;
                    let theirs = model(&all, prefix, token.as_deref(), delimiter, limit);
                    assert_eq!(
                        ours, theirs,
                        "prefix {prefix:?} delimiter {delimiter:?} limit {limit} token {token:?}"
                    );
                    match ours.2 {
                        Some(next) => token = Some(next),
                        None => break,
                    }
                    guard += 1;
                    assert!(guard < 200, "listing did not terminate");
                }
            }
        }
    }
}

#[tokio::test]
async fn paginated_offset_resumes_after_a_named_child_like_start_after() {
    let env = Env::new();
    let store = env.open();
    populate(&store).await;
    let mut all = keys();
    all.sort();
    // `common_prefixes_page` resumes by naming a child as `<child>/`.
    for offset in ["a/", "a/b/", "cells/Counter:abc/", "nodes/", "zzz/"] {
        let ours = page(&store, "", Some(offset), None, Some("/"), 4).await;
        let theirs = model(&all, "", Some(offset), Some("/"), 4);
        assert_eq!(ours, theirs, "offset {offset:?}");
    }
    // A cell-class filtered walk uses a raw string prefix `cells/Counter:`.
    let ours = page(&store, "cells/Counter:", None, None, Some("/"), 10).await;
    assert_eq!(
        ours.1,
        vec!["cells/Counter:abc".to_string(), "cells/Counter:abd".to_string()]
    );
}

#[tokio::test]
async fn paginated_listing_without_a_limit_returns_everything_untruncated() {
    let env = Env::new();
    let store = env.open();
    populate(&store).await;
    let result = store
        .list_paginated(Some(""), PaginatedListOptions::default())
        .await
        .unwrap();
    assert_eq!(result.result.objects.len(), keys().len());
    assert!(result.page_token.is_none());
}

#[tokio::test]
async fn a_rolled_up_prefix_counts_once_toward_the_page_limit() {
    let env = Env::new();
    let store = env.open();
    for n in 0..50 {
        store
            .put(&Path::from(format!("cells/c1/ltx/{n:04}")), payload(1, 2).into())
            .await
            .unwrap();
    }
    store.put(&Path::from("cells/c2/x"), payload(1, 2).into()).await.unwrap();
    store.put(&Path::from("cells/c3/x"), payload(1, 2).into()).await.unwrap();
    let first = page(&store, "cells/", None, None, Some("/"), 2).await;
    assert_eq!(first.1, vec!["cells/c1".to_string(), "cells/c2".to_string()]);
    assert!(first.0.is_empty());
    let token = first.2.expect("c3 remains");
    let second = page(&store, "cells/", None, Some(token), Some("/"), 2).await;
    assert_eq!(second.1, vec!["cells/c3".to_string()]);
    assert!(second.2.is_none());
}
