//! OS-backed opaque secret entries. No application-managed plaintext files.
use lokai_domain::key_storage::{KeyStorage, KeyStorageError, SecretBytes, SecretKeyRef};

#[derive(Default)]
pub struct PlatformKeyStorage;

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
const SERVICE: &str = "org.lokai.private-keys.v1";
const PREFIX: &str = "os-keyring:v1:";

fn reference_id(reference: &SecretKeyRef) -> Result<&str, KeyStorageError> {
    let id = reference
        .0
        .strip_prefix(PREFIX)
        .ok_or(KeyStorageError::InvalidReference)?;
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| KeyStorageError::InvalidReference)?;
    if parsed.to_string() != id {
        return Err(KeyStorageError::InvalidReference);
    }
    Ok(id)
}

#[cfg(any(windows, target_os = "macos"))]
fn entry(reference: &SecretKeyRef) -> Result<keyring::Entry, KeyStorageError> {
    let id = reference_id(reference)?;
    // Select the concrete backend, never the replaceable global/mock default.
    #[cfg(windows)]
    let builder = keyring::windows::default_credential_builder();
    #[cfg(target_os = "macos")]
    let builder = keyring::macos::default_credential_builder();
    let credential = builder.build(None, SERVICE, id).map_err(map_error)?;
    Ok(keyring::Entry::new_with_credential(credential))
}

#[cfg(any(windows, target_os = "macos"))]
fn map_error(error: keyring::Error) -> KeyStorageError {
    match error {
        keyring::Error::NoEntry => KeyStorageError::Missing,
        _ => KeyStorageError::Unavailable,
    }
}

#[cfg(any(windows, target_os = "macos"))]
impl KeyStorage for PlatformKeyStorage {
    fn create(&self, secret: &[u8]) -> Result<SecretKeyRef, KeyStorageError> {
        let reference = SecretKeyRef(format!("{PREFIX}{}", uuid::Uuid::new_v4()));
        let entry = entry(&reference)?;
        entry.set_secret(secret).map_err(map_error)?;
        // Confirm exact round-trip before a caller publishes this reference.
        let read = SecretBytes::new(entry.get_secret().map_err(map_error)?);
        if read.as_ref() != secret {
            return Err(KeyStorageError::Unavailable);
        }
        Ok(reference)
    }
    fn read(&self, reference: &SecretKeyRef) -> Result<SecretBytes, KeyStorageError> {
        entry(reference)?
            .get_secret()
            .map(SecretBytes::new)
            .map_err(map_error)
    }
    fn delete(&self, reference: &SecretKeyRef) -> Result<(), KeyStorageError> {
        match entry(reference)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(map_error(error)),
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
impl KeyStorage for PlatformKeyStorage {
    fn create(&self, _: &[u8]) -> Result<SecretKeyRef, KeyStorageError> {
        Err(KeyStorageError::Unavailable)
    }
    fn read(&self, reference: &SecretKeyRef) -> Result<SecretBytes, KeyStorageError> {
        reference_id(reference)?;
        Err(KeyStorageError::Unavailable)
    }
    fn delete(&self, reference: &SecretKeyRef) -> Result<(), KeyStorageError> {
        reference_id(reference)?;
        Err(KeyStorageError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unknown_versions_and_foreign_credential_names() {
        for input in [
            "",
            "os-keyring:v2:00000000-0000-0000-0000-000000000000",
            "other-service",
            "os-keyring:v1:../../credential",
        ] {
            assert!(matches!(
                PlatformKeyStorage.read(&SecretKeyRef(input.into())),
                Err(KeyStorageError::InvalidReference)
            ));
        }
    }

    #[test]
    #[ignore = "writes a disposable entry in the native OS credential store; run explicitly on a configured host"]
    fn native_round_trip_delete_and_missing() {
        let vault = PlatformKeyStorage;
        let bytes = b"disposable Lokai credential-store test";
        let reference = vault.create(bytes).unwrap();
        let result = vault.read(&reference);
        let cleanup = vault.delete(&reference);
        assert_eq!(result.unwrap().as_ref(), bytes);
        cleanup.unwrap();
        assert!(matches!(
            vault.read(&reference),
            Err(KeyStorageError::Missing)
        ));
        vault.delete(&reference).unwrap();
    }
}

#[cfg(target_os = "linux")]
mod linux;
