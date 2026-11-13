//! The `vibestore://` bucket URL.
//!
//! ```text
//! vibestore:///abs/path/to/blobroot?index=/abs/path/index.sqlite
//! ```
//!
//! The path is the Vibestudio blob root (`<root>/sha256/aa/bb/<rest>`). Query
//! parameters, all optional:
//!
//! | name | default | meaning |
//! | --- | --- | --- |
//! | `index` | `<root>/celld/index.sqlite` | the key index file |
//! | `shared` | `1` | `1`: the root is Vibestudio's, so keep GC retentions in `<root>/retentions.db` and never unlink blobs. `0`: the root is celld's alone; no retentions, and celld sweeps orphans |
//! | `namespace` | derived from the index path | the retention namespace id (`["celld", id]`) |
//! | `inline` | `16384` | objects up to this many bytes live in the index, not the CAS; `0` disables |
//! | `grace_secs` | `3600` | the age an unreferenced blob needs before an exclusive-mode sweep unlinks it |

use crate::error::{Result, StoreError};
use sha2::{Digest as _, Sha256};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

pub const SCHEME: &str = "vibestore";
pub const DEFAULT_INLINE_MAX: u64 = 16 * 1024;
pub const DEFAULT_GRACE: Duration = Duration::from_secs(3600);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VibeSpec {
    pub root: PathBuf,
    pub index: PathBuf,
    pub shared: bool,
    /// The id half of the retention namespace `["celld", id]`.
    pub namespace_id: String,
    pub inline_max: u64,
    pub grace: Duration,
}

/// Whether a bucket string names this backend. The scheme is case-insensitive.
pub fn is_spec(spec: &str) -> bool {
    let n = SCHEME.len();
    spec.get(..n + 3)
        .is_some_and(|head| head[..n].eq_ignore_ascii_case(SCHEME) && &head[n..] == "://")
}

impl VibeSpec {
    /// A spec over `root` with every default, for programmatic use.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let index = root.join("celld").join("index.sqlite");
        Self::resolved(root, index, true, None, DEFAULT_INLINE_MAX, DEFAULT_GRACE)
    }

    fn resolved(
        root: PathBuf,
        index: PathBuf,
        shared: bool,
        namespace_id: Option<String>,
        inline_max: u64,
        grace: Duration,
    ) -> Self {
        let namespace_id = namespace_id.unwrap_or_else(|| {
            let digest = Sha256::digest(index.to_string_lossy().as_bytes());
            hex::encode(&digest[..8])
        });
        Self {
            root,
            index,
            shared,
            namespace_id,
            inline_max,
            grace,
        }
    }

    pub fn parse(spec: &str) -> Result<Self> {
        if !is_spec(spec) {
            return Err(StoreError::Other(format!(
                "not a {SCHEME}:// bucket: {spec:?}"
            )));
        }
        let rest = &spec[SCHEME.len() + 3..];
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        if !path.starts_with('/') {
            return Err(StoreError::Other(format!(
                "{SCHEME}:// takes an absolute path and no host: write {SCHEME}:///path/to/blobroot"
            )));
        }
        let root = path_from_url(path)?;
        let mut index = None;
        let mut shared = true;
        let mut namespace = None;
        let mut inline_max = DEFAULT_INLINE_MAX;
        let mut grace = DEFAULT_GRACE;
        for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
            match name.as_ref() {
                "index" => index = Some(checked_path("index", &value)?),
                "shared" => shared = parse_bool(&name, &value)?,
                "namespace" => {
                    if value.is_empty()
                        || !value
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
                    {
                        return Err(StoreError::Other(format!(
                            "{SCHEME}:// namespace accepts only letters, digits and -_.: {value:?}"
                        )));
                    }
                    namespace = Some(value.into_owned());
                }
                "inline" => {
                    inline_max = value.parse().map_err(|_| {
                        StoreError::Other(format!("{SCHEME}:// inline must be a byte count"))
                    })?
                }
                "grace_secs" => {
                    grace = Duration::from_secs(value.parse().map_err(|_| {
                        StoreError::Other(format!("{SCHEME}:// grace_secs must be a number"))
                    })?)
                }
                other => {
                    return Err(StoreError::Other(format!(
                        "{SCHEME}:// has no parameter {other:?} \
                         (known: index, shared, namespace, inline, grace_secs)"
                    )))
                }
            }
        }
        let index = index.unwrap_or_else(|| root.join("celld").join("index.sqlite"));
        if !index.is_absolute() {
            return Err(StoreError::Other(format!(
                "{SCHEME}:// index must be an absolute path: {}",
                index.display()
            )));
        }
        Ok(Self::resolved(
            root, index, shared, namespace, inline_max, grace,
        ))
    }

    /// The text after `vibestore://`: what `Bucket::name` holds, so that
    /// `{scheme}://{name}` reads back as the spec. Parameters at their default
    /// are omitted (the default namespace derives from the index path, so it
    /// survives the round trip), which keeps operator-facing messages short.
    pub fn name(&self) -> String {
        let defaults = Self::resolved(
            self.root.clone(),
            self.root.join("celld").join("index.sqlite"),
            true,
            None,
            DEFAULT_INLINE_MAX,
            DEFAULT_GRACE,
        );
        let mut parameters = Vec::new();
        if self.index != defaults.index {
            parameters.push(format!("index={}", encode_query(&self.index.to_string_lossy())));
        }
        if !self.shared {
            parameters.push("shared=0".to_string());
        }
        let derived = Self::resolved(
            self.root.clone(),
            self.index.clone(),
            true,
            None,
            0,
            DEFAULT_GRACE,
        )
        .namespace_id;
        if self.namespace_id != derived {
            parameters.push(format!("namespace={}", self.namespace_id));
        }
        if self.inline_max != DEFAULT_INLINE_MAX {
            parameters.push(format!("inline={}", self.inline_max));
        }
        if self.grace != DEFAULT_GRACE {
            parameters.push(format!("grace_secs={}", self.grace.as_secs()));
        }
        let mut name = encode_path(&self.root);
        if !parameters.is_empty() {
            name.push('?');
            name.push_str(&parameters.join("&"));
        }
        name
    }

    /// The canonical spec: every parameter that differs from its default.
    pub fn canonical(&self) -> String {
        format!("{SCHEME}://{}", self.name())
    }

    /// The retention namespace, byte-identical to Vibestudio's
    /// `blobRetentionNamespace({kind, id})`, which is `JSON.stringify([kind, id])`.
    pub fn retention_namespace(&self) -> String {
        serde_json::to_string(&["celld", self.namespace_id.as_str()])
            .expect("a string pair always serializes")
    }
}

fn parse_bool(name: &str, value: &str) -> Result<bool> {
    match value {
        "1" | "true" | "on" | "yes" => Ok(true),
        "0" | "false" | "off" | "no" => Ok(false),
        _ => Err(StoreError::Other(format!(
            "{SCHEME}:// {name} must be 0 or 1, not {value:?}"
        ))),
    }
}

fn path_from_url(path: &str) -> Result<PathBuf> {
    let decoded = percent_encoding::percent_decode_str(path)
        .decode_utf8()
        .map_err(|_| StoreError::Other(format!("{SCHEME}:// path is not UTF-8: {path:?}")))?;
    let decoded = decoded.as_ref();
    // `vibestore:///C:/x` carries the drive after the URL's leading slash.
    let bytes = decoded.as_bytes();
    let drive =
        bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':';
    let plain = if cfg!(windows) && drive {
        &decoded[1..]
    } else {
        decoded
    };
    let root = PathBuf::from(plain);
    if root.components().filter(|c| matches!(c, Component::Normal(_))).count() == 0 {
        return Err(StoreError::Other(format!(
            "{SCHEME}:// refuses to use the filesystem root as a blob root"
        )));
    }
    checked_path("path", plain)
}

fn checked_path(what: &str, value: &str) -> Result<PathBuf> {
    let path = PathBuf::from(value);
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(StoreError::Other(format!(
            "{SCHEME}:// {what} may not contain `..`: {value:?}"
        )));
    }
    Ok(path)
}

fn encode_path(path: &Path) -> String {
    const SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'/')
        .remove(b'-')
        .remove(b'_')
        .remove(b'.');
    percent_encoding::utf8_percent_encode(&path.to_string_lossy(), SET).to_string()
}

fn encode_query(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_scheme_case_insensitively() {
        assert!(is_spec("vibestore:///x/y"));
        assert!(is_spec("ViBeStoRe:///x/y"));
        assert!(!is_spec("s3://bucket"));
        assert!(!is_spec("vibestore:/x"));
        assert!(!is_spec(""));
    }

    #[test]
    fn parses_defaults_and_explicit_parameters() {
        let spec = VibeSpec::parse("vibestore:///data/blobs").unwrap();
        assert_eq!(spec.root, PathBuf::from("/data/blobs"));
        assert_eq!(spec.index, PathBuf::from("/data/blobs/celld/index.sqlite"));
        assert!(spec.shared);
        assert_eq!(spec.inline_max, DEFAULT_INLINE_MAX);

        let spec = VibeSpec::parse(
            "vibestore:///data/blobs?index=/data/idx.sqlite&shared=0&namespace=fleet-a&inline=0&grace_secs=5",
        )
        .unwrap();
        assert_eq!(spec.index, PathBuf::from("/data/idx.sqlite"));
        assert!(!spec.shared);
        assert_eq!(spec.retention_namespace(), r#"["celld","fleet-a"]"#);
        assert_eq!(spec.inline_max, 0);
        assert_eq!(spec.grace, Duration::from_secs(5));
    }

    #[test]
    fn canonical_form_round_trips() {
        let spec =
            VibeSpec::parse("vibestore:///data/my%20blobs?index=/data/a%20b/i.sqlite").unwrap();
        assert_eq!(spec.root, PathBuf::from("/data/my blobs"));
        assert_eq!(VibeSpec::parse(&spec.canonical()).unwrap(), spec);
    }

    #[test]
    fn the_name_omits_defaults_and_still_round_trips() {
        let plain = VibeSpec::parse("vibestore:///data/blobs").unwrap();
        assert_eq!(plain.name(), "/data/blobs");
        let indexed = VibeSpec::parse("vibestore:///data/blobs?index=/data/i.sqlite").unwrap();
        assert_eq!(indexed.name(), "/data/blobs?index=%2Fdata%2Fi.sqlite");
        for text in [
            "vibestore:///data/blobs?index=/data/i.sqlite&shared=0&namespace=fleet-a&inline=0&grace_secs=5",
            "vibestore:///data/blobs?namespace=x",
            "vibestore:///data/blobs?inline=100",
        ] {
            let spec = VibeSpec::parse(text).unwrap();
            assert_eq!(VibeSpec::parse(&spec.canonical()).unwrap(), spec, "{text}");
        }
    }

    #[test]
    fn rejects_what_would_misdirect_writes() {
        for bad in [
            "vibestore://host/path",
            "vibestore://relative",
            "vibestore:///",
            "vibestore:///a/../b",
            "vibestore:///a/b?index=relative.sqlite",
            "vibestore:///a/b?index=/a/../x",
            "vibestore:///a/b?bogus=1",
            "vibestore:///a/b?shared=maybe",
            "vibestore:///a/b?namespace=a/b",
            "vibestore:///a/b?inline=lots",
            "s3://bucket",
        ] {
            assert!(VibeSpec::parse(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn default_namespace_is_per_index() {
        let a = VibeSpec::parse("vibestore:///r?index=/a.sqlite").unwrap();
        let b = VibeSpec::parse("vibestore:///r?index=/b.sqlite").unwrap();
        assert_ne!(a.namespace_id, b.namespace_id);
        assert_eq!(a.namespace_id.len(), 16);
    }
}
