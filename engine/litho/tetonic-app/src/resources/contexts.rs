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
            .provision_discussion(alice.expose_secret(), "private".into(), "discussion".into())
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
        let tools =
            tetonic_tools::Tools::new(tetonic_tools::Workspace::new(dir.path()).unwrap(), false);
        let bound = service
            .bind_recall(alice.expose_secret(), "private".into(), tools.clone())
            .await
            .unwrap();
        assert!(service
            .bind_recall(admin.expose_secret(), "private".into(), tools)
            .await
            .is_err());
        let check = bound.clone();
        tokio::task::spawn_blocking(move || {
            let result = check.execute("recall", &serde_json::json!({"query":"PRIVATECANARY"}));
            assert!(result.ok);
            assert!(result.content.contains("PRIVATECANARY"));
            let spoof = check.execute(
                "recall",
                &serde_json::json!({"query":"PRIVATECANARY","context":"other"}),
            );
            assert!(!spoof.ok);
            let preserved = check
                .with_memory("missing-other-database", None)
                .execute("recall", &serde_json::json!({"query":"PRIVATECANARY"}));
            assert!(preserved.ok);
            assert!(preserved.content.contains("PRIVATECANARY"));
        })
        .await
        .unwrap();
        local
            .credentials()
            .revoke(alice.credential_id.clone())
            .await
            .unwrap();
        tokio::task::spawn_blocking(move || {
            let denied = bound.execute("recall", &serde_json::json!({"query":"PRIVATECANARY"}));
            assert!(!denied.ok);
            assert!(!denied.content.contains("PRIVATECANARY"));
        })
        .await
        .unwrap();
        let replacement = local
            .credentials()
            .issue("alice".into(), 3600)
            .await
            .unwrap();
        let bound = service
            .bind_recall(
                replacement.expose_secret(),
                "private".into(),
                tetonic_tools::Tools::new(
                    tetonic_tools::Workspace::new(dir.path()).unwrap(),
                    false,
                ),
            )
            .await
            .unwrap();
        let check = bound.clone();
        tokio::task::spawn_blocking(move || {
            assert!(
                check
                    .execute("recall", &serde_json::json!({"query":"PRIVATECANARY"}))
                    .ok
            );
        })
        .await
        .unwrap();
        local
            .resources()
            .set_organization_member(admin.expose_secret(), "org".into(), "alice".into(), None)
            .await
            .unwrap();
        tokio::task::spawn_blocking(move || {
            let denied = bound.execute("recall", &serde_json::json!({"query":"PRIVATECANARY"}));
            assert!(!denied.ok);
            assert!(!denied.content.contains("PRIVATECANARY"));
        })
        .await
        .unwrap();
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

    #[tokio::test]
    async fn scoped_live_lookup_requires_current_membership() {
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
        let live = crate::session_live::SessionLiveStore::new();
        let session = crate::session_live::fixture_live_session();
        live.insert_in_context("secret".into(), "private".into(), session.clone())
            .unwrap();
        assert!(live.get("secret").is_none());
        let opened = service
            .open_live(alice.expose_secret(), &live, "private".into(), "secret".into())
            .await
            .unwrap();
        assert_eq!(opened.session_id(), "secret");
        let conversation = opened
            .take_conversation(alice.expose_secret())
            .await
            .unwrap();
        opened
            .restore_conversation(alice.expose_secret(), conversation)
            .await
            .unwrap();
        assert!(matches!(
            service
                .open_live(admin.expose_secret(), &live, "private".into(), "secret".into())
                .await,
            Err(ResourceError::Denied)
        ));
        assert!(matches!(
            service
                .open_live(alice.expose_secret(), &live, "shared".into(), "secret".into())
                .await,
            Err(ResourceError::Denied)
        ));
        local
            .resources()
            .set_organization_member(admin.expose_secret(), "org".into(), "alice".into(), None)
            .await
            .unwrap();
        assert!(matches!(
            service
                .open_live(alice.expose_secret(), &live, "private".into(), "secret".into())
                .await,
            Err(ResourceError::Denied)
        ));
        assert!(matches!(
            opened.take_conversation(alice.expose_secret()).await,
            Err(ResourceError::Denied)
        ));
    }

    #[tokio::test]
    async fn team_participation_context_is_the_callers_empty_working_context() {
        let dir = tempfile::tempdir().unwrap();
        let local = LocalControl::open(dir.path().join("control.db"), "test".into())
            .await
            .unwrap();
        local
            .bootstrap("admin".into(), "org".into(), "Org".into())
            .await
            .unwrap();
        local.register_principal("member".into()).await.unwrap();
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
                "member".into(),
                Some(OrganizationRole::Member),
            )
            .await
            .unwrap();
        local
            .resources()
            .create_team(
                admin.expose_secret(),
                "org".into(),
                "team".into(),
                "Team".into(),
            )
            .await
            .unwrap();
        local
            .resources()
            .set_team_member(
                admin.expose_secret(),
                "org".into(),
                "team".into(),
                "member".into(),
                true,
            )
            .await
            .unwrap();
        let member = local
            .credentials()
            .issue("member".into(), 3600)
            .await
            .unwrap();
        let service = local.contexts();
        service
            .create(
                member.expose_secret(),
                "member-private".into(),
                ContextOwner::Private {
                    org_id: "org".into(),
                },
            )
            .await
            .unwrap();
        service
            .provision_discussion(
                member.expose_secret(),
                "member-private".into(),
                "notes".into(),
            )
            .await
            .unwrap();
        service
            .append_message(
                member.expose_secret(),
                "member-private".into(),
                "notes".into(),
                "secret".into(),
                "PRIVATECANARY".into(),
            )
            .await
            .unwrap();
        assert!(matches!(
            service
                .team_participation_context(admin.expose_secret(), "org".into(), "missing".into())
                .await,
            Err(ResourceError::Denied)
        ));
        let working = service
            .team_participation_context(member.expose_secret(), "org".into(), "team".into())
            .await
            .unwrap();
        assert_eq!(
            working,
            crate::resources::team_participation_context_id("org", "team", "member").unwrap()
        );
        let hits = service
            .recall(
                member.expose_secret(),
                working.clone(),
                "PRIVATECANARY".into(),
                5,
            )
            .await
            .unwrap();
        assert!(hits.is_empty());
        let owner = service
            .team_participation_context(admin.expose_secret(), "org".into(), "team".into())
            .await
            .unwrap();
        assert_ne!(owner, working);
        assert!(matches!(
            service
                .recall(
                    admin.expose_secret(),
                    working,
                    "PRIVATECANARY".into(),
                    5,
                )
                .await,
            Err(ResourceError::Denied)
        ));
    }
}

/// A live conversation looked up under a current membership check.
/// Reading or replacing it checks membership again.
pub struct AuthorizedLive {
    live: std::sync::Arc<crate::session_live::LiveSession>,
    store: SharedStore,
    verifier: Arc<dyn CredentialVerifier>,
    actor: String,
    context: String,
    session: String,
}

impl AuthorizedLive {
    pub fn session_id(&self) -> &str {
        &self.session
    }

    pub async fn take_conversation(
        &self,
        credential: &str,
    ) -> Result<tetonic_core::Conversation, ResourceError> {
        self.recheck(credential).await?;
        self.live
            .take_conversation()
            .map_err(|_| ResourceError::Conflict)
    }

    pub async fn restore_conversation(
        &self,
        credential: &str,
        conversation: tetonic_core::Conversation,
    ) -> Result<(), ResourceError> {
        self.recheck(credential).await?;
        self.live.restore_conversation(conversation);
        Ok(())
    }

    async fn recheck(&self, credential: &str) -> Result<(), ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        if actor.principal_id != self.actor {
            return Err(ResourceError::Denied);
        }
        let principal = self.actor.clone();
        let context = self.context.clone();
        let allowed = self
            .store
            .read(move |db| db.context_access(&principal, &context))
            .await??;
        if allowed {
            Ok(())
        } else {
            Err(ResourceError::Denied)
        }
    }
}

impl ContextService {
    /// Membership-checked lookup of a process-local live conversation.
    /// Missing and unauthorized sessions are both denied. The handle rechecks
    /// membership before it reads or replaces the conversation.
    pub async fn open_live(
        &self,
        credential: &str,
        live: &crate::session_live::SessionLiveStore,
        context: String,
        session: String,
    ) -> Result<AuthorizedLive, ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        let principal = actor.principal_id.clone();
        let scope = context.clone();
        let allowed = self
            .store
            .read(move |db| db.context_access(&principal, &scope))
            .await??;
        if !allowed {
            return Err(ResourceError::Denied);
        }
        let found = live.get_in_context(&session, &context);
        let actor = self.verifier.verify(credential).await?;
        let principal = actor.principal_id;
        let scope = context.clone();
        let still_allowed = self
            .store
            .read({
                let principal = principal.clone();
                let scope = scope.clone();
                move |db| db.context_access(&principal, &scope)
            })
            .await??;
        if !still_allowed {
            return Err(ResourceError::Denied);
        }
        let live = found.ok_or(ResourceError::Denied)?;
        Ok(AuthorizedLive {
            live,
            store: self.store.clone(),
            verifier: self.verifier.clone(),
            actor: principal,
            context,
            session,
        })
    }

    pub async fn close_history(
        &self,
        credential: &str,
        context: String,
        session: String,
    ) -> Result<(), ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        self.store
            .write(move |db| db.close_context_history(&actor.principal_id, &context, &session))
            .await??;
        Ok(())
    }

    /// Bind recall to one authorized context. Does not activate an agent or
    /// authorize other tools. Credential validity is rechecked by the bound synchronous adapter.
    pub async fn bind_recall(
        &self,
        credential: &str,
        context: String,
        tools: tetonic_tools::Tools,
    ) -> Result<tetonic_tools::Tools, ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        let principal = actor.principal_id.clone();
        let scope = context.clone();
        let allowed = self
            .store
            .read(move |db| db.context_access(&principal, &scope))
            .await??;
        if !allowed {
            return Err(ResourceError::Denied);
        }
        let credential_check = self
            .verifier
            .memory_credential_check(credential)
            .ok_or(ResourceError::Denied)?;
        Ok(tools.with_context_memory(
            self.store.path().to_path_buf(),
            actor.principal_id,
            context,
            credential_check,
        ))
    }

    pub async fn recall(
        &self,
        credential: &str,
        context: String,
        query: String,
        limit: u32,
    ) -> Result<Vec<tetonic_memory::RecallHit>, ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        Ok(self
            .store
            .read(move |db| {
                db.recall_context_messages(&actor.principal_id, &context, &query, limit)
            })
            .await??)
    }

    /// Reopen a discussion. A missing id and a foreign id are both denied.
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

    /// Create a discussion and return its server-chosen id.
    pub async fn create_history(
        &self,
        credential: &str,
        context: String,
    ) -> Result<String, ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        Ok(self
            .store
            .write(move |db| db.create_context_history(&actor.principal_id, &context))
            .await??)
    }

    /// Test fixture. A caller-chosen id is not the employee create door.
    #[cfg(test)]
    pub(crate) async fn provision_discussion(
        &self,
        credential: &str,
        context: String,
        session: String,
    ) -> Result<(), ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        self.store
            .write(move |db| {
                db.insert_open_discussion(&actor.principal_id, &context, &session)
            })
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

    /// The caller's private working context for a team they own or belong to.
    /// It is not their other private history. A missing team and a
    /// non-participant are both denied.
    pub async fn team_participation_context(
        &self,
        credential: &str,
        org: String,
        team: String,
    ) -> Result<String, ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        Ok(self
            .store
            .write(move |db| db.ensure_actor_team_participation(&actor.principal_id, &org, &team))
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

    /// Project notes and digests for one authorized context. This does not
    /// publish them into another context.
    pub async fn project_memory(
        &self,
        credential: &str,
        context: String,
        root: std::path::PathBuf,
        token_budget: usize,
    ) -> Result<String, ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        Ok(self
            .store
            .read(move |db| {
                db.load_context_project_memory(&actor.principal_id, &context, &root, token_budget)
            })
            .await??)
    }

    pub async fn add_project_note(
        &self,
        credential: &str,
        context: String,
        root: std::path::PathBuf,
        content: String,
        source: String,
    ) -> Result<(), ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        self.store
            .write(move |db| {
                db.add_context_project_note(&actor.principal_id, &context, &root, &content, &source)
            })
            .await??;
        Ok(())
    }

    /// Copy one stored source message into a destination discussion. The
    /// request cannot supply replacement text. The receipt carries provenance,
    /// not the message body.
    pub async fn publish_message(
        &self,
        credential: &str,
        source_context: String,
        source_session: String,
        source_seq: i64,
        destination_context: String,
        destination_session: String,
        request_id: String,
    ) -> Result<tetonic_memory::ContextPublication, ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        Ok(self
            .store
            .write(move |db| {
                db.publish_context_message(
                    &actor.principal_id,
                    &source_context,
                    &source_session,
                    source_seq,
                    &destination_context,
                    &destination_session,
                    &request_id,
                )
            })
            .await??)
    }

    pub async fn consolidate_project_memory(
        &self,
        credential: &str,
        context: String,
        session: String,
    ) -> Result<(), ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        self.store
            .write(move |db| {
                db.consolidate_context_session(&actor.principal_id, &context, &session)
            })
            .await??;
        Ok(())
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
