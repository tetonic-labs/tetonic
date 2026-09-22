//! SUB-04 loop neutralization tests in lokai-core.
//! Proves production loop has no coding literals and empty discipline has no coding law.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use lokai_inference::{
    ChatRequest, ChatResponse, FabricSnapshot, FunctionCall, InferenceError, InferenceProvider,
    Message, TokenSink, ToolCall,
};
use lokai_tools::{Tools, Workspace};
use tetonic_core::{Agent, AgentConfig, Conversation};
use tetonic_domain::{AgentInvocation, CandidateOutcome, CompletionKind, LoopDiscipline};

fn production_prefix(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap_or(src)
}

#[test]
fn sub04_production_loop_source_has_no_coding_literals() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    for file in ["agent.rs", "monitor.rs", "demuxer.rs"] {
        let src = fs::read_to_string(root.join(file)).unwrap_or_else(|_| panic!("read {file}"));
        let prod = production_prefix(&src);
        assert!(
            !prod.contains("\"read_file\""),
            "{file} production prefix must not contain \"read_file\""
        );
        assert!(
            !prod.contains("\"search_code\""),
            "{file} production prefix must not contain \"search_code\""
        );
        assert!(
            !prod.contains("summary.len() < 20"),
            "{file} production prefix must not contain \"summary.len() < 20\""
        );
        assert!(
            !prod.contains("\"spawn_agent\""),
            "{file} production prefix must not contain \"spawn_agent\""
        );
        assert!(
            !prod.contains("\"expand_context\""),
            "{file} production prefix must not contain \"expand_context\""
        );
        assert!(
            !prod.contains("std::fs::metadata"),
            "{file} production prefix must not contain std::fs::metadata"
        );
    }
}

struct StepMockProvider {
    responses: Mutex<Vec<ChatResponse>>,
}

#[async_trait]
impl InferenceProvider for StepMockProvider {
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
        let mut resp = self.responses.lock().unwrap();
        if !resp.is_empty() {
            Ok(resp.remove(0))
        } else {
            Ok(ChatResponse {
                message: Message::assistant("default"),
                usage: Default::default(),
                provenance: Default::default(),
            })
        }
    }
}

#[tokio::test]
async fn sub04_empty_discipline_has_no_spawn_or_floor() {
    let temp = tempfile::tempdir().unwrap();
    let ws = Workspace::new(temp.path()).unwrap();
    let tools = Tools::new(ws, false);

    // Call finish with a summary of only 4 characters ("done").
    // With empty LoopDiscipline, finish_min_chars is None, so it must not be rejected!
    let provider = Arc::new(StepMockProvider {
        responses: Mutex::new(vec![ChatResponse {
            message: Message::assistant("").with_tool_calls(vec![ToolCall {
                function: FunctionCall {
                    name: "finish".into(),
                    arguments: serde_json::json!({ "summary": "done" }),
                },
            }]),
            usage: Default::default(),
            provenance: Default::default(),
        }]),
    });

    let agent = Agent::new(provider, tools, AgentConfig::default());
    let mut convo = Conversation::new();
    let inv = AgentInvocation {
        instructions: "test".into(),
        user_input: "run".into(),
        explain_turn: true,
        empty_tool_nudge: false,
        max_steps: 4,
        completion_tool: "finish".into(),
        discipline: LoopDiscipline::default(),
    };

    let outcome = agent.turn(&mut convo, inv, |_| {}).await;
    assert!(
        matches!(
            outcome,
            CandidateOutcome::Completed {
                kind: CompletionKind::Finish,
                ..
            }
        ),
        "short summary must complete when finish_min_chars is None"
    );
}
