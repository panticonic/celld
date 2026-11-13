// Copyright 2026 Deno Land Inc. Apache-2.0 license.

//! Per-class secrets through the real binary and a local blob root.
//!
//! `celld secret` stores sealed values, `celld deploy` refuses a deployment whose
//! secrets are not stored, and a node opens them with the key and gives each to
//! its declaring class only. The deploy step bundles with `esbuild`; set
//! `CELLD_ESBUILD` or put it on `PATH`, or the test says so and returns.
//! `CELLD_REQUIRE_SECRETS_E2E=1` makes a missing esbuild a failure.
#![cfg(unix)]
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn find_esbuild() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CELLD_ESBUILD") {
        return Some(PathBuf::from(path));
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join("esbuild"))
            .find(|candidate| candidate.is_file())
    })
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("secrets")
}

struct Fleet {
    root: tempfile::TempDir,
    esbuild: PathBuf,
}

impl Fleet {
    fn bucket(&self) -> String {
        format!("vibestore://{}", self.root.path().join("blobs").display())
    }

    fn celld(&self, arguments: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_celld"))
            .args(arguments)
            .env("CELLD_ESBUILD", &self.esbuild)
            .env_remove("CELLD_SECRETS_KEY")
            .env_remove("CELLD_SECRETS_KEY_FILE")
            .output()
            .expect("run celld")
    }
}

fn text(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Every byte stored under `dir`.
fn stored_bytes(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if let Ok(bytes) = std::fs::read(&path) {
                found.push((path, bytes));
            }
        }
    }
    found
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

async fn get_json(client: &reqwest::Client, url: &str) -> Result<Value, String> {
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    let body = response.text().await.map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| format!("{url}: {e}: {body}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_secret_is_sealed_in_the_bucket_and_seen_by_its_class_only() {
    let Some(esbuild) = find_esbuild() else {
        if std::env::var_os("CELLD_REQUIRE_SECRETS_E2E").is_some() {
            panic!("CELLD_REQUIRE_SECRETS_E2E is set but no esbuild was found");
        }
        eprintln!("skipping: set CELLD_ESBUILD or put esbuild on PATH");
        return;
    };
    let fleet = Fleet {
        root: tempfile::tempdir().unwrap(),
        esbuild,
    };
    let bucket = fleet.bucket();
    std::fs::create_dir_all(fleet.root.path().join("blobs")).unwrap();
    let project = example();
    let project = project.to_str().unwrap();

    // A key is one line of base64, and a value cannot be stored without one.
    let keygen = fleet.celld(&["secret", "keygen"]);
    assert!(keygen.status.success(), "{}", text(&keygen));
    let key = String::from_utf8(keygen.stdout).unwrap().trim().to_string();
    assert_eq!(key.len(), 44, "{key:?}");
    let key_file = fleet.root.path().join("secrets.key");
    std::fs::write(&key_file, format!("{key}\n")).unwrap();
    let key_file = key_file.to_str().unwrap();
    let no_key = fleet.celld(&[
        "secret", "put", "secrets", "API_TOKEN", "--value", "x", "--bucket", &bucket,
    ]);
    assert!(!no_key.status.success());
    assert!(text(&no_key).contains("needs the secrets key"), "{}", text(&no_key));

    // A deployment whose secret is not stored is refused, naming what to run.
    let refused = fleet.celld(&["deploy", project, "--bucket", &bucket]);
    assert!(!refused.status.success(), "{}", text(&refused));
    let message = text(&refused);
    assert!(message.contains("API_TOKEN (class Vault)"), "{message}");
    assert!(message.contains("celld secret put secrets"), "{message}");

    // Store it, list it (names only), and look at what the bucket holds.
    let value = "s3cr3t-value-for-the-test";
    let put = fleet.celld(&[
        "secret",
        "put",
        "secrets",
        "API_TOKEN",
        "--value",
        value,
        "--secrets-key-file",
        key_file,
        "--bucket",
        &bucket,
    ]);
    assert!(put.status.success(), "{}", text(&put));
    let listed = fleet.celld(&["secret", "list", "secrets", "--bucket", &bucket]);
    assert_eq!(text(&listed).trim(), "API_TOKEN");
    let deployed = fleet.celld(&["deploy", project, "--bucket", &bucket]);
    assert!(deployed.status.success(), "{}", text(&deployed));
    let files = stored_bytes(fleet.root.path());
    assert!(
        files.iter().any(|(_, bytes)| contains(bytes, b"aes-256-gcm")),
        "the sealed secret is in the bucket"
    );
    for (path, bytes) in &files {
        if path.ends_with("secrets.key") {
            continue;
        }
        assert!(
            !contains(bytes, value.as_bytes()),
            "{} holds the secret in the clear",
            path.display()
        );
    }

    // A node with no key cannot load a deployment that declares secrets, and
    // says why. It does not serve the class a missing credential.
    let keyless = fleet.celld(&["--bucket", &bucket, "--listen", "127.0.0.1:0", "--internal-listen", "127.0.0.1:0"]);
    assert!(!keyless.status.success());
    assert!(text(&keyless).contains("no key to open them"), "{}", text(&keyless));

    // A node with the key serves the secret to Vault and to nothing else.
    let port = free_port();
    let log = std::fs::File::create(fleet.root.path().join("node.log")).unwrap();
    let mut node = Command::new(env!("CARGO_BIN_EXE_celld"))
        .args(["--bucket", &bucket, "--secrets-key-file", key_file])
        .args(["--listen", &format!("127.0.0.1:{port}")])
        .args(["--internal-listen", "127.0.0.1:0"])
        .env("CELLD_ESBUILD", &fleet.esbuild)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .expect("start celld");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let deadline = Instant::now() + Duration::from_secs(90);
    let vault = loop {
        match get_json(&client, &format!("{base}/vault")).await {
            Ok(value) => break value,
            Err(error) => {
                assert!(Instant::now() < deadline, "the node never served: {error}");
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        }
    };
    assert_eq!(vault["token"], value, "{vault}");
    assert_eq!(vault["moduleEnvToken"], Value::Null, "{vault}");
    assert_eq!(vault["keys"], serde_json::json!(["API_TOKEN", "PUBLIC"]));
    let other = get_json(&client, &format!("{base}/other")).await.unwrap();
    assert_eq!(other["token"], Value::Null, "{other}");
    assert_eq!(other["hasToken"], false, "{other}");
    let worker = get_json(&client, &format!("{base}/")).await.unwrap();
    assert_eq!(worker["token"], Value::Null, "{worker}");
    assert_eq!(worker["keys"], serde_json::json!(["PUBLIC"]));
    let _ = node.kill();
    let _ = node.wait();

    // Deleting the value takes it from the list.
    let deleted = fleet.celld(&["secret", "delete", "secrets", "API_TOKEN", "--bucket", &bucket]);
    assert!(deleted.status.success(), "{}", text(&deleted));
    let listed = fleet.celld(&["secret", "list", "secrets", "--bucket", &bucket]);
    assert_eq!(text(&listed).trim(), "");
}
