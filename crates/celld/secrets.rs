// Copyright 2026 Deno Land Inc. Apache-2.0 license.

// This module reads operator input (a key file) and uses OS randomness for
// nonces; neither is part of the simulated execution domain.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

//! Per-class secrets: values that one Durable Object class sees and nothing
//! else does.
//!
//! A Wrangler `vars` entry travels in the deployment manifest, in the clear,
//! and is visible to every handler of the Worker. A secret here does neither.
//!
//! * **Declared by name, per class.** The `celld` block of the config lists
//!   `durable_objects.<Class>.secrets: ["NAME"]`. Only the names are in the
//!   deployment manifest.
//! * **Stored apart, sealed.** The values live in the fleet bucket under
//!   `secrets/<script>/<NAME>`, outside every deployment, each sealed with
//!   AES-256-GCM under an operator-held key that is not in the bucket
//!   (`CELLD_SECRETS_KEY`, or `CELLD_SECRETS_KEY_FILE`). The script and the name
//!   are the associated data, so a sealed value cannot be moved to another name
//!   or script. Reading the bucket reveals no secret.
//! * **Opened by the node, injected into one class.** When a node loads a
//!   deployment it opens the values the deployment's classes name, and the
//!   harness hands each one to the constructor of its class only: the `env`
//!   argument of that class has the extra properties, and no other `env` (the
//!   Worker's `fetch`, another class, `cloudflare:workers`) does.
//!
//! The values are never in the manifest, the bootstrap source or a log line;
//! [`ClassSecrets`]' `Debug` prints names only.

use crate::bucket::Bucket;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, bail, ensure, Context as _};
use base64::Engine as _;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The key, base64 or 64 hex digits.
pub const KEY_ENV: &str = "CELLD_SECRETS_KEY";
/// A file holding the key: 32 raw bytes, or the same text as `CELLD_SECRETS_KEY`.
pub const KEY_FILE_ENV: &str = "CELLD_SECRETS_KEY_FILE";

const FORMAT_VERSION: u64 = 1;
const ALGORITHM: &str = "aes-256-gcm";
/// A secret is a credential, not a document.
pub const MAX_VALUE_BYTES: usize = 64 * 1024;

/// The 32-byte key sealing secrets. Never printed.
#[derive(Clone)]
pub struct SecretsKey([u8; 32]);

impl std::fmt::Debug for SecretsKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretsKey(..)")
    }
}

impl SecretsKey {
    pub fn generate() -> Self {
        let mut key = [0_u8; 32];
        getrandom::fill(&mut key).expect("the operating system provides randomness");
        Self(key)
    }

    /// 32 raw bytes, or text that is 64 hex digits or the base64 of 32 bytes.
    pub fn parse(bytes: &[u8]) -> anyhow::Result<Self> {
        if let Ok(raw) = <[u8; 32]>::try_from(bytes) {
            return Ok(Self(raw));
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| anyhow!("the secrets key must be 32 bytes, 64 hex digits or base64"))?
            .trim();
        let decoded = if text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            (0..32)
                .map(|index| u8::from_str_radix(&text[2 * index..2 * index + 2], 16))
                .collect::<Result<Vec<u8>, _>>()
                .ok()
        } else {
            base64::engine::general_purpose::STANDARD.decode(text).ok()
        };
        let decoded = decoded
            .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
            .ok_or_else(|| {
                anyhow!("the secrets key must be 32 bytes, as 64 hex digits or as base64")
            })?;
        Ok(Self(decoded))
    }

    /// The key as the text `CELLD_SECRETS_KEY` takes.
    pub fn to_base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.0)
    }

    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("read the secrets key file {}", path.display()))?;
        Self::parse(&bytes).with_context(|| format!("secrets key file {}", path.display()))
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new((&self.0).into())
    }
}

/// What `--secrets-key-file` set; it wins over the variables.
static KEY_FILE_FLAG: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn set_key_file_flag(path: PathBuf) {
    *KEY_FILE_FLAG.lock().unwrap() = Some(path);
}

/// The key this process was given, if any: the flag, then the file variable,
/// then the key variable. An invalid key is an error and is never ignored.
pub fn key_from_environment() -> anyhow::Result<Option<SecretsKey>> {
    if let Some(path) = KEY_FILE_FLAG.lock().unwrap().clone() {
        return SecretsKey::from_file(&path).map(Some);
    }
    if let Some(path) = std::env::var_os(KEY_FILE_ENV).filter(|value| !value.is_empty()) {
        return SecretsKey::from_file(Path::new(&path)).map(Some);
    }
    match std::env::var(KEY_ENV) {
        Ok(value) if !value.trim().is_empty() => SecretsKey::parse(value.as_bytes())
            .with_context(|| KEY_ENV.to_string())
            .map(Some),
        _ => Ok(None),
    }
}

/// The bucket key of one secret. The script and the name are validated by
/// every entry point, so neither can add a path segment.
pub fn object_key(script: &str, name: &str) -> String {
    format!("secrets/{script}/{name}")
}

fn associated_data(script: &str, name: &str) -> Vec<u8> {
    format!("celld-secret/v{FORMAT_VERSION}\0{script}\0{name}").into_bytes()
}

/// A secret name is a JavaScript binding name, which is also what keeps it a
/// bucket key segment.
pub fn valid_name(name: &str) -> bool {
    name.len() <= 128
        && name
            .chars()
            .next()
            .is_some_and(|first| first == '_' || first == '$' || first.is_ascii_alphabetic())
        && name
            .chars()
            .skip(1)
            .all(|next| next == '_' || next == '$' || next.is_ascii_alphanumeric())
}

fn valid_script(script: &str) -> bool {
    !script.is_empty()
        && script.len() <= 63
        && script
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn check_names(script: &str, name: Option<&str>) -> anyhow::Result<()> {
    ensure!(
        valid_script(script),
        "{script:?} is not a Worker name (lowercase letters, digits and hyphens)"
    );
    if let Some(name) = name {
        ensure!(
            valid_name(name),
            "{name:?} is not a valid secret name (a JavaScript identifier)"
        );
    }
    Ok(())
}

/// Seal one value for `script` and `name`: the bytes stored in the bucket.
pub fn seal(key: &SecretsKey, script: &str, name: &str, value: &[u8]) -> anyhow::Result<Vec<u8>> {
    check_names(script, Some(name))?;
    ensure!(
        value.len() <= MAX_VALUE_BYTES,
        "a secret is at most {MAX_VALUE_BYTES} bytes, not {}",
        value.len()
    );
    let mut nonce = [0_u8; 12];
    getrandom::fill(&mut nonce).expect("the operating system provides randomness");
    let ciphertext = key
        .cipher()
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: value,
                aad: &associated_data(script, name),
            },
        )
        .map_err(|_| anyhow!("could not seal the secret"))?;
    let engine = base64::engine::general_purpose::STANDARD;
    Ok(serde_json::to_vec(&serde_json::json!({
        "v": FORMAT_VERSION,
        "alg": ALGORITHM,
        "nonce": engine.encode(nonce),
        "ct": engine.encode(ciphertext),
    }))?)
}

/// Open a sealed value. A wrong key, a different script or name, and a
/// modified object all fail the same way.
pub fn open(key: &SecretsKey, script: &str, name: &str, sealed: &[u8]) -> anyhow::Result<Vec<u8>> {
    check_names(script, Some(name))?;
    let record: serde_json::Value =
        serde_json::from_slice(sealed).context("the secret is not a sealed record")?;
    ensure!(
        record["v"] == FORMAT_VERSION && record["alg"] == ALGORITHM,
        "the secret was sealed by a newer or different version of celld"
    );
    let engine = base64::engine::general_purpose::STANDARD;
    let field = |name: &str| -> anyhow::Result<Vec<u8>> {
        engine
            .decode(record[name].as_str().unwrap_or_default())
            .map_err(|_| anyhow!("the sealed secret has a malformed {name}"))
    };
    let (nonce, ciphertext) = (field("nonce")?, field("ct")?);
    ensure!(nonce.len() == 12, "the sealed secret has a malformed nonce");
    key.cipher()
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: &associated_data(script, name),
            },
        )
        .map_err(|_| {
            anyhow!(
                "could not open secret {name} of {script}: the key is not the one it was \
                 sealed with, or the stored value was changed"
            )
        })
}

/// Seal `value` and store it as secret `name` of `script`, replacing any
/// earlier value.
pub async fn put(
    bucket: &Bucket,
    key: &SecretsKey,
    script: &str,
    name: &str,
    value: &[u8],
) -> anyhow::Result<()> {
    let sealed = seal(key, script, name, value)?;
    bucket
        .put(&object_key(script, name), bytes::Bytes::from(sealed))
        .await
}

/// The names stored for `script`, sorted. Values are not read.
pub async fn list(bucket: &Bucket, script: &str) -> anyhow::Result<Vec<String>> {
    check_names(script, None)?;
    let prefix = format!("secrets/{script}/");
    let mut names: Vec<String> = bucket
        .list(&prefix)
        .await?
        .into_iter()
        .filter_map(|meta| {
            meta.location
                .as_ref()
                .strip_prefix(&prefix)
                .map(str::to_string)
        })
        .filter(|name| valid_name(name))
        .collect();
    names.sort();
    Ok(names)
}

pub async fn delete(bucket: &Bucket, script: &str, name: &str) -> anyhow::Result<()> {
    check_names(script, Some(name))?;
    bucket.delete(&object_key(script, name)).await
}

/// Fail naming every secret a deployment declares and the bucket does not hold.
/// Needs no key: it looks at what is stored, not at what it says.
pub async fn require_present(
    bucket: &Bucket,
    script: &str,
    declared: &BTreeMap<String, Vec<String>>,
) -> anyhow::Result<()> {
    if declared.is_empty() {
        return Ok(());
    }
    let stored: BTreeSet<String> = list(bucket, script).await?.into_iter().collect();
    let missing: Vec<String> = declared
        .iter()
        .flat_map(|(class, names)| names.iter().map(move |name| (class, name)))
        .filter(|(_, name)| !stored.contains(*name))
        .map(|(class, name)| format!("{name} (class {class})"))
        .collect();
    ensure!(
        missing.is_empty(),
        "the deployment declares secrets the bucket does not hold: {}. Set each with \
         `celld secret put {script} NAME`",
        missing.join(", ")
    );
    Ok(())
}

/// The opened secrets of one deployment: class, then name, then value.
/// Printing it shows names and no value.
#[derive(Clone, Default)]
pub struct ClassSecrets(BTreeMap<String, BTreeMap<String, String>>);

impl std::fmt::Debug for ClassSecrets {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut shown = formatter.debug_map();
        for (class, secrets) in &self.0 {
            shown.entry(class, &secrets.keys().collect::<Vec<_>>());
        }
        shown.finish()
    }
}

impl ClassSecrets {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn from_values(values: BTreeMap<String, BTreeMap<String, String>>) -> Self {
        Self(values)
    }

    /// `(name, value)` of one class's secrets.
    pub fn for_class(&self, class: &str) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .get(class)
            .into_iter()
            .flat_map(|secrets| secrets.iter())
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    pub fn classes(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }
}

/// Open every secret a deployment's classes declare. A node that declares any
/// and has no key, or whose bucket lacks one, does not load the deployment:
/// serving it with a secret missing would hand the application `undefined` where
/// it expects a credential.
pub async fn open_for_deployment(
    bucket: &Bucket,
    key: Option<&SecretsKey>,
    script: &str,
    declared: &BTreeMap<String, Vec<String>>,
) -> anyhow::Result<ClassSecrets> {
    if declared.is_empty() {
        return Ok(ClassSecrets::default());
    }
    let Some(key) = key else {
        bail!(
            "the deployment declares class secrets ({}), and this node has no key to open \
             them: set {KEY_ENV} or {KEY_FILE_ENV} (or --secrets-key-file)",
            declared
                .values()
                .flatten()
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(", ")
        );
    };
    let mut opened: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (class, names) in declared {
        for name in names {
            let (sealed, _) = bucket
                .get(&object_key(script, name))
                .await?
                .ok_or_else(|| {
                    anyhow!(
                        "secret {name} of class {class} is not set: run \
                         `celld secret put {script} {name}`"
                    )
                })?;
            let value = open(key, script, name, &sealed)?;
            let value = String::from_utf8(value)
                .with_context(|| format!("secret {name} is not UTF-8 text"))?;
            opened
                .entry(class.clone())
                .or_default()
                .insert(name.clone(), value);
        }
    }
    Ok(ClassSecrets(opened))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> SecretsKey {
        SecretsKey([9; 32])
    }

    #[test]
    fn a_sealed_value_opens_and_is_not_the_plaintext() {
        let sealed = seal(&key(), "app", "TOKEN", b"hunter2").unwrap();
        assert!(!String::from_utf8_lossy(&sealed).contains("hunter2"));
        assert_eq!(open(&key(), "app", "TOKEN", &sealed).unwrap(), b"hunter2");
    }

    #[test]
    fn sealing_twice_gives_different_objects() {
        let first = seal(&key(), "app", "TOKEN", b"x").unwrap();
        let second = seal(&key(), "app", "TOKEN", b"x").unwrap();
        assert_ne!(first, second, "a fresh nonce every time");
    }

    #[test]
    fn a_value_does_not_open_under_another_name_script_or_key() {
        let sealed = seal(&key(), "app", "TOKEN", b"hunter2").unwrap();
        assert!(open(&key(), "app", "OTHER", &sealed).is_err());
        assert!(open(&key(), "other", "TOKEN", &sealed).is_err());
        assert!(open(&SecretsKey([1; 32]), "app", "TOKEN", &sealed).is_err());
    }

    #[test]
    fn a_modified_object_does_not_open() {
        let sealed = seal(&key(), "app", "TOKEN", b"hunter2").unwrap();
        let mut record: serde_json::Value = serde_json::from_slice(&sealed).unwrap();
        let mut ciphertext = base64::engine::general_purpose::STANDARD
            .decode(record["ct"].as_str().unwrap())
            .unwrap();
        ciphertext[0] ^= 1;
        record["ct"] = base64::engine::general_purpose::STANDARD
            .encode(ciphertext)
            .into();
        let error = open(&key(), "app", "TOKEN", &serde_json::to_vec(&record).unwrap())
            .unwrap_err()
            .to_string();
        assert!(error.contains("could not open secret TOKEN"), "{error}");
        assert!(open(&key(), "app", "TOKEN", b"not json").is_err());
    }

    #[test]
    fn names_and_sizes_are_checked() {
        assert!(seal(&key(), "app", "bad name", b"x").is_err());
        assert!(seal(&key(), "app", "../TOKEN", b"x").is_err());
        assert!(seal(&key(), "App", "TOKEN", b"x").is_err());
        assert!(seal(&key(), "app", "TOKEN", &vec![0; MAX_VALUE_BYTES + 1]).is_err());
        assert!(seal(&key(), "app", "TOKEN", &vec![0; MAX_VALUE_BYTES]).is_ok());
    }

    #[test]
    fn a_key_is_raw_hex_or_base64() {
        let raw = [7_u8; 32];
        assert_eq!(SecretsKey::parse(&raw).unwrap().0, raw);
        let hex: String = raw.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(SecretsKey::parse(hex.as_bytes()).unwrap().0, raw);
        let generated = SecretsKey::generate();
        let again = SecretsKey::parse(format!("{}\n", generated.to_base64()).as_bytes()).unwrap();
        assert_eq!(again.0, generated.0);
        assert!(SecretsKey::parse(b"short").is_err());
        assert!(SecretsKey::parse(&[0xff; 40]).is_err());
    }

    #[test]
    fn debug_never_shows_a_value() {
        let secrets = ClassSecrets::from_values(BTreeMap::from([(
            "Vault".to_string(),
            BTreeMap::from([("TOKEN".to_string(), "hunter2".to_string())]),
        )]));
        let shown = format!("{secrets:?} {:?}", key());
        assert!(shown.contains("TOKEN") && !shown.contains("hunter2"), "{shown}");
        assert_eq!(secrets.for_class("Vault").collect::<Vec<_>>(), [("TOKEN", "hunter2")]);
        assert_eq!(secrets.for_class("Other").count(), 0);
    }
}
