//! Mock test harness for Application and portal testing.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tetonic_inference::{
    ChatRequest, ChatResponse, FabricSnapshot, FunctionCall, InferenceError, InferenceProvider,
    Message, TokenSink, ToolCall, LOCAL_NODE_ID,
};

use crate::events::ApplicationEventSink;
use crate::{Application, ApplicationDependencies};

#[derive(Clone, Debug)]
pub struct ScriptTurn {
    pub content: String,
    pub calls: Vec<(String, Value)>,
}

pub fn turn(content: &str, calls: Vec<(&str, Value)>) -> ScriptTurn {
    ScriptTurn {
        content: content.into(),
        calls: calls.into_iter().map(|(n, a)| (n.to_string(), a)).collect(),
    }
}

pub struct MockProvider {
    turns: Vec<ScriptTurn>,
    idx: AtomicUsize,
}

impl MockProvider {
    pub fn new(turns: Vec<ScriptTurn>) -> Self {
        Self {
            turns,
            idx: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl InferenceProvider for MockProvider {
    async fn fabric_snapshot(&self) -> FabricSnapshot {
        FabricSnapshot {
            nodes: vec![tetonic_inference::NodeInfo {
                id: LOCAL_NODE_ID.into(),
                label: "mock".into(),
                vram_total_mb: 0,
                vram_free_mb: 0,
                resident_models: vec![],
                queue_depth: 0,
                healthy: true,
                models_verified: true,
                capacity: None,
                legacy_v1_chat_only: false,
                negotiated_protocol_version: None,
            }],
            effective_concurrency: 1,
            generated_at: chrono::Utc::now(),
        }
    }

    async fn chat(
        &self,
        _req: ChatRequest,
        on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        let i = self.idx.fetch_add(1, Ordering::Relaxed);
        let msg = match self.turns.get(i) {
            Some(t) => {
                let calls = t
                    .calls
                    .iter()
                    .map(|(n, a)| ToolCall {
                        function: FunctionCall {
                            name: n.clone(),
                            arguments: a.clone(),
                        },
                    })
                    .collect::<Vec<_>>();
                let m = Message::assistant(&t.content);
                if calls.is_empty() {
                    m
                } else {
                    m.with_tool_calls(calls)
                }
            }
            None => Message::assistant("").with_tool_calls(vec![ToolCall {
                function: FunctionCall {
                    name: "finish".into(),
                    arguments: json!({"summary":"done"}),
                },
            }]),
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

impl Application {
    pub fn bootstrap_mock(
        workspace: &Path,
        event_sink: Arc<dyn ApplicationEventSink>,
        turns: Vec<ScriptTurn>,
    ) -> Arc<Self> {
        Self::bootstrap_mock_with_store(workspace, None, event_sink, turns)
    }

    pub fn bootstrap_mock_with_store(
        workspace: &Path,
        store: Option<tetonic_memory::SharedStore>,
        event_sink: Arc<dyn ApplicationEventSink>,
        turns: Vec<ScriptTurn>,
    ) -> Arc<Self> {
        let policy = Arc::new(tetonic_policy::PolicyEngine::default());
        let artifact_store = Arc::new(
            tetonic_artifact::LocalArtifactStore::new(
                workspace.join("artifacts"),
                crate::secret_scanner_factory::artifact_scan_policy(&store),
            )
            .unwrap(),
        );
        let runtime = Arc::new(tetonic_runtime::EngineRuntime::new(
            policy.clone(),
            None,
            artifact_store,
        ));
        let app = Arc::new(Self::new(ApplicationDependencies {
            runtime,
            store,
            policy,
            event_sink,
            index_db: None,
            fabric_hint: None,
        }));
        let provider: Arc<dyn InferenceProvider> = Arc::new(MockProvider::new(turns));
        app.bind_inference(provider, None);
        app
    }
}
