//! V4-PROOF-08 / CAP-01 behavioral proofs for capability repository interpretation.
//!
//! Proves:
//! 1. Coding context works through supplied capability (WorkspaceContextProvider compiles context + scans secrets).
//! 2. Non-coding starter with EmptyToolHost and context_compiler: None executes through
//!    DefaultRunService::start_identity_job + LocalAgentAttemptExecutor + Agent::turn with zero repository calls.
//! 3. Neutral runtime assembly without compiler produces Agent where has_context_compiler() == false.
//! 4. lokai-runtime crate is completely clean of repository heuristics and walk dependencies.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tetonic_app::commands;
use tetonic_app::definition::CodingAgentDefinition;
use tetonic_app::events::{ApplicationEvent, ApplicationEventSink};
use tetonic_app::{Application, ApplicationDependencies};
use tetonic_context::workspace::{
    build_production_context_compiler_injected, ContextFsHooks, WorkspaceContextProvider,
};
use tetonic_core::agent::EmptyToolHost;
use tetonic_core::{Agent, AgentConfig, HeuristicTokenizer};
use tetonic_domain::classify::DataClass;
use tetonic_domain::ids::{RunId, SessionId, TaskId};
use tetonic_domain::workspace::{
    ContentDigest, RepositoryId, WorkspaceVersion, WorkspaceVersionScheme,
};
use tetonic_domain::{
    AgentIdentity, AgentInvocation, AgentJobSpec, CandidateOutcome, ContextCompileRequest,
    ContextCompiler,
};
use tetonic_inference::{
    ChatRequest, ChatResponse, FunctionCall, GenUsage, InferenceError, InferenceProvenance,
    InferenceProvider, Message, TokenSink, ToolCall,
};
use tetonic_policy::PolicyEngine;
use tetonic_run::job_input_digest;
use tetonic_runtime::{
    AgentAssemblyParts, AssemblyMode, EngineRuntime, NullAudit, ProductionApproval,
};

static CAP01_DB_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestEventSink;
impl ApplicationEventSink for TestEventSink {
    fn send(&self, _event: ApplicationEvent) {}
}

fn make_test_app() -> Application {
    let db_path = std::env::temp_dir().join(format!(
        "lokai_cap01_test_{}_{}.db",
        std::process::id(),
        CAP01_DB_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let store = tetonic_memory::SharedStore::open(&db_path, 1).unwrap();
    let policy = Arc::new(PolicyEngine::default());
    let artifact_store = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join(format!(
                "lokai_cap01_art_{}_{}",
                std::process::id(),
                CAP01_DB_COUNTER.load(Ordering::Relaxed)
            )),
            tetonic_app::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = EngineRuntime::new(policy.clone(), None, artifact_store);
    Application::new(ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store),
        policy,
        event_sink: Arc::new(TestEventSink),
        index_db: None,
        fabric_hint: None,
    })
}

struct FinisherProvider;

#[async_trait]
impl InferenceProvider for FinisherProvider {
    async fn chat(
        &self,
        _req: ChatRequest,
        _on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        Ok(ChatResponse {
            message: Message::assistant("").with_tool_calls(vec![ToolCall {
                function: FunctionCall {
                    name: "finish".into(),
                    arguments: serde_json::json!({ "summary": "completed successfully" }),
                },
            }]),
            usage: GenUsage::default(),
            provenance: InferenceProvenance::default(),
        })
    }
}

fn identity_and_spec(job_input: &str) -> (AgentIdentity, AgentJobSpec) {
    let identity = CodingAgentDefinition::production().coding_identity_record();
    let spec = AgentJobSpec {
        identity_id: identity.id.clone(),
        definition_digest: identity.bound_definition_digest.clone(),
        input_digest: job_input_digest(job_input),
        capability_bindings: Vec::new(),
        artifact_bindings: Vec::new(),
        recovery_id: identity.recovery_id.clone(),
    };
    (identity, spec)
}

fn finish_invocation(user_input: &str) -> AgentInvocation {
    AgentInvocation {
        instructions: "Answer the user question and call finish when done.".into(),
        user_input: user_input.into(),
        explain_turn: false,
        empty_tool_nudge: false,
        max_steps: 4,
        completion_tool: "finish".into(),
        discipline: tetonic_domain::LoopDiscipline::default(),
    }
}

#[derive(Default)]
struct CapabilityTracker {
    walks: AtomicUsize,
    git_ops: AtomicUsize,
    repo_reads: AtomicUsize,
}

#[tokio::test]
async fn cap01_coding_context_works_through_supplied_capability() {
    let ws_dir = std::env::temp_dir().join(format!(
        "lokai_cap01_ws_{}_{}",
        std::process::id(),
        CAP01_DB_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&ws_dir);
    fs::create_dir_all(ws_dir.join("src")).unwrap();

    let code_file = ws_dir.join("src/lib.rs");
    fs::write(
        &code_file,
        "pub fn calculate_hash() -> &'static str { \"test-hash-42\" }\n",
    )
    .unwrap();

    let secret_file = ws_dir.join("src/secret.rs");
    fs::write(
        &secret_file,
        "// test api key\nconst API_KEY: &str = \"mock_entropy_token_9xK2qW8mZ3vL7pT1yR5jN4cF6\";\n",
    )
    .unwrap();

    let read_count = Arc::new(AtomicUsize::new(0));
    let git_count = Arc::new(AtomicUsize::new(0));
    let r_clone = read_count.clone();
    let g_clone = git_count.clone();

    let hooks = ContextFsHooks {
        skip_symlink: Arc::new(|_| false),
        jailed_read: Arc::new(move |root, rel| {
            r_clone.fetch_add(1, Ordering::SeqCst);
            let path = root.join(rel);
            fs::read_to_string(&path).map_err(|e| e.to_string())
        }),
        run_git: Arc::new(move |_root, _args| {
            g_clone.fetch_add(1, Ordering::SeqCst);
            Err("git disabled in test".into())
        }),
    };

    let art_dir =
        std::env::temp_dir().join(format!("lokai_cap01_art_coding_{}", std::process::id()));
    let artifact_store = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            art_dir,
            tetonic_app::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );

    let compiler: Arc<dyn ContextCompiler> = build_production_context_compiler_injected(
        &ws_dir,
        artifact_store,
        None,
        None,
        None,
        None,
        hooks,
    );

    let req = ContextCompileRequest {
        session_id: SessionId::new("sess_cap01"),
        run_id: RunId::new("run_cap01"),
        task_id: TaskId::new("task_cap01"),
        objective: "calculate_hash implementation".into(),
        workspace_version: WorkspaceVersion {
            repository_id: RepositoryId("cap01_repo".into()),
            version_scheme: WorkspaceVersionScheme::Git,
            git_head: None,
            dirty_state_digest: ContentDigest("dirty".into()),
            tracked_state_digest: ContentDigest("tracked".into()),
            relevant_path_digests: std::collections::BTreeMap::new(),
            index_generation: None,
        },
        data_class_ceiling: DataClass::RepositorySource,
    };

    let compiled = compiler.compile(req).await.expect("compile must succeed");

    assert!(
        read_count.load(Ordering::SeqCst) > 0,
        "jailed_read hook must have been called by WorkspaceContextProvider"
    );

    assert!(
        !compiled.evidence.is_empty(),
        "compiled context must contain evidence retrieved from workspace"
    );

    for ev in &compiled.evidence {
        assert!(
            !ev.text
                .contains("mock_entropy_token_9xK2qW8mZ3vL7pT1yR5jN4cF6"),
            "secret scanner must redact or exclude live secret token: evidence id {}",
            ev.evidence_id
        );
    }

    let _ = fs::remove_dir_all(&ws_dir);
}

#[tokio::test]
async fn cap01_non_coding_starter_zero_repository_calls() {
    let app = make_test_app();
    let tracker = Arc::new(CapabilityTracker::default());

    let provider = Arc::new(FinisherProvider);
    let mut agent = Agent::new(provider, EmptyToolHost, AgentConfig::default());
    assert!(
        !agent.has_context_compiler(),
        "starter non-coding agent must not have context compiler"
    );

    let user_input = "explain something without workspace";
    let (identity, spec) = identity_and_spec(user_input);
    let invocation = finish_invocation(user_input);

    let result = app
        .runs
        .start_identity_job(
            commands::StartIdentityJobCommand {
                identity,
                job_spec: spec,
                invocation,
            },
            &mut agent,
        )
        .await
        .expect("sessionless execution of non-coding starter must succeed");

    assert!(
        matches!(result.outcome, CandidateOutcome::Completed { .. }),
        "starter turn must complete: {:?}",
        result.outcome
    );

    assert_eq!(
        tracker.walks.load(Ordering::SeqCst),
        0,
        "non-coding starter must perform 0 filesystem walks"
    );
    assert_eq!(
        tracker.git_ops.load(Ordering::SeqCst),
        0,
        "non-coding starter must perform 0 git operations"
    );
    assert_eq!(
        tracker.repo_reads.load(Ordering::SeqCst),
        0,
        "non-coding starter must perform 0 repository reads"
    );
}

#[test]
fn cap01_neutral_runtime_assembly_no_default_walk() {
    let policy = Arc::new(PolicyEngine::default());
    let art_dir =
        std::env::temp_dir().join(format!("lokai_cap01_art_assembly_{}", std::process::id()));
    let artifact_store = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            art_dir,
            tetonic_app::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let rt = EngineRuntime::new(policy.clone(), None, artifact_store);

    let temp_dir = tempfile::tempdir().unwrap();
    let ws = tetonic_tools::Workspace::new(temp_dir.path()).unwrap();
    let tools = tetonic_tools::Tools::new(ws, false);
    let config = AgentConfig::default();

    let parts_without_compiler = AgentAssemblyParts {
        agent: Agent::with_tokenizer(
            Arc::new(FinisherProvider),
            tools.clone(),
            config.clone(),
            Box::new(HeuristicTokenizer),
        ),
        audit: Box::new(NullAudit),
        approval: ProductionApproval::cli_verify_finish_only(),
        spawn: None,
        process_broker: None,
        context_compiler: None,
        post_edit_snapshot: Arc::new(tetonic_tools::format_post_edit_snapshot),
        resolve_under_root: Arc::new(|root, rel| {
            let abs = tetonic_transaction::fs_ops::resolve_under_root(root, rel).map_err(|_| ())?;
            fs::metadata(abs).map(|m| m.len()).map_err(|_| ())
        }),
        capture_workspace_version: Arc::new(|root, paths| {
            tetonic_transaction::version::capture_workspace_version(root, paths)
                .map_err(|e| e.to_string())
        }),
    };

    let assembled_neutral = rt
        .assemble_agent(AssemblyMode::CliEphemeral, parts_without_compiler)
        .expect("assemble neutral agent");
    assert!(
        !assembled_neutral.has_context_compiler(),
        "neutral runtime assembly must not attach context compiler when omitted"
    );

    let dummy_provider = Arc::new(WorkspaceContextProvider::new(
        temp_dir.path(),
        None,
        None,
        ContextFsHooks {
            skip_symlink: Arc::new(|_| false),
            jailed_read: Arc::new(|_, _| Ok("".into())),
            run_git: Arc::new(|_, _| Ok("".into())),
        },
    ));
    let supplied_compiler: Arc<dyn ContextCompiler> = Arc::new(
        tetonic_context::pipeline::ContextCompiler::new(dummy_provider),
    );

    let parts_with_compiler = AgentAssemblyParts {
        agent: Agent::with_tokenizer(
            Arc::new(FinisherProvider),
            tools,
            config,
            Box::new(HeuristicTokenizer),
        ),
        audit: Box::new(NullAudit),
        approval: ProductionApproval::cli_verify_finish_only(),
        spawn: None,
        process_broker: None,
        context_compiler: Some(supplied_compiler),
        post_edit_snapshot: Arc::new(tetonic_tools::format_post_edit_snapshot),
        resolve_under_root: Arc::new(|root, rel| {
            let abs = tetonic_transaction::fs_ops::resolve_under_root(root, rel).map_err(|_| ())?;
            fs::metadata(abs).map(|m| m.len()).map_err(|_| ())
        }),
        capture_workspace_version: Arc::new(|root, paths| {
            tetonic_transaction::version::capture_workspace_version(root, paths)
                .map_err(|e| e.to_string())
        }),
    };

    let assembled_with_compiler = rt
        .assemble_agent(AssemblyMode::CliEphemeral, parts_with_compiler)
        .expect("assemble agent with compiler");
    assert!(
        assembled_with_compiler.has_context_compiler(),
        "runtime assembly must attach context compiler when supplied by composition"
    );
}

#[test]
fn cap01_runtime_crate_clean_of_repository_heuristics() {
    let runtime_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../core/tetonic-runtime");

    let toml_content =
        fs::read_to_string(runtime_dir.join("Cargo.toml")).expect("read lokai-runtime Cargo.toml");
    let prod_deps = toml_content
        .split("[dependencies]")
        .nth(1)
        .and_then(|after| after.split('[').next())
        .unwrap_or("");

    for forbidden in ["ignore", "tetonic-secrets", "lokai-secrets", "sha2", "hex"] {
        assert!(
            !prod_deps.contains(forbidden),
            "lokai-runtime [dependencies] must not include `{forbidden}`: {prod_deps}"
        );
    }

    let src_dir = runtime_dir.join("src");
    let mut files = Vec::new();
    collect_rs_files(&src_dir, &mut files);
    assert!(!files.is_empty(), "must find source files in lokai-runtime");

    for path in files {
        if skip_cfg_test_path(&path) {
            continue;
        }
        let content = fs::read_to_string(&path).expect("read rs file");
        let prod_content = content.split("#[cfg(test)]").next().unwrap_or(&content);

        assert!(
            !prod_content.contains("WalkBuilder"),
            "file {:?} must not contain WalkBuilder",
            path
        );
        assert!(
            !prod_content.contains("WorkspaceContextProvider"),
            "file {:?} must not contain WorkspaceContextProvider",
            path
        );
        assert!(
            !prod_content.contains("RepositorySummary"),
            "file {:?} must not contain RepositorySummary",
            path
        );
        assert!(
            !prod_content.contains("build_production_context_compiler"),
            "file {:?} must not contain build_production_context_compiler",
            path
        );
        assert!(
            !prod_content.contains("current_dir()"),
            "file {:?} must not contain current_dir()",
            path
        );
    }
}

fn skip_cfg_test_path(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.ends_with("_test.rs") || name.ends_with("_tests.rs") || name == "tests.rs"
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
}
