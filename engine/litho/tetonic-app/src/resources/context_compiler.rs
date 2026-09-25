use super::*;
use tetonic_context::{interfaces::ContextAccessGate, pipeline::ContextCompiler};

struct MembershipGate {
    store: SharedStore,
    actor: String,
    credential: super::credential_binding::BoundCredential,
    context: String,
    session: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bound_membership_checks_session_and_current_grants() {
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
        let service = local.contexts();
        service
            .create(
                alice.expose_secret(),
                "private".into(),
                ContextOwner::Private {
                    org_id: "org".into(),
                },
            )
            .await
            .unwrap();
        service
            .open_history(alice.expose_secret(), "private".into(), "discussion".into())
            .await
            .unwrap();
        let gate = MembershipGate {
            store: service.store.clone(),
            actor: "alice".into(),
            credential: super::credential_binding::BoundCredential::new(
                service.verifier.clone(),
                alice.expose_secret(),
            ),
            context: "private".into(),
            session: "discussion".into(),
        };
        let session = tetonic_domain::SessionId::new("discussion");
        assert!(gate.authorize(&session).await.is_ok());
        assert!(gate
            .authorize(&tetonic_domain::SessionId::new("other"))
            .await
            .is_err());
        let admin_gate = MembershipGate {
            actor: "admin".into(),
            credential: super::credential_binding::BoundCredential::new(
                service.verifier.clone(),
                admin.expose_secret(),
            ),
            store: gate.store.clone(),
            context: gate.context.clone(),
            session: gate.session.clone(),
        };
        assert!(admin_gate.authorize(&session).await.is_err());
        local
            .credentials()
            .revoke(alice.credential_id.clone())
            .await
            .unwrap();
        assert!(gate.authorize(&session).await.is_err());
        let replacement = local
            .credentials()
            .issue("alice".into(), 3600)
            .await
            .unwrap();
        let gate = MembershipGate {
            credential: super::super::credential_binding::BoundCredential::new(
                service.verifier.clone(),
                replacement.expose_secret(),
            ),
            ..gate
        };
        assert!(gate.authorize(&session).await.is_ok());
        local
            .resources()
            .set_organization_member(admin.expose_secret(), "org".into(), "alice".into(), None)
            .await
            .unwrap();
        assert!(gate.authorize(&session).await.is_err());
    }
}

#[async_trait]
impl ContextAccessGate for MembershipGate {
    async fn authorize(&self, session: &tetonic_domain::SessionId) -> Result<(), ()> {
        self.credential.verify(&self.actor).await.map_err(|_| ())?;
        if session.0 != self.session {
            return Err(());
        }
        let (actor, context, session) = (
            self.actor.clone(),
            self.context.clone(),
            self.session.clone(),
        );
        let allowed = self
            .store
            .read(move |db| db.context_session_access(&actor, &context, &session))
            .await
            .map_err(|_| ())?
            .map_err(|_| ())?;
        if allowed {
            Ok(())
        } else {
            Err(())
        }
    }
}

impl ContextService {
    /// Trusted composition: provider and artifact grants must be configured
    /// independently. This adds current membership/session checks, not source grants.
    pub async fn bind_compiler(
        &self,
        credential: &str,
        context: String,
        session: String,
        mut compiler: ContextCompiler,
    ) -> Result<ContextCompiler, ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        if let Some(inner) = compiler.artifact_store.take() {
            compiler.artifact_store = Some(Arc::new(super::context_artifacts::ScopedArtifacts {
                store: self.store.clone(),
                actor: actor.principal_id.clone(),
                credential: super::credential_binding::BoundCredential::new(
                    self.verifier.clone(),
                    credential,
                ),
                context: context.clone(),
                inner,
            }));
        }
        let gate = Arc::new(MembershipGate {
            store: self.store.clone(),
            actor: actor.principal_id,
            credential: super::credential_binding::BoundCredential::new(
                self.verifier.clone(),
                credential,
            ),
            context,
            session: session.clone(),
        });
        gate.authorize(&tetonic_domain::SessionId::new(session))
            .await
            .map_err(|_| ResourceError::Denied)?;
        Ok(compiler.with_access_gate(gate))
    }
}
