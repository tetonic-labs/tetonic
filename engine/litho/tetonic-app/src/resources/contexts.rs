use super::*;

/// Authorized content access, separate from metadata resource permissions.
/// Credential validity is checked at admission; grants are checked in the store
/// snapshot that serves the operation. No scoped inference is activated here.
pub struct ContextService {
    pub(super) store: SharedStore,
    pub(super) verifier: Arc<dyn CredentialVerifier>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn context_service_verifies_credentials_and_never_uses_metadata_admin_override() {
        let dir = tempfile::tempdir().unwrap();
        let local = LocalControl::open(dir.path().join("control.db"), "test".into())
            .await
            .unwrap();
        local
            .bootstrap("admin".into(), "org".into(), "Org".into())
            .await
            .unwrap();
        local.register_principal("alice".into()).await.unwrap();
        let admin = local
            .credentials()
            .issue("admin".into(), 3600)
            .await
            .unwrap();
        local
            .resources()
            .set_organization_member(
                admin.expose_secret(),
                "org".into(),
                "alice".into(),
                Some(OrganizationRole::Member),
            )
            .await
            .unwrap();
        let alice = local
            .credentials()
            .issue("alice".into(), 3600)
            .await
            .unwrap();
        let owner = ContextOwner::Private {
            org_id: "org".into(),
        };
        let service = local.contexts();
        assert!(matches!(
            service
                .create("forged", "private".into(), owner.clone())
                .await,
            Err(ResourceError::Denied)
        ));
        service
            .create(alice.expose_secret(), "private".into(), owner.clone())
            .await
            .unwrap();
        assert!(matches!(
            service
                .create(admin.expose_secret(), "private".into(), owner.clone())
                .await,
            Err(ResourceError::Denied)
        ));
        service
            .open_history(alice.expose_secret(), "private".into(), "discussion".into())
            .await
            .unwrap();
        let append = || {
            service.append_message(
                alice.expose_secret(),
                "private".into(),
                "discussion".into(),
                "request-1".into(),
                "PRIVATECANARY".into(),
            )
        };
        let first = append().await.unwrap();
        assert_eq!(append().await.unwrap(), first);
        assert!(service
            .append_message(
                alice.expose_secret(),
                "private".into(),
                "discussion".into(),
                "request-1".into(),
                "changed".into()
            )
            .await
            .is_err());
        let rows = service
            .transcript(
                alice.expose_secret(),
                "private".into(),
                "discussion".into(),
                20,
            )
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].2, "PRIVATECANARY");
        assert!(service
            .transcript(
                admin.expose_secret(),
                "private".into(),
                "discussion".into(),
                20
            )
            .await
            .is_err());
        assert!(service
            .append_message(
                admin.expose_secret(),
                "private".into(),
                "discussion".into(),
                "attack".into(),
                "intrusion".into()
            )
            .await
            .is_err());
        local
            .credentials()
            .revoke(alice.credential_id.clone())
            .await
            .unwrap();
        assert!(matches!(
            service
                .create(alice.expose_secret(), "private".into(), owner)
                .await,
            Err(ResourceError::Denied)
        ));
        assert!(matches!(
            service
                .transcript(
                    admin.expose_secret(),
                    "private".into(),
                    "guessed".into(),
                    20
                )
                .await,
            Err(ResourceError::Denied)
        ));
    }
}

impl ContextService {
    pub async fn open_history(
        &self,
        credential: &str,
        context: String,
        session: String,
    ) -> Result<(), ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        self.store
            .write(move |db| db.open_context_history(&actor.principal_id, &context, &session))
            .await??;
        Ok(())
    }

    pub async fn append_message(
        &self,
        credential: &str,
        context: String,
        session: String,
        request: String,
        content: String,
    ) -> Result<i64, ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        Ok(self
            .store
            .write(move |db| {
                db.append_context_message(
                    &actor.principal_id,
                    &context,
                    &session,
                    &request,
                    &content,
                )
            })
            .await??)
    }

    pub async fn create(
        &self,
        credential: &str,
        id: String,
        owner: ContextOwner,
    ) -> Result<(), ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        self.store
            .write(move |db| db.create_information_context(&actor.principal_id, &id, &owner))
            .await??;
        Ok(())
    }

    pub async fn transcript(
        &self,
        credential: &str,
        context: String,
        session: String,
        limit: u32,
    ) -> Result<Vec<(i64, String, String)>, ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        Ok(self
            .store
            .read(move |db| db.scoped_transcript(&actor.principal_id, &context, &session, limit))
            .await??)
    }
}

impl crate::Application {
    pub fn context_service(
        &self,
        verifier: Arc<dyn CredentialVerifier>,
    ) -> Result<ContextService, ResourceError> {
        let store = self
            .run_manager
            .managed()
            .store()
            .cloned()
            .ok_or(ResourceError::StorageRequired)?;
        Ok(ContextService { store, verifier })
    }
}
