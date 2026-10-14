use super::*;
use std::sync::Mutex;

struct Summarizer {
    requests: Mutex<Vec<ChatRequest>>,
}

#[async_trait]
impl InferenceProvider for Summarizer {
    fn estimate_input_tokens(&self, request: &ChatRequest) -> u64 {
        request.estimated_input_tokens() + 500 // metering wrapper's framing
    }

    async fn chat(
        &self,
        req: ChatRequest,
        _: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        let input = self.estimate_input_tokens(&req);
        self.requests.lock().unwrap().push(req);
        Ok(ChatResponse {
            message: Message::assistant("Read source.md lines 1-300; it requires scoped access. Draft.md is staged, not committed or verified. Next inspect draft.md and finish only after checking it."),
            usage: tetonic_inference::GenUsage { prompt_tokens: Some(input), eval_tokens: Some(40), ..Default::default() },
            provenance: Default::default(),
        })
    }
}

fn history() -> Vec<Message> {
    let mut history = vec![
        Message::system("immutable policy"),
        Message::system("scoped context"),
        Message::user("ASSIGNMENT: create and verify draft.md"),
    ];
    for n in 0..8 {
        history.push(
            Message::assistant("").with_tool_calls(vec![tetonic_inference::ToolCall {
                function: tetonic_inference::FunctionCall {
                    name: "read_file".into(),
                    arguments: serde_json::json!({"path":"source.md", "page": n}),
                },
            }]),
        );
        history.push(Message::tool(
            "read_file",
            format!("source.md page {n}: {}", "source evidence ".repeat(400)),
        ));
    }
    history.extend(super::context_window::exchange());
    history
}

#[tokio::test]
async fn allowance_pressure_compacts_early_and_preserves_authority_assignment_and_native_tail() {
    let provider = Arc::new(Summarizer {
        requests: Mutex::new(vec![]),
    });
    let agent = Agent::new(
        provider.clone(),
        DummyHost,
        AgentConfig {
            model: "model".into(),
            num_ctx: 128_000,
            reported_token_ceiling: Some(80_000),
            keep_recent: 2,
            ..Default::default()
        },
    );
    let mut history = history();
    let mut prefix = 2;
    let mut spent = 20_000;
    let template = ChatRequest {
        model: "model".into(),
        fabric: Some(FabricCallMeta {
            run_id: Some("run".into()),
            task_id: Some("task".into()),
            attempt_id: Some("attempt".into()),
            information_context_id: Some("private-work".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let folded = agent
        .maybe_compact(
            &mut history,
            &mut prefix,
            0.75,
            Some("Summarize evidence"),
            &mut spent,
            &template,
        )
        .await
        .unwrap();
    assert_eq!(folded, Some(16));
    assert_eq!(prefix, 2);
    assert_eq!(history[0].content, "immutable policy");
    assert_eq!(history[1].content, "scoped context");
    assert_eq!(history[2].content, "ASSIGNMENT: create and verify draft.md");
    assert!(history[3].content.contains("not committed or verified"));
    assert_eq!(history[4].provider_tool_call_ids(), ["a", "b"]);
    assert!(spent > 20_000, "maintenance is charged, never free");
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let req = &requests[0];
    let meta = req.fabric.as_ref().unwrap();
    assert_eq!(meta.run_id, Some("run".into()));
    assert_eq!(meta.task_id, Some("task".into()));
    assert_eq!(meta.attempt_id, Some("attempt".into()));
    assert_eq!(meta.information_context_id, Some("private-work".into()));
    assert_eq!(req.max_tokens, Some(1024));
    assert!(req.tools.is_empty());
    assert!(!req.messages[1].content.contains("opaque-test-state"));
}

#[tokio::test]
async fn unaffordable_maintenance_does_not_spend_or_destroy_evidence() {
    let provider = Arc::new(Summarizer {
        requests: Mutex::new(vec![]),
    });
    let agent = Agent::new(
        provider.clone(),
        DummyHost,
        AgentConfig {
            reported_token_ceiling: Some(22_000),
            ..Default::default()
        },
    );
    let mut messages = history();
    let before = serde_json::to_value(&messages).unwrap();
    let mut prefix = 2;
    let mut spent = 20_000;
    assert_eq!(
        agent
            .maybe_compact(
                &mut messages,
                &mut prefix,
                0.75,
                Some("Summary"),
                &mut spent,
                &ChatRequest::default()
            )
            .await
            .unwrap(),
        None
    );
    assert!(provider.requests.lock().unwrap().is_empty());
    assert_eq!(serde_json::to_value(messages).unwrap(), before);
    assert_eq!(spent, 20_000);
}
