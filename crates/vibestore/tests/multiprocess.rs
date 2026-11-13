#![allow(clippy::disallowed_methods)]

//! Conditional writes across real OS processes: the `celld dev` supervisor and
//! its node are separate processes that share one store.

mod common;

use common::*;
use object_store::{ObjectStore, PutMode, UpdateVersion};
use std::process::Command;

const WORKER_ENV: &str = "VIBESTORE_TEST_WORKER_DIR";
const KEYS: usize = 25;

/// Re-executed by the parent test as a child process; a no-op otherwise.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worker_entry() {
    let Ok(dir) = std::env::var(WORKER_ENV) else {
        return;
    };
    let worker: u8 = std::env::var("VIBESTORE_TEST_WORKER_ID").unwrap().parse().unwrap();
    // Open the shared store, not a private one.
    let mut spec = celld_vibestore::VibeSpec::new(std::path::PathBuf::from(&dir).join("blobs"));
    spec.index = std::path::PathBuf::from(&dir).join("index.sqlite");
    spec.inline_max = INLINE_MAX;
    spec.namespace_id = "test".into();
    let store = celld_vibestore::VibeStore::open(&spec, celld_vibestore::tokio_blocker()).unwrap();
    let mut won = Vec::new();
    for key in 0..KEYS {
        let body = payload(worker, if key % 2 == 0 { 4 } else { 200 });
        let created = store
            .put_opts(&path(&format!("race/{key}")), body.into(), PutMode::Create.into())
            .await;
        if created.is_ok() {
            won.push(format!("create:{key}"));
        }
    }
    // Then race to advance a shared counter by compare-and-swap.
    for _ in 0..40 {
        let current = match store.head(&path("counter")).await {
            Ok(meta) => meta,
            Err(_) => {
                let created = store
                    .put_opts(&path("counter"), payload(0, 4).into(), PutMode::Create.into())
                    .await;
                if created.is_ok() {
                    won.push("counter-create".into());
                }
                continue;
            }
        };
        let swap = store
            .put_opts(
                &path("counter"),
                payload(worker, 4).into(),
                PutMode::Update(UpdateVersion {
                    e_tag: current.e_tag.clone(),
                    version: None,
                })
                .into(),
            )
            .await;
        if swap.is_ok() {
            won.push(format!("swap:{}", current.e_tag.unwrap()));
        }
    }
    std::fs::write(
        std::path::PathBuf::from(&dir).join(format!("won-{worker}.txt")),
        won.join("\n"),
    )
    .unwrap();
}

#[test]
fn separate_processes_agree_on_one_winner_per_key_and_per_version() {
    let dir = tempfile::tempdir().unwrap();
    let exe = std::env::current_exe().unwrap();
    let children: Vec<_> = (0..4u8)
        .map(|worker| {
            Command::new(&exe)
                .args(["worker_entry", "--exact", "--nocapture", "--test-threads=1"])
                .env(WORKER_ENV, dir.path())
                .env("VIBESTORE_TEST_WORKER_ID", worker.to_string())
                .spawn()
                .unwrap()
        })
        .collect();
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    let mut created = std::collections::BTreeMap::<String, u8>::new();
    let mut swaps = std::collections::BTreeMap::<String, u8>::new();
    for worker in 0..4u8 {
        let report = std::fs::read_to_string(dir.path().join(format!("won-{worker}.txt"))).unwrap();
        for line in report.lines() {
            let (kind, value) = line.split_once(':').unwrap_or((line, ""));
            let map = match kind {
                "create" => &mut created,
                "swap" => &mut swaps,
                _ => continue,
            };
            *map.entry(value.to_string()).or_default() += 1;
        }
    }
    assert_eq!(created.len(), KEYS, "every key was created by someone");
    assert!(created.values().all(|&n| n == 1), "exactly one creator per key: {created:?}");
    assert!(
        swaps.values().all(|&n| n == 1),
        "no two processes advanced the same version: {swaps:?}"
    );
    assert!(!swaps.is_empty());
}
