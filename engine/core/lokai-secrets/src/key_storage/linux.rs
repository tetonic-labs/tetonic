//! Persistent Secret Service adapter with interactive prompts disabled.
use super::*;
use dbus_secret_service::{Collection, EncryptionType, SecretService};
use std::collections::HashMap;

fn connect() -> Result<SecretService, KeyStorageError> {
    SecretService::connect_with_max_prompt_timeout(EncryptionType::Dh, 0)
        .map_err(|_| KeyStorageError::Unavailable)
}
fn collection(service: &SecretService) -> Result<Collection<'_>, KeyStorageError> {
    let collection = service
        .get_default_collection()
        .map_err(|_| KeyStorageError::Unavailable)?;
    if collection.path.to_string() == "/org/freedesktop/secrets/collection/session"
        || collection
            .is_locked()
            .map_err(|_| KeyStorageError::Unavailable)?
    {
        return Err(KeyStorageError::Unavailable);
    }
    Ok(collection)
}
fn attributes(id: &str) -> HashMap<&str, &str> {
    HashMap::from([("service", SERVICE), ("username", id)])
}
impl KeyStorage for PlatformKeyStorage {
    fn create(&self, secret: &[u8]) -> Result<SecretKeyRef, KeyStorageError> {
        let reference = SecretKeyRef(format!("{PREFIX}{}", uuid::Uuid::new_v4()));
        let service = connect()?;
        let collection = collection(&service)?;
        let item = collection
            .create_item(
                "Lokai private key",
                attributes(reference_id(&reference)?),
                secret,
                false,
                "application/octet-stream",
            )
            .map_err(|_| KeyStorageError::Unavailable)?;
        let read = SecretBytes::new(
            item.get_secret()
                .map_err(|_| KeyStorageError::Unavailable)?,
        );
        if read.as_ref() != secret {
            return Err(KeyStorageError::Unavailable);
        }
        Ok(reference)
    }
    fn read(&self, reference: &SecretKeyRef) -> Result<SecretBytes, KeyStorageError> {
        let id = reference_id(reference)?;
        let service = connect()?;
        let collection = collection(&service)?;
        let items = collection
            .search_items(attributes(id))
            .map_err(|_| KeyStorageError::Unavailable)?;
        match items.as_slice() {
            [] => Err(KeyStorageError::Missing),
            [item] if !item.is_locked().map_err(|_| KeyStorageError::Unavailable)? => item
                .get_secret()
                .map(SecretBytes::new)
                .map_err(|_| KeyStorageError::Unavailable),
            _ => Err(KeyStorageError::Unavailable),
        }
    }
    fn delete(&self, reference: &SecretKeyRef) -> Result<(), KeyStorageError> {
        let id = reference_id(reference)?;
        let service = connect()?;
        let collection = collection(&service)?;
        let items = collection
            .search_items(attributes(id))
            .map_err(|_| KeyStorageError::Unavailable)?;
        match items.as_slice() {
            [] => Ok(()),
            [item] => item.delete().map_err(|_| KeyStorageError::Unavailable),
            _ => Err(KeyStorageError::Unavailable),
        }
    }
}
