//! Worker identity lifecycle. Memory persists references; KeyStorage owns secrets.
use crate::issue_self_signed_cert;
use anyhow::{bail, Context};
use lokai_domain::key_storage::{KeyStorage, SecretBytes, SecretKeyRef};
use lokai_memory::{WorkerStore, WorkerTlsKey};
#[cfg(test)]
use std::sync::Arc;

#[derive(Debug)]
pub struct TlsIdentity {
    pub certificate: Vec<u8>,
    pub private_key: SecretBytes,
}

fn validate(identity: &TlsIdentity) -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let key = rustls::pki_types::PrivateKeyDer::try_from(identity.private_key.as_ref().to_vec())
        .map_err(|_| anyhow::anyhow!("stored TLS private key is invalid"))?;
    rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(
                identity.certificate.clone(),
            )],
            key,
        )
        .context("stored TLS certificate and key are invalid or mismatched")?;
    Ok(())
}

fn generate() -> anyhow::Result<TlsIdentity> {
    let (certificate, key) = issue_self_signed_cert("lokai-worker")?;
    Ok(TlsIdentity {
        certificate,
        private_key: SecretBytes::new(key),
    })
}

fn publish(
    store: &WorkerStore,
    keys: &dyn KeyStorage,
    revision: Option<i64>,
    identity: &TlsIdentity,
) -> anyhow::Result<bool> {
    validate(identity)?;
    let reference = keys.create(identity.private_key.as_ref())?;
    // Verify through the port before publishing. A failed/ambiguous storage
    // operation retains its entry for recovery, never deletes a possibly live key.
    let persisted = keys.read(&reference)?;
    if persisted.as_ref() != identity.private_key.as_ref() {
        bail!("secure key storage round-trip failed");
    }
    if !store.publish_tls_identity(revision, &identity.certificate, &reference)? {
        if keys.delete(&reference).is_err() {
            tracing::warn!(key_reference = %reference.0, "unpublished TLS key cleanup failed");
        }
        return Ok(false);
    }
    store.scrub_legacy_tls_key()?;
    Ok(true)
}

/// Missing/revoked/unreadable referenced keys are errors, never a reason to
/// silently generate a replacement. Concurrent initializers converge by CAS.
pub fn load_or_create_tls_identity(
    store: &WorkerStore,
    keys: &dyn KeyStorage,
) -> anyhow::Result<TlsIdentity> {
    for _ in 0..8 {
        let (revision, identity) = match store.tls_identity()? {
            Some(row) => match row.key {
                WorkerTlsKey::Stored(reference) => {
                    let identity = TlsIdentity {certificate: row.cert_der, private_key: keys.read(&reference)?};
                    validate(&identity)?;
                    store.scrub_legacy_tls_key()?;
                    return Ok(identity);
                }
                WorkerTlsKey::Revoked(_) => bail!("worker TLS identity is revoked; explicitly rotate and re-enroll before serving"),
                WorkerTlsKey::Legacy(key) => (Some(row.revision), TlsIdentity {certificate: row.cert_der, private_key: key}),
            },
            None => (None, generate()?),
        };
        if publish(store, keys, revision, &identity)? {
            return Ok(identity);
        }
    }
    bail!("worker TLS identity changed concurrently; retry with other worker processes stopped")
}

/// Offline administration: stop listeners first and re-enroll coordinators after
/// changing their pinned certificate. Old keys are retained for backup restores.
pub fn rotate_tls_identity(
    store: &WorkerStore,
    keys: &dyn KeyStorage,
) -> anyhow::Result<TlsIdentity> {
    let revision = store.tls_identity()?.map(|row| row.revision);
    let identity = generate()?;
    if !publish(store, keys, revision, &identity)? {
        bail!("concurrent TLS identity change; rotation was not published");
    }
    Ok(identity)
}

/// Offline administration. Publish the tombstone first; deletion can be retried
/// without accidentally re-enabling a worker when its OS store is unavailable.
pub fn revoke_tls_identity(store: &WorkerStore, keys: &dyn KeyStorage) -> anyhow::Result<()> {
    let Some(row) = store.tls_identity()? else {
        bail!("worker has no TLS identity to revoke");
    };
    let reference: Option<SecretKeyRef> = match row.key {
        WorkerTlsKey::Stored(reference) => Some(reference),
        WorkerTlsKey::Revoked(reference) => reference,
        WorkerTlsKey::Legacy(_) => None,
    };
    if !store.revoke_tls_identity(row.revision)? {
        bail!("concurrent TLS identity change; revocation must be retried");
    }
    if let Some(reference) = reference {
        keys.delete(&reference)?;
    }
    store.scrub_legacy_tls_key()?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
