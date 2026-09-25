//! Private host-side credential binding. Never serialize or log bearer material.
use super::*;

#[derive(Clone)]
pub(super) struct BoundCredential {
    verifier: Arc<dyn CredentialVerifier>,
    secret: Arc<str>,
}

impl BoundCredential {
    pub(super) fn new(verifier: Arc<dyn CredentialVerifier>, secret: &str) -> Self {
        Self {
            verifier,
            secret: Arc::from(secret),
        }
    }

    pub(super) async fn verify(&self, expected_actor: &str) -> Result<(), AccessError> {
        let current = self.verifier.verify(&self.secret).await?;
        if current.principal_id == expected_actor {
            Ok(())
        } else {
            Err(AccessError)
        }
    }
}
