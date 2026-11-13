//! The store's own error type, mapped onto `object_store::Error` at the trait
//! boundary. Keeping it separate lets the synchronous core report a failed
//! precondition, a corrupt object and a simulated crash as different things.

use std::fmt;

pub(crate) const STORE: &str = "vibestore";

#[derive(Debug)]
pub enum StoreError {
    NotFound(String),
    AlreadyExists(String),
    Precondition(String),
    /// A feature celld does not get from this store. Always loud.
    NotSupported(String),
    /// The index and the blob root disagree, or a blob fails its address.
    Corrupt(String),
    /// Every other failure: I/O, SQLite, a malformed spec.
    Other(String),
    /// A simulated process death. No cleanup runs after it.
    Crashed(&'static str),
}

pub type Result<T> = std::result::Result<T, StoreError>;

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NotFound(path) => write!(f, "{path}: the object does not exist"),
            StoreError::AlreadyExists(path) => write!(f, "{path}: the object already exists"),
            StoreError::Precondition(path) => write!(f, "{path}: the ETag does not match"),
            StoreError::NotSupported(what) => write!(f, "vibestore does not support {what}"),
            StoreError::Corrupt(what) => write!(f, "vibestore is inconsistent: {what}"),
            StoreError::Other(what) => f.write_str(what),
            StoreError::Crashed(at) => write!(f, "simulated crash at {at}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl StoreError {
    pub(crate) fn other(context: impl fmt::Display, error: impl fmt::Display) -> Self {
        StoreError::Other(format!("{context}: {error}"))
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        StoreError::Other(format!("index: {error}"))
    }
}

impl From<StoreError> for object_store::Error {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::NotFound(path) => object_store::Error::NotFound {
                path,
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "the object does not exist",
                )),
            },
            StoreError::AlreadyExists(path) => object_store::Error::AlreadyExists {
                path,
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "the object already exists",
                )),
            },
            StoreError::Precondition(path) => object_store::Error::Precondition {
                path,
                source: Box::new(std::io::Error::other("the ETag does not match")),
            },
            StoreError::NotSupported(what) => object_store::Error::NotSupported {
                source: Box::new(std::io::Error::other(format!(
                    "vibestore does not support {what}"
                ))),
            },
            other => object_store::Error::Generic {
                store: STORE,
                source: Box::new(std::io::Error::other(other.to_string())),
            },
        }
    }
}
