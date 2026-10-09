use super::*;
use tetonic_domain::ToolHost;

fn action_config(fixture: &Fixture) -> Vec<u8> {
    let mut config: Value = serde_json::from_slice(&fixture.config).unwrap();
    config["connections"][0]["action_tools"] =
        json!(["delete_event", "unannotated", "background_only"]);
    serde_json::to_vec(&config).unwrap()
}

#[tokio::test]
async fn explicit_action_grants_support_mutations_and_missing_hints_without_widening_reads() {
    let fixture = Fixture::new().await;
    let registry = McpRegistry::from_json(&action_config(&fixture)).unwrap();
    let view = registry.refresh("calendar").await.unwrap();
    assert_eq!(
        view.tools.len(),
        4,
        "skip task-only tools, not the whole server"
    );
    let action = view
        .tools
        .iter()
        .find(|t| t.name == "delete_event")
        .unwrap();
    let unknown = view.tools.iter().find(|t| t.name == "unannotated").unwrap();
    assert!(!action.read_only && action.destructive && !action.idempotent);
    assert!(!unknown.read_only && unknown.destructive && !unknown.idempotent);
    let read = view.tools.iter().find(|t| t.name == "search").unwrap();
    let mut host = McpToolHost {
        inner: Box::new(tetonic_tools::Tools::without_repository().unwrap()),
        registry: registry.clone(),
        selected: view.tools.iter().map(|t| t.id.clone()).collect(),
        consumer: Arc::new(tetonic_runtime::InMemoryCapabilityStore::default()),
        runtime: tokio::runtime::Handle::current(),
    };
    assert!(host.is_tool_allowed(&action.id));
    assert!(host.requires_action_broker(&action.id));
    assert!(!host.is_read_only(&action.id));
    assert!(!host.is_read_only(&unknown.id));
    assert!(host.is_read_only(&read.id));
    assert!(host.checkpoint_ready());
    host.selected.remove(&action.id);
    assert!(!host.is_tool_allowed(&action.id));
    let cancel = CancellationSignal::default();
    let result = registry.call(&action.id, &json!({}), &cancel).await;
    assert!(result.ok && result.content.contains("Event deleted"));
    fixture.mode.store(10, Ordering::SeqCst);
    let before = fixture.calls.lock().unwrap().len();
    assert!(!registry.call(&action.id, &json!({}), &cancel).await.ok);
    assert!(!fixture.calls.lock().unwrap()[before..]
        .iter()
        .any(|c| c["method"] == "tools/call"));
    fixture.mode.store(9, Ordering::SeqCst);
    assert_eq!(
        registry.refresh("calendar").await.unwrap().status,
        "unavailable",
        "a legacy read grant must never become an action grant"
    );
}

#[test]
fn config_accepts_action_only_but_rejects_ambiguous_or_implicit_grants() {
    let base = json!({"connections":[{"id":"test","name":"Test","endpoint":"http://127.0.0.1:9000/mcp","action_tools":["move"]}]});
    assert!(McpRegistry::from_json(&serde_json::to_vec(&base).unwrap()).is_ok());
    let mut duplicate = base.clone();
    duplicate["connections"][0]["read_tools"] = json!(["move"]);
    assert!(McpRegistry::from_json(&serde_json::to_vec(&duplicate).unwrap()).is_err());
    let mut empty = base;
    empty["connections"][0]["action_tools"] = json!([]);
    assert!(McpRegistry::from_json(&serde_json::to_vec(&empty).unwrap()).is_err());
}

#[tokio::test]
async fn interrupted_actions_report_unknown_outcomes_and_are_never_replayed() {
    for disconnect in [false, true] {
        let fixture = Fixture::new().await;
        let registry = McpRegistry::from_json(&action_config(&fixture)).unwrap();
        let tool = registry
            .refresh("calendar")
            .await
            .unwrap()
            .tools
            .into_iter()
            .find(|t| t.name == "delete_event")
            .unwrap();
        fixture
            .mode
            .store(if disconnect { 11 } else { 3 }, Ordering::SeqCst);
        let scope = tetonic_domain::work_scope::WorkScope::default();
        let cancel = scope.cancellation_signal();
        let args = json!({});
        let operation = registry.call(&tool.id, &args, &cancel);
        let stop = async {
            if disconnect {
                return;
            }
            while !fixture
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|c| c["method"] == "tools/call")
            {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            scope.cancel();
        };
        let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            tokio::join!(operation, stop)
        })
        .await
        .unwrap();
        assert_eq!(result.error_kind.as_deref(), Some("mcp_outcome_unknown"));
        assert!(result.content.contains("may have completed"));
        assert_eq!(
            fixture
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c["method"] == "tools/call")
                .count(),
            1
        );
    }
}
