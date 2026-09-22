//! SUB-04 product compile and behavioral explain tests in lokai-app.
//! Proves production compile fills LoopDiscipline and real loop refuses mutation on explain turns.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tetonic_app::definition::CodingAgentDefinition;
use tetonic_core::{Agent, AgentConfig, Conversation};
use tetonic_domain::{CandidateOutcome, CompletionKind};
use tetonic_inference::{
    ChatRequest, ChatResponse, FabricSnapshot, FunctionCall, InferenceError, InferenceProvider,
    Message, TokenSink, ToolCall,
};
use tetonic_tools::{Tools, Workspace};

fn production() -> CodingAgentDefinition {
    CodingAgentDefinition::production()
}

fn test_tools() -> Tools {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.keep();
    Tools::new(Workspace::new(path).unwrap(), false)
}

#[test]
fn sub04_compile_fills_discipline_on_explain() {
    let def = production();
    let tools = test_tools();
    let cfg = AgentConfig {
        explain_turn: true,
        ..AgentConfig::default()
    };
    let inv = def.compile_invocation_from_tools(&tools, &cfg, "explain the compiler");

    assert!(inv.explain_turn);
    assert!(!inv.empty_tool_nudge);
    assert_eq!(inv.completion_tool, "finish");
    assert_eq!(inv.discipline.finish_min_chars, Some(20));
    assert_eq!(inv.discipline.empty_tool_nudge_text, None);
    assert_eq!(inv.discipline.spawn_tool.as_deref(), Some("spawn_agent"));
    assert_eq!(
        inv.discipline.expand_tool.as_deref(),
        Some("expand_context")
    );
    assert!(inv.discipline.is_whole_file_tool("read_file"));
    assert!(inv.discipline.is_search_tool("search_code"));
    assert!(inv.discipline.notes.mutate_feedback.is_some());
    assert!(inv.discipline.notes.reread_feedback.is_some());
    assert!(inv.discipline.notes.size_feedback.is_some());
}

#[test]
fn sub04_compile_omits_nudge_text_when_ineligible() {
    let def = production();
    let tools = test_tools();
    let cfg = AgentConfig {
        explain_turn: false,
        ..AgentConfig::default()
    };
    // Non-action query should not trigger empty_tool_nudge
    let inv = def.compile_invocation_from_tools(&tools, &cfg, "hello there");

    assert!(!inv.empty_tool_nudge);
    assert_eq!(inv.discipline.empty_tool_nudge_text, None);
}

struct SequentialProvider {
    steps: Mutex<Vec<ChatResponse>>,
}

#[async_trait]
impl InferenceProvider for SequentialProvider {
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
        let mut steps = self.steps.lock().unwrap();
        if !steps.is_empty() {
            Ok(steps.remove(0))
        } else {
            Ok(ChatResponse {
                message: Message::assistant("fallback answer"),
                usage: Default::default(),
                provenance: Default::default(),
            })
        }
    }
}

#[tokio::test]
async fn sub04_explain_refuses_mutation_and_answers() {
    let dir = tempfile::tempdir().unwrap();
    let ws_path = dir.path().to_path_buf();
    let tools = Tools::new(Workspace::new(&ws_path).unwrap(), false);

    let provider = Arc::new(SequentialProvider {
        steps: Mutex::new(vec![
            // Step 1: Model attempts to mutate a file on an explain turn.
            ChatResponse {
                message: Message::assistant("").with_tool_calls(vec![ToolCall {
                    function: FunctionCall {
                        name: "write_file".into(),
                        arguments: serde_json::json!({
                            "path": "forbidden.txt",
                            "content": "must not be written"
                        }),
                    },
                }]),
                usage: Default::default(),
                provenance: Default::default(),
            },
            // Step 2: Model receives the refusal and answers in prose.
            ChatResponse {
                message: Message::assistant("This codebase implements a parser for syntax trees."),
                usage: Default::default(),
                provenance: Default::default(),
            },
        ]),
    });

    let def = production();
    let cfg = AgentConfig {
        explain_turn: true,
        workspace_root: Some(ws_path.clone()),
        ..AgentConfig::default()
    };
    let inv = def.compile_invocation_from_tools(&tools, &cfg, "explain this code");

    let agent = Agent::new(provider, tools, cfg);
    let mut convo = Conversation::new();

    let outcome = agent.turn(&mut convo, inv, |_| {}).await;

    // Must complete via prose answer without verify or commit
    match outcome {
        CandidateOutcome::Completed { summary, kind } => {
            assert_eq!(kind, CompletionKind::Answer);
            assert!(summary.contains("This codebase implements a parser"));
        }
        other => panic!("expected Completed Answer outcome, got {other:?}"),
    }

    // Verify mutating file was never created on disk
    assert!(
        !ws_path.join("forbidden.txt").exists(),
        "forbidden.txt must not exist on disk"
    );
}
