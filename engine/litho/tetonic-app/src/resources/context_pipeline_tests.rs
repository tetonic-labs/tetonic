use super::*;
use tetonic_context::{pipeline::ContextCompiler, types::*, WorkspaceContextProvider};
use tetonic_domain::{artifact::ArtifactStore, workspace::*, DataClass, RunId, SessionId, TaskId};

#[tokio::test]
async fn scoped_compiler_persists_retrievable_artifacts_and_revokes_future_use() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(
        workspace.join("canary.rs"),
        "fn scoped_canary() { /* PRIVATECOMPILERCANARY */ }\n",
    )
    .unwrap();
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
    let contexts = local.contexts();
    for context in ["private", "other"] {
        contexts
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
    contexts
        .open_history(alice.expose_secret(), "private".into(), "session".into())
        .await
        .unwrap();
    let raw: Arc<dyn ArtifactStore> = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            dir.path().join("artifacts"),
            tetonic_artifact::ScanPolicy::Scan(Arc::new(|_| false)),
        )
        .unwrap(),
    );
    let compiler = ContextCompiler::new(Arc::new(WorkspaceContextProvider::new(
        &workspace,
        None,
        None,
        crate::turn_execution::composition_fs_hooks(),
    )))
    .with_artifact_store(raw.clone());
    let compiler = contexts
        .bind_compiler(
            alice.expose_secret(),
            "private".into(),
            "session".into(),
            compiler,
        )
        .await
        .unwrap();
    let request = ContextRequest {
        session_id: SessionId::new("session"),
        run_id: RunId::new("run"),
        task_id: TaskId::new("task"),
        objective: "Implement scoped_canary in canary.rs".into(),
        workspace_version: WorkspaceVersion {
            repository_id: RepositoryId("fixture".into()),
            version_scheme: WorkspaceVersionScheme::Git,
            git_head: None,
            dirty_state_digest: ContentDigest("fixture".into()),
            tracked_state_digest: ContentDigest("fixture".into()),
            relevant_path_digests: Default::default(),
            index_generation: None,
        },
        data_class_ceiling: DataClass::Secret,
        allowed_paths: PathPolicy { rules: vec![] },
        excluded_paths: PathPolicy { rules: vec![] },
        token_budget: TokenBudget {
            max_tokens: 8000,
            safety_reserve: 200,
        },
        retrieval_profile: RetrievalProfile {
            version: "1".into(),
            max_candidates: 20,
        },
        prior_artifacts: vec![],
        trace_context: Default::default(),
    };
    let pack = compiler.compile(request.clone()).await.unwrap();
    assert!(pack
        .evidence
        .iter()
        .any(|e| e.text.contains("PRIVATECOMPILERCANARY")));
    let id = pack.stored_artifact_id.as_ref().unwrap();
    let authorized = contexts
        .bind_artifacts(alice.expose_secret(), "private".into(), raw.clone())
        .await
        .unwrap();
    let other = contexts
        .bind_artifacts(alice.expose_secret(), "other".into(), raw)
        .await
        .unwrap();
    assert!(other.open(id).await.is_err());
    let mut reader = authorized.open(id).await.unwrap();
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0; 1024];
        let n = reader.read_chunk(&mut chunk).await.unwrap();
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    let saved: ContextPack = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(saved.context_pack_id, pack.context_pack_id);
    assert!(saved
        .evidence
        .iter()
        .any(|e| e.text.contains("PRIVATECOMPILERCANARY")));
    local
        .resources()
        .set_organization_member(admin.expose_secret(), "org".into(), "alice".into(), None)
        .await
        .unwrap();
    assert!(authorized.open(id).await.is_err());
    assert!(compiler.compile(request).await.is_err());
    let handle = pack
        .expansion_handles
        .first()
        .expect("real workspace evidence must expose a bounded handle");
    assert!(compiler
        .expand_pack(&tetonic_domain::ContextExpansionRequest {
            handle_id: handle.handle_id.0.clone(),
            session_id: handle.session_id.clone(),
            run_id: handle.run_id.clone(),
            task_id: handle.task_id.clone(),
            current_workspace_fp: handle.workspace_version.state_fingerprint(),
        })
        .await
        .is_err());
}
