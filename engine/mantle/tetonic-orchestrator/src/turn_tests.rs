//! Integration tests for the orchestrated turn loop, spawn host, and LLM router.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use lokai_tools::{Tools, Workspace};
use serde_json::{json, Value};
use tetonic_core::{Agent, AgentConfig, Conversation};
use tetonic_domain::sinks::ActionBroker;
use tetonic_domain::{
    AgentInvocation, AttemptId, CandidateOutcome, CapabilityError, IssuedCapability, ProposedAction,
};
use tetonic_inference::{
    ChatRequest, ChatResponse, FabricCallMeta, InferenceError, InferenceProvider, Message, ToolCall,
};

fn wire_test_agent(agent: Agent) -> Agent {
    tetonic_runtime::wire_kernel_capability_helpers(
        agent,
        Arc::new(lokai_tools::format_post_edit_snapshot),
        Arc::new(|root, rel| {
            let abs = tetonic_transaction::fs_ops::resolve_under_root(root, rel).map_err(|_| ())?;
            std::fs::metadata(abs).map(|m| m.len()).map_err(|_| ())
        }),
        Arc::new(|root, paths| {
            tetonic_transaction::version::capture_workspace_version(root, paths)
                .map_err(|e| e.to_string())
        }),
    )
}
use tempfile::TempDir;

use crate::router::{RouteContext, RouteMode};
use crate::router_llm::llm_route_task;
use crate::run::{specialist_agent_config, OrchestrationMode, SpawnLimits};
use crate::spawn_host::SpawnHost;
use crate::spawn_session::SpawnSessionTrack;
use crate::specialist::{RoleId, TestCodingPack};
use crate::turn::{
    run_orchestrated_turn, run_spawned_specialist, AgentBuildRequest, ChildAdmit, ChildJob,
    OrchestratedTurnInput, RootExecute, ROOT_AGENT,
};
use tetonic_core::Step;

struct TestRootExecute;

#[async_trait::async_trait]
impl RootExecute for TestRootExecute {
    async fn execute(
        &self,
        _attempt_id: AttemptId,
        agent: &mut Agent,
        conversation: &mut Conversation,
        invocation: AgentInvocation,
        on_step: &mut (dyn FnMut(Step) + Send),
    ) -> CandidateOutcome {
        agent.turn(conversation, invocation, on_step).await
    }
}

/// Test stand-in for the app finalizer verify-fail note. Kernel no longer rejects finish.
struct AppVerifyFailNoteExecute;

#[async_trait::async_trait]
impl RootExecute for AppVerifyFailNoteExecute {
    async fn execute(
        &self,
        _attempt_id: AttemptId,
        agent: &mut Agent,
        conversation: &mut Conversation,
        invocation: AgentInvocation,
        on_step: &mut (dyn FnMut(Step) + Send),
    ) -> CandidateOutcome {
        let outcome = {
            let mut captured_step: Option<Step> = None;
            let mut step_wrapper = |step: Step| {
                captured_step = Some(step.clone());
                on_step(step);
            };
            agent
                .turn(conversation, invocation, &mut step_wrapper)
                .await
        };
        on_step(Step::Note("verify `verify`: FAILED".into()));
        outcome
    }
}

struct TestChildJob;

#[async_trait::async_trait]
impl ChildJob for TestChildJob {
    async fn admit_child(&self, req: ChildAdmit) -> Result<AttemptId, String> {
        Ok(AttemptId::new(format!("att_child_{}", req.agent_id)))
    }

    async fn complete_child(
        &self,
        _attempt_id: AttemptId,
        _outcome: CandidateOutcome,
    ) -> Result<(), String> {
        Ok(())
    }
}
use tetonic_core::SpawnRequest;

type AgentBuildFn =
    dyn FnMut(AgentBuildRequest, &str) -> Result<(Agent, AgentInvocation), String> + Send;

struct IssueAllBroker;

#[async_trait]
impl ActionBroker for IssueAllBroker {
    async fn evaluate_and_issue(
        &self,
        action: &ProposedAction,
    ) -> Result<IssuedCapability, CapabilityError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Ok(IssuedCapability {
            capability_id: tetonic_domain::CapabilityId::new("cap_orch_test"),
            session_id: action.session_id.clone(),
            run_id: action.run_id.clone(),
            task_id: action.task_id.clone(),
            attempt_id: action.attempt_id.clone(),
            agent_id: action.agent_id.clone(),
            action_kind: action.kind.clone(),
            canonical_parameter_digest: action.parameters.digest.clone(),
            workspace_version: action.workspace_version.clone(),
            data_classification: action.data_class,
            issuance_timestamp: now,
            expiration: now + 300,
            max_use_count: 8,
            current_use_count: 0,
            issuing_policy_version: "v2".into(),
            approval_record_id: None,
            revoked: false,
        })
    }
}

struct ScriptTurn {
    content: &'static str,
    calls: Vec<(&'static str, Value)>,
}

struct MockProvider {
    turns: Vec<ScriptTurn>,
    idx: AtomicUsize,
}

impl MockProvider {
    fn new(turns: Vec<ScriptTurn>) -> Self {
        Self {
            turns,
            idx: AtomicUsize::new(0),
        }
    }

    fn tool_call(name: &str, args: Value) -> ToolCall {
        ToolCall {
            function: tetonic_inference::FunctionCall {
                name: name.into(),
                arguments: args,
            },
        }
    }
}

#[async_trait]
impl InferenceProvider for MockProvider {
    async fn chat(
        &self,
        _req: ChatRequest,
        on_token: &mut tetonic_inference::TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        let i = self.idx.fetch_add(1, Ordering::Relaxed);
        let msg = match self.turns.get(i) {
            Some(t) => {
                let calls = t
                    .calls
                    .iter()
                    .map(|(n, a)| Self::tool_call(n, a.clone()))
                    .collect::<Vec<_>>();
                let m = Message::assistant(t.content);
                if calls.is_empty() {
                    m
                } else {
                    m.with_tool_calls(calls)
                }
            }
            None => Message::assistant("")
                .with_tool_calls(vec![Self::tool_call("finish", json!({"summary":"done"}))]),
        };
        if !msg.content.is_empty() {
            on_token(&msg.content);
        }
        Ok(ChatResponse {
            message: msg,
            usage: Default::default(),
            provenance: Default::default(),
        })
    }
}

struct TurnFixture {
    _dir: TempDir,
    ws_path: PathBuf,
    provider: Arc<MockProvider>,
}

impl TurnFixture {
    fn with_scripts(scripts: Vec<ScriptTurn>) -> Self {
        let dir = TempDir::new().unwrap();
        let ws_path = dir.path().to_path_buf();
        std::fs::write(ws_path.join("lib.rs"), "fn main() {}\n").unwrap();
        Self {
            provider: Arc::new(MockProvider::new(scripts)),
            ws_path,
            _dir: dir,
        }
    }

    fn build_agent(
        &self,
        build: AgentBuildRequest,
        user_input: &str,
    ) -> Result<(Agent, AgentInvocation), String> {
        let ws = Workspace::new(&self.ws_path).map_err(|e| e.to_string())?;
        let tools = Tools::new(ws, true);
        let provider = match build.role.as_ref().map(|r| r.as_str()) {
            Some("critic") => Arc::new(MockProvider::new(vec![ScriptTurn {
                content: "",
                calls: vec![("finish", json!({"summary": "REVISE: add edge-case tests"}))],
            }])),
            Some("coder")
                if build.agent_id.starts_with("a0_s")
                    && build.agent_id != "a0_s0"
                    && build
                        .agent_id
                        .rsplit("_s")
                        .next()
                        .and_then(|n| n.parse::<u32>().ok())
                        .is_some_and(|n| n >= 1) =>
            {
                Arc::new(MockProvider::new(vec![ScriptTurn {
                    content: "",
                    calls: vec![("finish", json!({"summary": "addressed feedback"}))],
                }]))
            }
            _ => self.provider.clone(),
        };
        let mut config = AgentConfig {
            agent_id: build.agent_id.clone(),
            max_steps: build.max_steps.unwrap_or(12),
            ..AgentConfig::default()
        };
        if let Some(r) = &build.role {
            config = specialist_agent_config(&config, &TestCodingPack, r, &build.agent_id);
            if let Some(ms) = build.max_steps {
                config.max_steps = ms;
            }
        }
        config.workspace_root = Some(tools.workspace().root().to_path_buf());
        let invocation = AgentInvocation {
            instructions: "You are a test agent in the workspace.".into(),
            user_input: user_input.to_string(),
            explain_turn: config.explain_turn || build.explain_turn,
            empty_tool_nudge: false,
            max_steps: config.max_steps,
            completion_tool: "finish".into(),
            discipline: tetonic_domain::LoopDiscipline::default(),
        };
        Ok((
            wire_test_agent(
                Agent::new(provider, tools, config).with_action_broker(Arc::new(IssueAllBroker)),
            ),
            invocation,
        ))
    }
}

fn edit_then_finish_scripts() -> Vec<ScriptTurn> {
    vec![
        ScriptTurn {
            content: "",
            calls: vec![(
                "edit_file",
                json!({
                    "path": "lib.rs",
                    "old_string": "fn main() {}",
                    "new_string": "fn main() { println!(\"hi\"); }"
                }),
            )],
        },
        ScriptTurn {
            content: "",
            calls: vec![("finish", json!({"summary": "implemented change"}))],
        },
        ScriptTurn {
            content: "",
            calls: vec![("finish", json!({"summary": "implemented change"}))],
        },
    ]
}

#[tokio::test]
async fn orchestrated_turn_runs_critic_and_revision_on_specialist_route() {
    let fx = TurnFixture::with_scripts(edit_then_finish_scripts());
    let mut conversation = Conversation::new();
    let spawn_serial = Arc::new(AtomicU32::new(0));
    let turn_spawn_count = Arc::new(AtomicU32::new(0));
    let builds: Arc<std::sync::Mutex<Vec<AgentBuildRequest>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let builds2 = builds.clone();
    let ws_path = fx.ws_path.clone();

    let outcome = run_orchestrated_turn(
        &mut conversation,
        &OrchestratedTurnInput {
            user_text: "implement the greeting in lib.rs",
            orchestration: OrchestrationMode::Auto,
            critic_enabled: true,
            verify_gated: true,
            workspace_root: &ws_path,
            index_db: None,
            code_index: None,
            pack: &TestCodingPack,
            llm_route: None,
            session_prefers_hard: false,
            spawn_limits: SpawnLimits::default(),
            session_max_steps: 16,
            root_attempt_id: AttemptId::new("att_test_root"),
        },
        spawn_serial,
        turn_spawn_count,
        move |build, user_input| {
            builds2.lock().unwrap().push(build.clone());
            fx.build_agent(build, user_input)
        },
        |_aid, _step| {},
        Some(|_decision, _tier| {}),
        AppVerifyFailNoteExecute,
        TestChildJob,
    )
    .await
    .expect("turn");

    assert!(matches!(
        outcome.route.mode,
        RouteMode::Specialist(ref r) if r.as_str() == "coder"
    ));
    assert!(outcome.critic_ran);
    assert!(outcome.revision_ran);
    let roles: Vec<_> = builds
        .lock()
        .unwrap()
        .iter()
        .filter_map(|b| b.role.clone())
        .collect();
    assert!(roles.contains(&RoleId::new("coder")));
    assert!(roles.contains(&RoleId::new("critic")));
}

#[tokio::test]
async fn orchestrated_turn_runs_critic_on_root_single_route() {
    let fx = TurnFixture::with_scripts(edit_then_finish_scripts());
    let mut conversation = Conversation::new();
    let spawn_serial = Arc::new(AtomicU32::new(0));
    let turn_spawn_count = Arc::new(AtomicU32::new(0));
    let builds: Arc<std::sync::Mutex<Vec<AgentBuildRequest>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let builds2 = builds.clone();
    let ws_path = fx.ws_path.clone();

    let outcome = run_orchestrated_turn(
        &mut conversation,
        &OrchestratedTurnInput {
            user_text: "ambiguous task about the repo",
            orchestration: OrchestrationMode::Auto,
            critic_enabled: true,
            verify_gated: true,
            workspace_root: &ws_path,
            index_db: None,
            code_index: None,
            pack: &TestCodingPack,
            llm_route: None,
            session_prefers_hard: false,
            spawn_limits: SpawnLimits::default(),
            session_max_steps: 16,
            root_attempt_id: AttemptId::new("att_test_root"),
        },
        spawn_serial,
        turn_spawn_count,
        move |build, user_input| {
            builds2.lock().unwrap().push(build.clone());
            fx.build_agent(build, user_input)
        },
        |_aid, _step| {},
        Some(|_decision, _tier| {}),
        AppVerifyFailNoteExecute,
        TestChildJob,
    )
    .await
    .expect("turn");

    assert!(matches!(outcome.route.mode, RouteMode::Single));
    assert_eq!(outcome.primary_agent_id, ROOT_AGENT);
    assert!(outcome.critic_ran);
    assert!(builds
        .lock()
        .unwrap()
        .iter()
        .any(|b| b.role.as_ref().map(|r| r.as_str()) == Some("critic")));
}

#[tokio::test]
async fn orchestrated_turn_skips_critic_when_verify_not_gated() {
    let scripts = vec![
        ScriptTurn {
            content: "",
            calls: vec![(
                "edit_file",
                json!({
                    "path": "lib.rs",
                    "old_string": "fn main() {}",
                    "new_string": "fn main() { 1 }"
                }),
            )],
        },
        ScriptTurn {
            content: "",
            calls: vec![("finish", json!({"summary": "done"}))],
        },
    ];
    let fx = TurnFixture::with_scripts(scripts);
    let mut conversation = Conversation::new();
    let ws_path = fx.ws_path.clone();

    let outcome = run_orchestrated_turn(
        &mut conversation,
        &OrchestratedTurnInput {
            user_text: "implement tweak",
            orchestration: OrchestrationMode::Auto,
            critic_enabled: true,
            verify_gated: false,
            workspace_root: &ws_path,
            index_db: None,
            code_index: None,
            pack: &TestCodingPack,
            llm_route: None,
            session_prefers_hard: false,
            spawn_limits: SpawnLimits::default(),
            session_max_steps: 8,
            root_attempt_id: AttemptId::new("att_test_root"),
        },
        Arc::new(AtomicU32::new(0)),
        Arc::new(AtomicU32::new(0)),
        |build, user_input| fx.build_agent(build, user_input),
        |_aid, _step| {},
        Some(|_decision, _tier| {}),
        TestRootExecute,
        TestChildJob,
    )
    .await
    .expect("turn");

    assert!(!outcome.critic_ran);
    assert!(!outcome.revision_ran);
}

#[tokio::test]
async fn run_spawned_specialist_rolls_back_and_returns_handoff() {
    let fx = TurnFixture::with_scripts(vec![ScriptTurn {
        content: "",
        calls: vec![("finish", json!({"summary": "spawn child done"}))],
    }]);
    let mut conversation = Conversation::new();
    let before = conversation.len();

    let outcome = run_spawned_specialist(
        &mut conversation,
        SpawnRequest {
            parent_agent_id: ROOT_AGENT.to_string(),
            role: "coder".into(),
            task: "fix lib.rs".into(),
        },
        Arc::new(AtomicU32::new(0)),
        Arc::new(AtomicU32::new(0)),
        SpawnLimits::default(),
        |build, user_input| fx.build_agent(build, user_input),
        |_aid, _step| {},
        None,
        RoleId::new("coder"),
        None,
        "sess_test".into(),
        None,
        TestChildJob,
        TestRootExecute,
        None,
    )
    .await;

    assert!(outcome.ok);
    assert!(outcome.content.contains("<untrusted spawn_handoff>"));
    assert!(outcome.content.contains("spawn_child_untrusted"));
    assert_eq!(conversation.len(), before);
}

#[tokio::test]
async fn spawn_host_dispatch_carves_budget_and_respects_limits() {
    let fx = TurnFixture::with_scripts(vec![ScriptTurn {
        content: "",
        calls: vec![("finish", json!({"summary": "nested spawn ok"}))],
    }]);
    let turn_spawn_count = Arc::new(AtomicU32::new(0));
    let spawn_serial = Arc::new(AtomicU32::new(0));
    let session_steps = Arc::new(AtomicUsize::new(8));
    let carved: Arc<std::sync::Mutex<Vec<usize>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let carved2 = carved.clone();
    let ws_path = fx.ws_path.clone();
    let provider = fx.provider.clone();
    let build: Arc<std::sync::Mutex<AgentBuildFn>> = Arc::new(std::sync::Mutex::new(
        move |build: AgentBuildRequest, user_input: &str| {
            if let Some(ms) = build.max_steps {
                carved2.lock().unwrap().push(ms);
            }
            let ws = Workspace::new(&ws_path).map_err(|e| e.to_string())?;
            let tools = Tools::new(ws, true);
            let mut config = AgentConfig {
                agent_id: build.agent_id,
                max_steps: build.max_steps.unwrap_or(8),
                ..AgentConfig::default()
            };
            config.workspace_root = Some(tools.workspace().root().to_path_buf());
            let invocation = AgentInvocation {
                instructions: "You are a test agent in the workspace.".into(),
                user_input: user_input.to_string(),
                explain_turn: false,
                empty_tool_nudge: false,
                max_steps: config.max_steps,
                completion_tool: "finish".into(),
                discipline: tetonic_domain::LoopDiscipline::default(),
            };
            Ok((
                wire_test_agent(
                    Agent::new(provider.clone(), tools, config)
                        .with_action_broker(Arc::new(IssueAllBroker)),
                ),
                invocation,
            ))
        },
    ));
    let host = Arc::new(SpawnHost {
        limits: SpawnLimits {
            max_per_turn: 1,
            max_depth: 2,
        },
        turn_spawn_count: turn_spawn_count.clone(),
        spawn_serial: spawn_serial.clone(),
        session_max_steps: session_steps.clone(),
        build: build.clone(),
        on_step: Arc::new(std::sync::Mutex::new(
            Box::new(|_aid: &str, _step: tetonic_core::Step| {})
                as Box<dyn for<'a> FnMut(&'a str, tetonic_core::Step) + Send>,
        )),
        spawn_track: SpawnSessionTrack::new(),
        session_id: "sess_test".into(),
        admit_spawn: None,
        pack: TestCodingPack::arc(),
        child_job: Arc::new(TestChildJob),
        root_execute: Arc::new(TestRootExecute),
    });

    let mut conversation = Conversation::new();
    let ok = host
        .dispatch(
            SpawnRequest {
                parent_agent_id: ROOT_AGENT.to_string(),
                role: "coder".into(),
                task: "subtask".into(),
            },
            &mut conversation,
        )
        .await;
    assert!(ok.ok);
    assert_eq!(turn_spawn_count.load(Ordering::SeqCst), 1);
    assert_eq!(carved.lock().unwrap()[0], 8);

    let exhausted = host
        .dispatch(
            SpawnRequest {
                parent_agent_id: ROOT_AGENT.to_string(),
                role: "coder".into(),
                task: "second spawn".into(),
            },
            &mut conversation,
        )
        .await;
    assert!(!exhausted.ok);
    assert!(exhausted.summary.contains("budget exhausted"));
}

struct LlmMock {
    content: String,
    fail: bool,
}

#[async_trait]
impl InferenceProvider for LlmMock {
    async fn chat(
        &self,
        _req: ChatRequest,
        _on_token: &mut tetonic_inference::TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        if self.fail {
            return Err(InferenceError::Provider("router down".into()));
        }
        Ok(ChatResponse {
            message: Message::assistant(&self.content),
            usage: Default::default(),
            provenance: Default::default(),
        })
    }
}

#[tokio::test]
async fn llm_route_task_uses_provider_when_available() {
    let provider = LlmMock {
        content: "CODER: implement handler".into(),
        fail: false,
    };
    let decision = llm_route_task(
        &provider,
        "router-model",
        "implement the handler",
        true,
        RouteContext {
            workspace_root: std::path::Path::new("."),
            index_db: None,
            code_index: None,
            pack: &TestCodingPack,
        },
        FabricCallMeta::default(),
    )
    .await;
    assert!(matches!(
        decision.mode,
        RouteMode::Specialist(ref r) if r.as_str() == "coder"
    ));
    assert_eq!(decision.source, crate::router::RouteSource::Llm);
}

#[tokio::test]
async fn llm_route_task_falls_back_on_provider_error() {
    let provider = LlmMock {
        content: String::new(),
        fail: true,
    };
    let decision = llm_route_task(
        &provider,
        "router-model",
        "implement the handler",
        true,
        RouteContext {
            workspace_root: std::path::Path::new("."),
            index_db: None,
            code_index: None,
            pack: &TestCodingPack,
        },
        FabricCallMeta::default(),
    )
    .await;
    assert!(matches!(
        decision.mode,
        RouteMode::Specialist(ref r) if r.as_str() == "coder"
    ));
    assert_eq!(decision.source, crate::router::RouteSource::Keyword);
}

struct StopCriticExecute {
    calls: AtomicUsize,
    stop: CandidateOutcome,
}
#[async_trait]
impl RootExecute for StopCriticExecute {
    async fn execute(
        &self,
        attempt: AttemptId,
        agent: &mut Agent,
        conversation: &mut Conversation,
        invocation: AgentInvocation,
        on_step: &mut (dyn FnMut(Step) + Send),
    ) -> CandidateOutcome {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
            // Even an approval-looking summary must not override incomplete execution.
            on_step(Step::Stopped("finished: APPROVE".into()));
            return self.stop.clone();
        }
        AppVerifyFailNoteExecute
            .execute(attempt, agent, conversation, invocation, on_step)
            .await
    }
}

#[tokio::test]
async fn incomplete_critic_cannot_become_a_successful_turn() {
    for stop in [
        CandidateOutcome::Canceled {
            reason: "interrupted".into(),
        },
        CandidateOutcome::Limited {
            kind: tetonic_domain::LimitKind::EffortCap,
            message: "limit".into(),
        },
    ] {
        let fx = TurnFixture::with_scripts(edit_then_finish_scripts());
        let mut conversation = Conversation::new();
        let spawn_serial = Arc::new(AtomicU32::new(0));
        let turn_spawn_count = Arc::new(AtomicU32::new(0));
        let builds: Arc<std::sync::Mutex<Vec<AgentBuildRequest>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let builds2 = builds.clone();
        let ws_path = fx.ws_path.clone();

        let outcome = run_orchestrated_turn(
            &mut conversation,
            &OrchestratedTurnInput {
                user_text: "ambiguous task about the repo",
                orchestration: OrchestrationMode::Auto,
                critic_enabled: true,
                verify_gated: true,
                workspace_root: &ws_path,
                index_db: None,
                code_index: None,
                pack: &TestCodingPack,
                llm_route: None,
                session_prefers_hard: false,
                spawn_limits: SpawnLimits::default(),
                session_max_steps: 16,
                root_attempt_id: AttemptId::new("att_test_root"),
            },
            spawn_serial,
            turn_spawn_count,
            move |build, user_input| {
                builds2.lock().unwrap().push(build.clone());
                fx.build_agent(build, user_input)
            },
            |_aid, _step| {},
            Some(|_decision, _tier| {}),
            StopCriticExecute {
                calls: AtomicUsize::new(0),
                stop: stop.clone(),
            },
            TestChildJob,
        )
        .await
        .expect("turn");

        assert!(outcome.critic_ran);
        assert!(!outcome.revision_ran);
        assert_eq!(outcome.outcome, stop);
    }
}
