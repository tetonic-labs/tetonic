//! Opaque persistent secret ownership. No OS, database, or TLS dependencies.
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretKeyRef(pub String);

/// Plaintext is neither serializable nor printable, and is cleared on drop.
pub struct SecretBytes(zeroize::Zeroizing<Vec<u8>>);
impl SecretBytes {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(zeroize::Zeroizing::new(bytes))
    }
}
impl AsRef<[u8]> for SecretBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}
impl std::fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretBytes([REDACTED])")
    }
}

#[derive(Debug, Error)]
pub enum KeyStorageError {
    #[error("secure key storage unavailable; unlock/configure the OS credential store for the worker service account (no plaintext fallback)")]
    Unavailable,
    #[error(
        "private key reference not found; restore the credential store or re-enroll this worker"
    )]
    Missing,
    #[error("unsupported or invalid private key reference")]
    InvalidReference,
}

/// Entries are immutable by reference. Rotation creates a new entry; callers
/// publish its reference before deciding whether an old entry may be deleted.
pub trait KeyStorage: Send + Sync {
    /// Allocate a fresh globally unique reference, never overwrite an existing
    /// entry, and return only after persistent storage succeeds.
    fn create(&self, secret: &[u8]) -> Result<SecretKeyRef, KeyStorageError>;
    fn read(&self, reference: &SecretKeyRef) -> Result<SecretBytes, KeyStorageError>;
    fn delete(&self, reference: &SecretKeyRef) -> Result<(), KeyStorageError>;
}
