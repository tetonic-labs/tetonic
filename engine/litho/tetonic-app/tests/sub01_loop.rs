//! SUB-01 loop ownership pins. Absence/inventory only; not ESTABLISHED. Not GATE-001.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tetonic_app::coding_pack::CodingPack;
use tetonic_app::definition::CodingAgentDefinition;
use tetonic_core::{Agent, AgentConfig, Conversation, Step};
use tetonic_domain::{CandidateOutcome, CompletionKind};
use tetonic_inference::{
    ChatRequest, ChatResponse, FabricSnapshot, InferenceError, InferenceProvider, Message,
    TokenSink,
};
use tetonic_orchestrator::SpecialistPack;
use tetonic_tools::{Tools, Workspace};

fn production() -> CodingAgentDefinition {
    CodingAgentDefinition::production()
}

fn engine_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).expect("read")
}

fn production_core_src(rel: &str) -> String {
    let src = read(engine_root().join("core/tetonic-core/src").join(rel));
    let mut out = String::new();
    for line in src.lines() {
        if line.trim().starts_with("#[cfg(test)]") {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[test]
fn sub01_instructions_compile_from_product() {
    let dir = tempfile::tempdir().unwrap();
    let tools = Tools::new(Workspace::new(dir.path()).unwrap(), false);
    let overlay = production().overlay(&tetonic_orchestrator::RoleId::new("planner"));
    let cfg = AgentConfig {
        system_overlay: Some(overlay.clone()),
        briefing: Some("session briefing".into()),
        project_context: Some("project notes".into()),
        ..AgentConfig::default()
    };
    let inv = production().compile_invocation_from_tools(&tools, &cfg, "plan the change");
    assert!(inv.instructions.contains("Investigate before editing"));
    assert!(inv.instructions.contains("session briefing"));
    assert!(inv.instructions.contains("project notes"));
    assert!(inv.instructions.contains("**planner**"));
    assert_eq!(inv.completion_tool, "finish");
}

#[test]
fn sub01_single_door_explain_compiles_explain_turn() {
    let def = production();
    assert!(def.root_explain_turn("what does this function do?"));
    assert!(def.root_explain_turn("explain the parser"));
    assert!(!def.root_explain_turn("implement slugify"));
    assert_eq!(
        CodingPack.root_explain_turn("what does this function do?"),
        def.root_explain_turn("what does this function do?")
    );

    let dir = tempfile::tempdir().unwrap();
    let tools = Tools::new(Workspace::new(dir.path()).unwrap(), false);
    let mut cfg = AgentConfig {
        explain_turn: def.root_explain_turn("what does this function do?"),
        ..AgentConfig::default()
    };
    let explain = def.compile_invocation_from_tools(&tools, &cfg, "what does this function do?");
    assert!(explain.explain_turn);
    cfg.explain_turn = def.root_explain_turn("implement slugify");
    let implement = def.compile_invocation_from_tools(&tools, &cfg, "implement slugify");
    assert!(!implement.explain_turn);
}

#[test]
fn sub01_single_door_explain_does_not_verify_or_commit() {
    let dir = tempfile::tempdir().unwrap();
    let tools = Tools::new(Workspace::new(dir.path()).unwrap(), false);
    let cfg = AgentConfig {
        explain_turn: true,
        ..AgentConfig::default()
    };
    let inv =
        production().compile_invocation_from_tools(&tools, &cfg, "what does this function do?");
    assert!(inv.explain_turn);
    assert!(!inv.empty_tool_nudge);
    let _ = tools;
}

#[test]
fn sub01_empty_tool_nudge_compiles_from_product() {
    let def = production();
    let dir = tempfile::tempdir().unwrap();
    let tools = Tools::new(Workspace::new(dir.path()).unwrap(), false);

    let action = AgentConfig {
        explain_turn: false,
        ..AgentConfig::default()
    };
    let implement = def.compile_invocation_from_tools(&tools, &action, "implement slugify");
    assert!(implement.empty_tool_nudge);
    let fix = def.compile_invocation_from_tools(&tools, &action, "Fix the bug in mathx.py");
    assert!(fix.empty_tool_nudge);

    let question_cfg = AgentConfig {
        explain_turn: def.root_explain_turn("What does this function do?"),
        ..AgentConfig::default()
    };
    let question =
        def.compile_invocation_from_tools(&tools, &question_cfg, "What does this function do?");
    assert!(question.explain_turn);
    assert!(!question.empty_tool_nudge);

    let forced_cfg = AgentConfig {
        explain_turn: true,
        ..AgentConfig::default()
    };
    let forced = def.compile_invocation_from_tools(&tools, &forced_cfg, "implement slugify");
    assert!(!forced.empty_tool_nudge);

    let investigative_cfg = AgentConfig {
        explain_turn: false,
        ..AgentConfig::default()
    };
    let investigative = def.compile_invocation_from_tools(
        &tools,
        &investigative_cfg,
        "look at the parser structure",
    );
    assert!(!investigative.explain_turn);
    assert!(
        !investigative.empty_tool_nudge,
        "must not compile empty_tool_nudge as !explain_turn alone"
    );
}

struct ScriptProvider {
    calls: Vec<(&'static str, serde_json::Value)>,
    requests: Mutex<Vec<ChatRequest>>,
}

impl ScriptProvider {
    fn new(calls: Vec<(&'static str, serde_json::Value)>) -> Self {
        Self {
            calls,
            requests: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl InferenceProvider for ScriptProvider {
    async fn fabric_snapshot(&self) -> FabricSnapshot {
        FabricSnapshot {
            nodes: vec![],
            effective_concurrency: 1,
            generated_at: chrono::Utc::now(),
        }
    }

    async fn chat(
        &self,
        req: ChatRequest,
        _on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        self.requests.lock().unwrap().push(req);
        Ok(ChatResponse {
            message: Message::assistant("").with_tool_calls(
                self.calls
                    .iter()
                    .map(|(name, args)| tetonic_inference::ToolCall {
                        function: tetonic_inference::FunctionCall {
                            name: (*name).into(),
                            arguments: args.clone(),
                        },
                    })
                    .collect(),
            ),
            usage: Default::default(),
            provenance: Default::default(),
        })
    }
}

fn test_tools() -> Tools {
    let dir = tempfile::tempdir().unwrap();
    // leak dir so Tools outlives the TempDir handle for the test body
    let path = dir.keep();
    Tools::new(Workspace::new(path).unwrap(), false)
}

#[tokio::test]
async fn sub01_turn_uses_compiled_instructions() {
    let tools = test_tools();
    let marker = "DISTINCTIVE_SUB01_INSTRUCTIONS_MARKER";
    let provider = Arc::new(ScriptProvider::new(vec![(
        "finish",
        serde_json::json!({"summary": "done with distinctive summary text"}),
    )]));
    let agent = Agent::new(provider.clone(), tools, AgentConfig::default());
    let mut convo = Conversation::new();
    let inv = tetonic_domain::AgentInvocation {
        instructions: marker.into(),
        user_input: "do it".into(),
        explain_turn: false,
        empty_tool_nudge: false,
        max_steps: 4,
        completion_tool: "finish".into(),
        discipline: tetonic_domain::LoopDiscipline::default(),
    };
    let outcome = agent.turn(&mut convo, inv, |_| {}).await;
    assert!(matches!(
        outcome,
        CandidateOutcome::Completed {
            kind: CompletionKind::Finish,
            ..
        }
    ));
    let reqs = provider.requests.lock().unwrap();
    let sys = reqs
        .first()
        .and_then(|r| r.messages.iter().find(|m| m.role == "system"))
        .expect("system");
    assert!(sys.content.contains(marker));
}

#[tokio::test]
async fn sub01_turn_uses_compiled_explain_turn() {
    let tools = test_tools();
    let provider = Arc::new(ScriptProvider::new(vec![(
        "write_file",
        serde_json::json!({"path": "x.txt", "content": "nope"}),
    )]));
    let agent = Agent::new(provider, tools, AgentConfig::default());
    let mut convo = Conversation::new();
    let inv = tetonic_domain::AgentInvocation {
        instructions: "rules".into(),
        user_input: "implement slugify".into(),
        explain_turn: true,
        empty_tool_nudge: false,
        max_steps: 2,
        completion_tool: "finish".into(),
        discipline: tetonic_domain::LoopDiscipline::default(),
    };
    let mut blocked = false;
    let _ = agent
        .turn(&mut convo, inv, |step| {
            if let Step::ToolResult {
                ok: false, summary, ..
            } = step
            {
                if summary.contains("blocked on explain turn") {
                    blocked = true;
                }
            }
        })
        .await;
    assert!(
        blocked,
        "compiled explain_turn must block mutating tools even if user text is implement"
    );
}

#[tokio::test]
async fn sub01_turn_uses_compiled_empty_tool_nudge() {
    struct EmptyThenFinish {
        turn: std::sync::atomic::AtomicUsize,
    }
    #[async_trait]
    impl InferenceProvider for EmptyThenFinish {
        async fn fabric_snapshot(&self) -> FabricSnapshot {
            FabricSnapshot {
                nodes: vec![],
                effective_concurrency: 1,
                generated_at: chrono::Utc::now(),
            }
        }
        async fn chat(
            &self,
            _req: ChatRequest,
            _on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            let n = self.turn.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n == 0 {
                Ok(ChatResponse {
                    message: Message::assistant("prose only"),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            } else {
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        tetonic_inference::ToolCall {
                            function: tetonic_inference::FunctionCall {
                                name: "finish".into(),
                                arguments: serde_json::json!({"summary": "finished after nudge xx"}),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            }
        }
    }

    let agent = Agent::new(
        Arc::new(EmptyThenFinish {
            turn: std::sync::atomic::AtomicUsize::new(0),
        }),
        test_tools(),
        AgentConfig::default(),
    );
    let mut convo = Conversation::new();
    let mut notes = Vec::new();
    let inv = tetonic_domain::AgentInvocation {
        instructions: "rules".into(),
        user_input: "implement slugify".into(),
        explain_turn: false,
        empty_tool_nudge: false,
        max_steps: 4,
        completion_tool: "finish".into(),
        discipline: tetonic_domain::LoopDiscipline::default(),
    };
    let _ = agent
        .turn(&mut convo, inv, |step| {
            if let Step::Note(n) = step {
                notes.push(n);
            }
        })
        .await;
    assert!(
        notes.iter().all(|n| !n.contains("no tool calls")),
        "empty_tool_nudge false must not emit the nudge even on ACTION text"
    );
}

#[test]
fn sub01_orchestrator_passes_invocation() {
    let src = read(engine_root().join("mantle/tetonic-orchestrator/src/turn.rs"));
    assert!(src.contains("root_execute"));
    assert!(src.contains(".execute("));
    assert!(src.contains("conversation,"));
    assert!(src.contains("invocation,"));
    assert!(src.contains("root_explain_turn(input.user_text)"));
    assert!(src.contains("is_completed()"));
}

#[test]
fn sub01_orchestrator_matches_candidate() {
    let src = read(engine_root().join("mantle/tetonic-orchestrator/src/turn.rs"));
    assert!(src.contains("if terminal.is_completed()"));
    assert!(src.contains("CandidateOutcome::Failed"));
}

#[test]
fn sub01_turn_returns_candidate_not_result() {
    let src = production_core_src("agent.rs");
    assert!(src.contains("-> CandidateOutcome"));
    assert!(!src.contains("pub async fn turn<F>(") || src.contains("invocation: AgentInvocation"));
    assert!(!src.contains("fn system_prompt"));
}

#[tokio::test]
async fn sub01_kernel_finish_does_not_verify_or_commit() {
    let provider = Arc::new(ScriptProvider::new(vec![(
        "finish",
        serde_json::json!({"summary": "done with distinctive summary text"}),
    )]));
    let agent = Agent::new(provider, test_tools(), AgentConfig::default());
    let mut convo = Conversation::new();
    let inv = tetonic_domain::AgentInvocation {
        instructions: "rules".into(),
        user_input: "implement slugify".into(),
        explain_turn: false,
        empty_tool_nudge: false,
        max_steps: 2,
        completion_tool: "finish".into(),
        discipline: tetonic_domain::LoopDiscipline::default(),
    };
    let outcome = agent.turn(&mut convo, inv, |_| {}).await;
    assert!(matches!(
        outcome,
        CandidateOutcome::Completed {
            kind: CompletionKind::Finish,
            ..
        }
    ));
}

#[test]
fn sub01_prompt_task_law_gone_from_core() {
    let agent = production_core_src("agent.rs");
    let monitor = production_core_src("monitor.rs");
    assert!(!agent.contains("fn system_prompt"));
    assert!(!agent.contains("run_verify_authorized"));
    assert!(!agent.contains("tools.commit_staged_if_any"));
    assert!(!agent.contains("task_is_explain_only"));
    assert!(!agent.contains("task_requires_tools"));
    assert!(!agent.contains("task_user_message"));
    assert!(!monitor.contains("task_requires_tools"));
    assert!(!monitor.contains("edit_file or write_file"));
    assert!(
        !Path::new(&engine_root().join("core/tetonic-core/src/task.rs")).exists(),
        "task.rs production heuristics must be gone"
    );
}

#[test]
fn sub01_infer_never_constructs_agent() {
    let node = engine_root().join("mantle/tetonic-node/src");
    fn mentions_agent(root: &Path) -> bool {
        if !root.exists() {
            return false;
        }
        for entry in fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if mentions_agent(&path) {
                    return true;
                }
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = path.to_string_lossy();
            if text.contains("tests") || text.contains("aud01_") {
                continue;
            }
            if let Ok(src) = fs::read_to_string(&path) {
                if src.contains("tetonic_core::Agent") || src.contains("Agent::new") {
                    return true;
                }
            }
        }
        false
    }
    assert!(
        !mentions_agent(&node),
        "Infer/node must not construct Agent"
    );
}

#[test]
fn sub01_arch_v4_sub_001_inventory_not_established() {
    let agent = production_core_src("agent.rs");
    assert!(agent.contains("-> CandidateOutcome"));
    assert!(!agent.contains("fn system_prompt"));
    assert!(!agent.contains("run_verify_authorized"));
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/tetonic-arch-gate/fixtures/v4/ARCH-V4-SUB-001.v4fix");
    let note = read(fixture);
    assert!(note.contains("SUB-01"));
    assert!(note.contains("not scanned as production"));
}
