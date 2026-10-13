use super::*;
use tetonic_inference::hosted::{responses, HostedModelConfig};

fn exchange() -> Vec<Message> {
    vec![
        responses::response(serde_json::json!({"status":"completed","output":[
            {"type":"reasoning","id":"reason-1","summary":[],"encrypted_content":"opaque-test-state"},
            {"type":"function_call","call_id":"a","name":"read_file","arguments":"{\"path\":\"a\"}"},
            {"type":"function_call","call_id":"b","name":"read_file","arguments":"{\"path\":\"b\"}"}
        ]}), "model").unwrap().message,
        Message::tool("read_file", "a ".repeat(400)).with_tool_call_id("a"),
        Message::tool("read_file", "b ".repeat(400)).with_tool_call_id("b"),
    ]
}

fn agent() -> Agent {
    Agent::new(
        Arc::new(super::MockTestProvider {
            requests: std::sync::Mutex::new(vec![]),
        }),
        super::DummyHost,
        AgentConfig {
            num_ctx: 400,
            context_reserve: 0,
            ..Default::default()
        },
    )
}

#[test]
fn trimming_keeps_the_task_and_entire_latest_tool_exchange() {
    let agent = agent();
    let mut history = vec![
        Message::system("policy"),
        Message::user("Original assigned task"),
    ];
    history.push(Message::assistant("older history ".repeat(200)));
    history.extend(exchange());
    let (sent, report) = agent.build_context(&history, 1);
    assert_eq!(sent.len(), 5);
    assert_eq!(sent[1].content, "Original assigned task");
    assert_eq!(sent[2].tool_calls.as_ref().unwrap().len(), 2);
    assert_eq!(sent[3].tool_call_id.as_deref(), Some("a"));
    assert_eq!(sent[4].tool_call_id.as_deref(), Some("b"));
    assert_eq!(report.dropped_messages, 1);
    assert_eq!(
        report.total_tokens,
        sent.iter().map(|m| agent.message_tokens(m)).sum::<usize>() + report.tools_tokens
    );
    let request = ChatRequest {
        model: "model".into(),
        messages: sent,
        ..Default::default()
    };
    let body = responses::request(&request, &HostedModelConfig::responses("model", 100)).unwrap();
    assert!(body["input"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["encrypted_content"] == "opaque-test-state"));
}

#[test]
fn older_exchanges_drop_whole_and_history_that_fits_is_unchanged() {
    let mut agent = agent();
    let mut history = vec![Message::system("policy"), Message::user("task")];
    history.extend(exchange());
    history.push(Message::assistant("Recent answer"));
    let (sent, report) = agent.build_context(&history, 1);
    assert_eq!(
        sent.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(),
        ["system", "user", "assistant"]
    );
    assert_eq!(report.dropped_messages, 3);
    agent.config.num_ctx = 8192;
    let (sent, report) = agent.build_context(&history, 1);
    assert_eq!(report.dropped_messages, 0);
    assert_eq!(
        serde_json::to_value(sent).unwrap(),
        serde_json::to_value(history).unwrap()
    );
}

#[test]
fn compaction_cuts_before_a_tool_batch_not_between_its_results() {
    let mut history = vec![Message::system("policy"), Message::user("task")];
    history.extend(exchange());
    for cut in 2..=4 {
        assert_eq!(exchange_start(&history, cut, 1), 2);
    }
    assert_eq!(exchange_start(&history, history.len(), 1), history.len());
    assert_eq!(exchange_start(&history, 1, 1), 1);
}

#[tokio::test]
async fn compaction_preserves_protected_prefix_and_complete_recent_exchange() {
    let mut agent = agent();
    agent.config.keep_recent = 2;
    let mut history = vec![
        Message::system("policy"),
        Message::system("protected context"),
        Message::user("old ".repeat(500)),
    ];
    history.extend(exchange());
    let mut prefix_len = 2;
    let mut tokens = 0;
    assert_eq!(
        agent
            .maybe_compact(
                &mut history,
                &mut prefix_len,
                0.5,
                Some("Summarize"),
                &mut tokens
            )
            .await
            .unwrap(),
        Some(1)
    );
    assert_eq!(prefix_len, 2);
    assert_eq!(history[0].content, "policy");
    assert_eq!(history[1].content, "protected context");
    assert_eq!(history[3].provider_tool_call_ids(), ["a", "b"]);
    responses::request(
        &ChatRequest {
            model: "model".into(),
            messages: history,
            ..Default::default()
        },
        &HostedModelConfig::responses("model", 100),
    )
    .unwrap();
}
