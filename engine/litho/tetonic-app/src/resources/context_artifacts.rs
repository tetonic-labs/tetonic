use super::*;
use tetonic_domain::{artifact::*, ArtifactId};

fn denied() -> ArtifactError {
    ArtifactError::Internal("artifact access denied".into())
}
fn unavailable() -> ArtifactError {
    ArtifactError::Internal("artifact storage unavailable".into())
}

#[derive(Clone)]
pub(super) struct ScopedArtifacts {
    pub(super) store: SharedStore,
    pub(super) actor: String,
    pub(super) credential: super::credential_binding::BoundCredential,
    pub(super) context: String,
    pub(super) inner: Arc<dyn ArtifactStore>,
}
impl ScopedArtifacts {
    async fn authorize(&self, id: Option<&ArtifactId>) -> Result<(), ArtifactError> {
        self.credential
            .verify(&self.actor)
            .await
            .map_err(|_| denied())?;
        let (actor, context, id) = (
            self.actor.clone(),
            self.context.clone(),
            id.map(|i| i.0.clone()),
        );
        let allowed = self
            .store
            .read(move |db| match id {
                Some(id) => db.context_artifact_access(&actor, &context, &id),
                None => db.context_access(&actor, &context),
            })
            .await
            .map_err(|_| denied())?
            .map_err(|_| denied())?;
        if allowed {
            Ok(())
        } else {
            Err(denied())
        }
    }
}

impl ContextService {
    /// Trusted composition. The backing store and database form one storage
    /// namespace. Membership permits content use, not acceptance or deletion.
    pub async fn bind_artifacts(
        &self,
        credential: &str,
        context: String,
        inner: Arc<dyn ArtifactStore>,
    ) -> Result<Arc<dyn ArtifactStore>, ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        let scoped = ScopedArtifacts {
            store: self.store.clone(),
            actor: actor.principal_id,
            credential: super::credential_binding::BoundCredential::new(
                self.verifier.clone(),
                credential,
            ),
            context,
            inner,
        };
        scoped
            .authorize(None)
            .await
            .map_err(|_| ResourceError::Denied)?;
        Ok(Arc::new(scoped))
    }
}

#[async_trait]
impl ArtifactStore for ScopedArtifacts {
    async fn begin_write(
        &self,
        declaration: ArtifactDeclaration,
    ) -> Result<Box<dyn ArtifactWriter>, ArtifactError> {
        self.authorize(None).await?;
        let inner = self
            .inner
            .begin_write(declaration)
            .await
            .map_err(|_| unavailable())?;
        Ok(Box::new(ScopedWriter {
            scope: self.clone(),
            inner,
        }))
    }
    async fn open(&self, id: &ArtifactId) -> Result<Box<dyn ArtifactReader>, ArtifactError> {
        self.authorize(Some(id)).await?;
        let inner = self.inner.open(id).await.map_err(|_| unavailable())?;
        self.authorize(Some(id)).await?;
        Ok(Box::new(ScopedReader {
            scope: self.clone(),
            id: id.clone(),
            inner,
        }))
    }
    async fn metadata(&self, id: &ArtifactId) -> Result<ArtifactMetadata, ArtifactError> {
        self.authorize(Some(id)).await?;
        let result = self.inner.metadata(id).await.map_err(|_| unavailable())?;
        self.authorize(Some(id)).await?;
        Ok(result)
    }
    async fn mark_accepted(&self, _id: &ArtifactId) -> Result<ArtifactMetadata, ArtifactError> {
        // Acceptance belongs to the managed execution authority, not membership.
        Err(denied())
    }
    async fn delete(&self, _id: &ArtifactId) -> Result<(), ArtifactError> {
        // Retention/deletion authority is deliberately separate from read grants.
        Err(denied())
    }
}
struct ScopedWriter {
    scope: ScopedArtifacts,
    inner: Box<dyn ArtifactWriter>,
}
#[async_trait]
impl ArtifactWriter for ScopedWriter {
    async fn write_chunk(&mut self, data: &[u8]) -> Result<(), ArtifactError> {
        self.scope.authorize(None).await?;
        self.inner
            .write_chunk(data)
            .await
            .map_err(|_| unavailable())
    }
    async fn seal(self: Box<Self>) -> Result<ArtifactMetadata, ArtifactError> {
        self.scope.authorize(None).await?;
        let meta = self.inner.seal().await.map_err(|_| unavailable())?;
        let (actor, context, id) = (
            self.scope.actor.clone(),
            self.scope.context.clone(),
            meta.artifact_id.0.clone(),
        );
        // A crash or denial here leaves an unbound object, inaccessible through
        // this adapter. No fallback to legacy reads and no ownership guessing.
        self.scope
            .store
            .write(move |db| db.bind_new_context_artifact(&actor, &context, &id))
            .await
            .map_err(|_| denied())?
            .map_err(|_| denied())?;
        self.scope.authorize(Some(&meta.artifact_id)).await?;
        Ok(meta)
    }
    async fn abandon(self: Box<Self>) -> Result<(), ArtifactError> {
        // Cleanup of this writer's temporary object remains allowed on revocation.
        self.inner.abandon().await.map_err(|_| unavailable())
    }
}
struct ScopedReader {
    scope: ScopedArtifacts,
    id: ArtifactId,
    inner: Box<dyn ArtifactReader>,
}
#[async_trait]
impl ArtifactReader for ScopedReader {
    async fn read_chunk(&mut self, buf: &mut [u8]) -> Result<usize, ArtifactError> {
        self.scope.authorize(Some(&self.id)).await?;
        // Do not mutate the caller's buffer until post-read authorization succeeds.
        let mut scratch = vec![0; buf.len().min(64 * 1024)];
        let size = self
            .inner
            .read_chunk(&mut scratch)
            .await
            .map_err(|_| unavailable())?;
        self.scope.authorize(Some(&self.id)).await?;
        buf[..size].copy_from_slice(&scratch[..size]);
        Ok(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_artifact::{LocalArtifactStore, ScanPolicy};
    use tetonic_domain::{classify::DataClass, AttemptId, RunId, TaskId};

    fn declaration() -> ArtifactDeclaration {
        ArtifactDeclaration {
            kind: ArtifactKind::ContextPack,
            producer_run_id: RunId::new("run"),
            producer_task_id: TaskId::new("task"),
            producer_attempt_id: AttemptId::new("attempt"),
            worker_id: None,
            workspace_version: None,
            data_class: DataClass::Secret,
            retention_policy: RetentionPolicy::ProjectHistory,
        }
    }

    #[tokio::test]
    async fn real_artifacts_require_scope_and_recheck_open_handles() {
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
        for context in ["private", "other"] {
            service
                .create(
                    alice.expose_secret(),
                    context.into(),
                    ContextOwner::Private {
                        org_id: "org".into(),
                    },
                )
                .await
                .unwrap();
        }
        let raw: Arc<dyn ArtifactStore> = Arc::new(
            LocalArtifactStore::new(
                dir.path().join("artifacts"),
                ScanPolicy::Scan(Arc::new(|_| false)),
            )
            .unwrap(),
        );
        let scoped = service
            .bind_artifacts(alice.expose_secret(), "private".into(), raw.clone())
            .await
            .unwrap();
        let other = service
            .bind_artifacts(alice.expose_secret(), "other".into(), raw.clone())
            .await
            .unwrap();
        assert!(service
            .bind_artifacts(admin.expose_secret(), "private".into(), raw.clone())
            .await
            .is_err());
        let mut writer = scoped.begin_write(declaration()).await.unwrap();
        writer.write_chunk(b"PRIVATECANARY").await.unwrap();
        let meta = writer.seal().await.unwrap();
        assert!(other.open(&meta.artifact_id).await.is_err());
        assert!(other.metadata(&meta.artifact_id).await.is_err());
        assert!(scoped.mark_accepted(&meta.artifact_id).await.is_err());
        assert!(scoped.delete(&meta.artifact_id).await.is_err());
        let mut reader = scoped.open(&meta.artifact_id).await.unwrap();
        let mut bytes = [0; 64];
        let count = reader.read_chunk(&mut bytes).await.unwrap();
        assert_eq!(&bytes[..count], b"PRIVATECANARY");
        // An existing legacy object has no employee ownership, even with a known ID.
        let mut legacy = raw.begin_write(declaration()).await.unwrap();
        legacy.write_chunk(b"legacy").await.unwrap();
        let legacy = legacy.seal().await.unwrap();
        assert!(scoped.open(&legacy.artifact_id).await.is_err());
        let mut held = scoped.open(&meta.artifact_id).await.unwrap();
        let mut pending = scoped.begin_write(declaration()).await.unwrap();
        pending.write_chunk(b"pending").await.unwrap();
        local
            .credentials()
            .revoke(alice.credential_id.clone())
            .await
            .unwrap();
        // Membership still exists: only the bound credential has been revoked.
        let replacement = local
            .credentials()
            .issue("alice".into(), 3600)
            .await
            .unwrap();
        let renewed = service
            .bind_artifacts(replacement.expose_secret(), "private".into(), raw.clone())
            .await
            .unwrap();
        assert!(renewed.open(&meta.artifact_id).await.is_ok());
        let mut unchanged = [42; 64];
        assert!(held.read_chunk(&mut unchanged).await.is_err());
        assert_eq!(unchanged, [42; 64]);
        assert!(pending.write_chunk(b"more").await.is_err());
        assert!(pending.seal().await.is_err());
        assert!(scoped.metadata(&meta.artifact_id).await.is_err());
        local
            .resources()
            .set_organization_member(admin.expose_secret(), "org".into(), "alice".into(), None)
            .await
            .unwrap();
        assert!(renewed.open(&meta.artifact_id).await.is_err());
    }
}
