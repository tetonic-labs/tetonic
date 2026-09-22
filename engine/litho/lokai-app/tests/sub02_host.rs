//! SUB-02 host-surface pins. Absence/inventory only; not ESTABLISHED. Not GATE-001.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use lokai_app::definition::CodingAgentDefinition;
use lokai_core::{Agent, AgentConfig, Conversation, Step};
use lokai_domain::AgentInvocation;
use lokai_inference::{
    ChatRequest, ChatResponse, FabricSnapshot, InferenceError, InferenceProvider, Message,
    TokenSink,
};
use lokai_tools::{Tools, Workspace};

fn engine_crate(crate_name: &str) -> PathBuf {
    let layers = [
        "core",
        "strata",
        "mantle",
        "atmos",
        "litho",
        "portals",
        "product",
        "manager",
        "substrate",
        "compute",
        "capabilities",
        "infrastructure",
        "tooling",
        "bins",
        "crates",
    ];
    let engine_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for layer in layers {
        let candidate = engine_root.join(layer).join(crate_name);
        if candidate.exists() {
            return candidate;
        }
    }
    panic!("crate {crate_name} not found across layers");
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).expect("read")
}

fn production_src(crate_name: &str, rel: &str) -> String {
    let src = read(engine_crate(crate_name).join("src").join(rel));
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

fn production() -> CodingAgentDefinition {
    CodingAgentDefinition::production()
}

#[test]
fn sub02_compile_invocation_does_not_use_toolhost_catalog() {
    let def_src = production_src("lokai-app", "definition.rs");
    assert!(
        !def_src.contains("&dyn ToolHost"),
        "compile_invocation must not take dyn ToolHost"
    );
    assert!(
        !def_src.contains(".operator_card()"),
        "compile must not call ToolHost::operator_card"
    );
    assert!(
        !def_src.contains(".catalog_prompt_lines()"),
        "compile must not call ToolHost::catalog_prompt_lines"
    );
    assert!(
        !def_src.contains(".workspace_root()"),
        "compile must not call ToolHost::workspace_root"
    );

    let turn_src = production_src("lokai-app", "turn_execution.rs");
    assert!(
        !turn_src.contains("ToolHost::operator_card")
            && !turn_src.contains("tools.operator_card()"),
        "build_agent must not call ToolHost catalog methods"
    );
    assert!(
        !turn_src.contains("tools.catalog_prompt_lines()"),
        "build_agent must not call ToolHost::catalog_prompt_lines"
    );
    assert!(
        !turn_src.contains("tools.workspace_root()"),
        "build_agent must not call ToolHost::workspace_root"
    );

    let dir = tempfile::tempdir().unwrap();
    let tools = Tools::new(Workspace::new(dir.path()).unwrap(), false);
    let unique = "SUB02_UNIQUE_OPERATOR_CARD";
    let inv = production().compile_invocation(
        unique,
        "SUB02_UNIQUE_CATALOG",
        tools.workspace().root(),
        &AgentConfig::default(),
        "plan the change",
    );
    assert!(
        inv.instructions.contains(unique),
        "compile_invocation must use the supplied card string, not ToolHost"
    );
    assert!(inv.instructions.contains("SUB02_UNIQUE_CATALOG"));
}

#[test]
fn sub02_kernel_abort_does_not_call_toolhost() {
    let agent = production_src("lokai-core", "agent.rs");
    assert!(
        !agent.contains("tools.abort_staged_if_any"),
        "kernel must not call ToolHost::abort_staged_if_any"
    );
}

#[test]
fn sub02_kernel_approval_has_no_run_shell_literal() {
    let agent = production_src("lokai-core", "agent.rs");
    let execute_gated = agent
        .split("async fn execute_gated")
        .nth(1)
        .and_then(|s| s.split("async fn run_tool_execute").next())
        .unwrap_or("");
    assert!(
        !execute_gated.contains("run_shell") && !execute_gated.contains("lsp_"),
        "execute_gated must not contain run_shell / lsp_ literals"
    );
    assert!(
        !agent.contains("name == \"run_shell\"") && !agent.contains("name.starts_with(\"lsp_\")"),
        "production agent.rs approval routing must not match run_shell / lsp_ literals"
    );
}

#[test]
fn sub02_toolhost_trait_has_no_workspace_verify_commit() {
    let host = production_src("lokai-domain", "tool_host.rs");
    for method in [
        "fn workspace_root",
        "fn resolve_path",
        "fn has_index",
        "fn has_memory",
        "fn has_lsp",
        "fn operator_card",
        "fn catalog_prompt_lines",
        "fn abort_staged_if_any",
        "fn run_command",
        "fn verification_overlay_if_staged",
        "fn finish_verification_run",
        "fn run_verify_sink",
        "fn commit_staged_if_any",
    ] {
        assert!(
            !host.contains(method),
            "kernel ToolHost must not declare {method}"
        );
    }
    let agent = production_src("lokai-core", "agent.rs");
    assert!(!agent.contains("tools.workspace_root"));
    assert!(!agent.contains("tools.resolve_path"));
    assert!(!agent.contains("tools.commit_staged_if_any"));
    assert!(!agent.contains("tools.run_command"));
}

#[test]
fn sub02_arch_v4_tool_001_fixture_planted() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/lokai-arch-gate/fixtures/v4/ARCH-V4-TOOL-001.v4fix");
    let text = read(fixture);
    assert!(text.contains("SUB-02"));
    assert!(text.contains("production-failing detector live"));
}

struct TwoTurnProvider {
    first_name: &'static str,
    first_args: serde_json::Value,
    turn: AtomicUsize,
}

#[async_trait]
impl InferenceProvider for TwoTurnProvider {
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
        let t = self.turn.fetch_add(1, Ordering::SeqCst);
        if t == 0 {
            Ok(ChatResponse {
                message: Message::assistant("").with_tool_calls(vec![lokai_inference::ToolCall {
                    function: lokai_inference::FunctionCall {
                        name: self.first_name.into(),
                        arguments: self.first_args.clone(),
                    },
                }]),
                usage: Default::default(),
                provenance: Default::default(),
            })
        } else {
            Ok(ChatResponse {
                message: Message::assistant("").with_tool_calls(vec![lokai_inference::ToolCall {
                    function: lokai_inference::FunctionCall {
                        name: "finish".into(),
                        arguments: serde_json::json!({"summary": "completed the explain size check"}),
                    },
                }]),
                usage: Default::default(),
                provenance: Default::default(),
            })
        }
    }
}

fn wire_test_agent(agent: Agent) -> Agent {
    lokai_runtime::wire_kernel_capability_helpers(
        agent,
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

fn explain_inv() -> AgentInvocation {
    AgentInvocation {
        instructions: "You are a test agent operating in the user's workspace.".into(),
        user_input: "what is in the file?".into(),
        explain_turn: true,
        empty_tool_nudge: false,
        max_steps: 8,
        completion_tool: "finish".into(),
        discipline: lokai_app::definition::coding_loop_discipline(true, false),
    }
}

#[tokio::test]
async fn sub02_explain_size_trips_on_workspace_file() {
    let temp = tempfile::tempdir().unwrap();
    let large = "x".repeat(20 * 1024);
    std::fs::write(temp.path().join("big.rs"), &large).unwrap();
    let ws = Workspace::new(temp.path()).unwrap();
    let root = ws.root().to_path_buf();
    let tools = Tools::new(ws, false);
    let provider = Arc::new(TwoTurnProvider {
        first_name: "read_file",
        first_args: serde_json::json!({"path": "big.rs"}),
        turn: AtomicUsize::new(0),
    });
    let agent = wire_test_agent(Agent::new(
        provider,
        tools,
        AgentConfig {
            explain_turn: true,
            workspace_root: Some(root),
            ..AgentConfig::default()
        },
    ));
    let mut convo = Conversation::new();
    let mut summaries = Vec::new();
    agent
        .turn(&mut convo, explain_inv(), |step| {
            if let Step::ToolResult { summary, .. } = step {
                summaries.push(summary);
            }
        })
        .await;
    assert!(
        summaries.iter().any(|s| s.contains("too large")),
        "size gate must trip on an in-workspace large file: {summaries:?}"
    );
}

#[tokio::test]
async fn sub02_explain_size_does_not_stat_outside_workspace() {
    let parent = tempfile::tempdir().unwrap();
    let ws_dir = parent.path().join("ws");
    std::fs::create_dir_all(&ws_dir).unwrap();
    std::fs::write(ws_dir.join("ok.rs"), "inside").unwrap();
    let huge = "y".repeat(32 * 1024);
    std::fs::write(parent.path().join("secret.bin"), &huge).unwrap();

    let ws = Workspace::new(&ws_dir).unwrap();
    let root = ws.root().to_path_buf();
    let tools = Tools::new(ws, false);
    let provider = Arc::new(TwoTurnProvider {
        first_name: "read_file",
        first_args: serde_json::json!({"path": "../secret.bin"}),
        turn: AtomicUsize::new(0),
    });
    let agent = wire_test_agent(Agent::new(
        provider,
        tools,
        AgentConfig {
            explain_turn: true,
            workspace_root: Some(root),
            ..AgentConfig::default()
        },
    ));
    let mut convo = Conversation::new();
    let mut summaries = Vec::new();
    agent
        .turn(&mut convo, explain_inv(), |step| {
            if let Step::ToolResult { summary, .. } = step {
                summaries.push(summary);
            }
        })
        .await;
    assert!(
        !summaries.is_empty(),
        "escape must still produce a tool result"
    );
    assert!(
        summaries.iter().all(|s| !s.contains("too large")),
        "naive join would stat the outside file; jail must miss: {summaries:?}"
    );
    assert!(
        summaries.iter().all(|s| !s.contains(&huge)),
        "must not leak outside file bytes: {summaries:?}"
    );
}
