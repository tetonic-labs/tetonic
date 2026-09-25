//! Local bearer credential provider. Issuance/revocation are trusted provisioning
//! functions: never wire them directly to unauthenticated transport requests.
use super::*;
use sha2::{Digest, Sha256};
use tetonic_memory::ControlCredentialRow;

pub struct IssuedCredential {
    pub credential_id: String,
    pub expires_at: i64,
    secret: String,
}

impl IssuedCredential {
    /// One-time delivery to the intended principal over a protected channel.
    /// The engine cannot recover this value from its credential store.
    pub fn expose_secret(&self) -> &str {
        &self.secret
    }
}

impl std::fmt::Debug for IssuedCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IssuedCredential")
            .field("credential_id", &self.credential_id)
            .field("expires_at", &self.expires_at)
            .field("secret", &"[REDACTED]")
            .finish()
    }
}

/// Bound to a deployment audience and the application's store. This is a local
/// API credential path, not enterprise SSO, a password verifier or a worker key.
pub struct LocalCredentials {
    pub(super) store: SharedStore,
    pub(super) audience: String,
}

fn secret_hash(secret: &str) -> Vec<u8> {
    let mut hash = Sha256::new();
    hash.update(b"tetonic-control-credential-v1\0");
    hash.update(secret.as_bytes());
    hash.finalize().to_vec()
}

impl crate::Application {
    pub fn local_credentials(&self, audience: String) -> Result<LocalCredentials, ResourceError> {
        if audience.trim().is_empty() || audience.len() > 256 || audience.contains('\0') {
            return Err(ResourceError::Invalid);
        }
        let store = self
            .run_manager
            .managed()
            .store()
            .cloned()
            .ok_or(ResourceError::StorageRequired)?;
        Ok(LocalCredentials { store, audience })
    }
}

impl LocalCredentials {
    /// Trusted local provisioning only. No implicit principal creation or grant.
    /// Credentials are bounded to 24 hours; renewal issues a new independent key.
    pub async fn issue(
        &self,
        principal_id: String,
        lifetime_seconds: u32,
    ) -> Result<IssuedCredential, ResourceError> {
        if !(1..=86400).contains(&lifetime_seconds) {
            return Err(ResourceError::Invalid);
        }
        let issued_at = chrono::Utc::now().timestamp();
        let expires_at = issued_at + i64::from(lifetime_seconds);
        let credential_id = uuid::Uuid::new_v4().to_string();
        let secret = format!(
            "ttc_{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let row = ControlCredentialRow {
            credential_id: credential_id.clone(),
            principal_id,
            audience: self.audience.clone(),
            secret_hash: secret_hash(&secret),
            issued_at,
            expires_at,
        };
        self.store
            .write(move |db| db.issue_control_credential(&row))
            .await??;
        Ok(IssuedCredential {
            credential_id,
            expires_at,
            secret,
        })
    }

    pub async fn revoke(&self, credential_id: String) -> Result<(), ResourceError> {
        let audience = self.audience.clone();
        self.store
            .write(move |db| {
                db.revoke_control_credential(
                    &credential_id,
                    &audience,
                    chrono::Utc::now().timestamp(),
                )
            })
            .await??;
        Ok(())
    }
}

#[async_trait]
impl CredentialVerifier for LocalCredentials {
    fn memory_credential_check(
        &self,
        credential: &str,
    ) -> Option<tetonic_tools::MemoryCredentialCheck> {
        let digest = secret_hash(credential);
        let audience = self.audience.clone();
        Some(Arc::new(
            move |db, expected| matches!(db.control_credential_principal(&digest, &audience, chrono::Utc::now().timestamp()), Ok(Some(actor)) if actor == expected),
        ))
    }

    async fn verify(&self, credential: &str) -> Result<AuthorizedPrincipal, AccessError> {
        let value = credential.strip_prefix("ttc_").ok_or(AccessError)?;
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(AccessError);
        }
        let digest = secret_hash(credential);
        let audience = self.audience.clone();
        let id = self
            .store
            .read(move |db| {
                db.control_credential_principal(&digest, &audience, chrono::Utc::now().timestamp())
            })
            .await
            .map_err(|_| AccessError)?
            .map_err(|_| AccessError)?
            .ok_or(AccessError)?;
        AuthorizedPrincipal::new(id)
    }
}
