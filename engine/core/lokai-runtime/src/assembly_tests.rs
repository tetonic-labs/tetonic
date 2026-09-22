use std::sync::Arc;

use lokai_core::{Agent, AgentConfig, HeuristicTokenizer};
use lokai_inference::OllamaProvider;
use lokai_policy::PolicyEngine;
use lokai_tools::{Tools, Workspace};

use super::{AgentAssemblyParts, AssemblyMode, EngineRuntime};
use crate::approval::ProductionApproval;
use crate::NullAudit;

fn test_fs_hooks() -> lokai_context::workspace::ContextFsHooks {
    lokai_context::workspace::ContextFsHooks {
        skip_symlink: Arc::new(lokai_transaction::fs_ops::is_symlink_or_reparse),
        jailed_read: Arc::new(|root, rel| {
            let ws = lokai_tools::Workspace::new(root).map_err(|e| e.to_string())?;
            let path = ws.resolve(rel).map_err(|e| e.to_string())?;
            lokai_tools::read_to_string_nofollow(&path).map_err(|e| e.to_string())
        }),
        run_git: Arc::new(|root, args| {
            let pe = lokai_tools::coding_executor(root, lokai_tools::EnforcementLevel::Sandboxed);
            let r = pe.run_git(args.iter().map(|s| (*s).to_string()))?;
            Ok(r.output)
        }),
    }
}

struct PersistingAudit;
impl lokai_core::AuditSink for PersistingAudit {
    fn message(&self, _: &str, _: &str, _: Option<&str>) {}
    fn tool_call(&self, _: &str, _: &str, _: &str, _: bool, _: &str, _: Option<&str>) {}
    fn file_change(&self, _: &str, _: &str, _: &str, _: Option<&str>, _: Option<&str>) {}
    fn note(&self, _: &str) {}
}

fn host_approval() -> ProductionApproval {
    ProductionApproval::host(Arc::new(|_| Box::pin(async { true })))
}

fn test_capability_hooks() -> (
    lokai_core::PostEditSnapshot,
    lokai_core::ResolveUnderRoot,
    lokai_core::CaptureWorkspaceVersion,
) {
    (
        Arc::new(lokai_tools::format_post_edit_snapshot),
        Arc::new(|root, rel| {
            let abs = lokai_transaction::fs_ops::resolve_under_root(root, rel).map_err(|_| ())?;
            std::fs::metadata(abs).map(|m| m.len()).map_err(|_| ())
        }),
        Arc::new(|root, paths| {
            lokai_transaction::version::capture_workspace_version(root, paths)
                .map_err(|e| e.to_string())
        }),
    )
}

fn sample_parts(
    audit: Box<dyn lokai_core::AuditSink>,
    approval: ProductionApproval,
) -> (tempfile::TempDir, AgentAssemblyParts) {
    let _pe = Arc::new(PolicyEngine::default());
    let fixture = tempfile::tempdir().unwrap();
    let ws = Workspace::new(fixture.path()).unwrap();
    let _artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            fixture.path().join("artifacts_sample"),
            lokai_artifact::ScanPolicy::Refuse,
        )
        .unwrap(),
    );

    let tools = Tools::new(ws, false);
    let process_broker = Arc::new(tools.executor().clone());
    let guard = Arc::new(lokai_egress::EgressGuard::new());
    let provider = Arc::new(OllamaProvider::new("http://127.0.0.1:11434", guard));
    let config = AgentConfig::default();
    let agent = Agent::with_tokenizer(provider, tools, config, Box::new(HeuristicTokenizer));
    let (post_edit_snapshot, resolve_under_root, capture_workspace_version) =
        test_capability_hooks();
    (
        fixture,
        AgentAssemblyParts {
            agent,
            audit,
            approval,
            spawn: None,
            process_broker: Some(process_broker),
            context_compiler: None,
            post_edit_snapshot,
            resolve_under_root,
            capture_workspace_version,
        },
    )
}

#[test]
fn production_assembly_wires_policy_and_hooks() {
    let pe = Arc::new(PolicyEngine::default());
    let artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join("artifacts"),
            lokai_artifact::ScanPolicy::Refuse,
        )
        .unwrap(),
    );
    let rt = EngineRuntime::new(pe, None, artifact_store);
    let (_fixture, parts) = sample_parts(Box::new(PersistingAudit), host_approval());
    let agent = rt
        .assemble_agent(AssemblyMode::Session, parts)
        .expect("production assembly");
    assert!(
        !agent.has_context_compiler(),
        "CAP-01: neutral assemble_agent without supplied compiler installs None"
    );
}

#[test]
fn production_assembly_attaches_supplied_context_compiler() {
    struct MockCompiler;
    #[async_trait::async_trait]
    impl lokai_domain::ContextCompiler for MockCompiler {
        async fn compile(
            &self,
            _req: lokai_domain::ContextCompileRequest,
        ) -> Result<lokai_domain::CompiledContext, String> {
            Err("mock".into())
        }
    }

    let pe = Arc::new(PolicyEngine::default());
    let artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join("artifacts_mock"),
            lokai_artifact::ScanPolicy::Refuse,
        )
        .unwrap(),
    );
    let rt = EngineRuntime::new(pe, None, artifact_store);
    let (_fixture, mut parts) = sample_parts(Box::new(PersistingAudit), host_approval());
    parts.context_compiler = Some(Arc::new(MockCompiler));
    let agent = rt
        .assemble_agent(AssemblyMode::Session, parts)
        .expect("production assembly");
    assert!(
        agent.has_context_compiler(),
        "CAP-01: assemble_agent attaches supplied ContextCompiler"
    );
}

#[tokio::test]
async fn production_compiler_compile_uses_workspace_inputs() {
    use lokai_context::types::{ContextRequest, PathPolicy, RetrievalProfile, TokenBudget};
    use lokai_domain::classify::DataClass;
    use lokai_domain::ids::{RunId, SessionId, TaskId};

    let dir = std::env::temp_dir().join(format!("lokai-r41-asm-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("r41_evidence.rs"), "fn r41_compile_marker() {}\n").unwrap();

    let pe = Arc::new(PolicyEngine::default());
    let artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            dir.join("artifacts"),
            lokai_artifact::ScanPolicy::Scan(Arc::new(|_| false)),
        )
        .unwrap(),
    );
    let compiler = lokai_context::workspace::build_production_context_compiler(
        &dir,
        artifact_store,
        None,
        None,
        test_fs_hooks(),
    );
    let wv = lokai_transaction::version::capture_workspace_version(&dir, &[]).unwrap();
    let pack = compiler
        .compile(ContextRequest {
            session_id: SessionId::new("s"),
            run_id: RunId::new("r"),
            task_id: TaskId::new("t"),
            objective: "find r41_compile_marker".into(),
            workspace_version: wv,
            data_class_ceiling: DataClass::RepositorySource,
            allowed_paths: PathPolicy { rules: vec![] },
            excluded_paths: PathPolicy { rules: vec![] },
            token_budget: TokenBudget {
                max_tokens: 4_000,
                safety_reserve: 200,
            },
            retrieval_profile: RetrievalProfile {
                version: "1.0".into(),
                max_candidates: 20,
            },
            prior_artifacts: vec![],
            trace_context: Default::default(),
        })
        .await
        .expect("compile");
    assert!(
        pack.evidence
            .iter()
            .any(|e| e.text.contains("r41_compile_marker")
                || e.repository_path
                    .as_deref()
                    .is_some_and(|p| p.contains("r41_evidence"))),
        "expected real workspace evidence, got {:?}",
        pack.evidence
            .iter()
            .map(|e| (
                e.repository_path.clone(),
                e.text.chars().take(80).collect::<String>()
            ))
            .collect::<Vec<_>>()
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = pe;
}

#[test]
fn session_mode_rejects_null_audit() {
    let pe = Arc::new(PolicyEngine::default());
    let artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join("artifacts"),
            lokai_artifact::ScanPolicy::Refuse,
        )
        .unwrap(),
    );
    let rt = EngineRuntime::new(pe, None, artifact_store);
    let (_fixture, parts) = sample_parts(Box::new(NullAudit), host_approval());
    assert!(matches!(
        rt.assemble_agent(AssemblyMode::Session, parts),
        Err(super::AssemblyError::MissingAudit)
    ));
}

#[test]
fn session_mode_rejects_allow_all_approval() {
    let pe = Arc::new(PolicyEngine::default());
    let artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join("artifacts"),
            lokai_artifact::ScanPolicy::Refuse,
        )
        .unwrap(),
    );
    let rt = EngineRuntime::new(pe, None, artifact_store);
    let (_fixture, parts) =
        sample_parts(Box::new(PersistingAudit), ProductionApproval::allow_all());
    assert!(matches!(
        rt.assemble_agent(AssemblyMode::Session, parts),
        Err(super::AssemblyError::MissingApproval)
    ));
}

#[test]
fn cli_ephemeral_allows_null_audit() {
    let pe = Arc::new(PolicyEngine::default());
    let artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join("artifacts"),
            lokai_artifact::ScanPolicy::Refuse,
        )
        .unwrap(),
    );
    let rt = EngineRuntime::new(pe, None, artifact_store);
    let (_fixture, parts) = sample_parts(
        Box::new(NullAudit),
        ProductionApproval::cli_verify_finish_only(),
    );
    rt.assemble_agent(AssemblyMode::CliEphemeral, parts)
        .unwrap();
}

#[test]
fn tools_new_defaults_sandboxed() {
    let fixture = tempfile::tempdir().unwrap();
    let ws = Workspace::new(fixture.path()).unwrap();
    let tools = Tools::new(ws, false);
    assert_eq!(
        tools.enforcement_level(),
        lokai_tools::EnforcementLevel::Sandboxed
    );
}
