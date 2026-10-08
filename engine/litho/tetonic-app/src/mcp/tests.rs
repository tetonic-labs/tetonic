use super::*;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Mutex,
};
pub(crate) mod fixture;
use fixture::Fixture;

#[test]
fn config_does_not_accept_remote_endpoints_credentials_or_implicit_tool_grants() {
    for endpoint in [
        "https://example.com/mcp",
        "http://localhost:9000/mcp",
        "http://user:secret@127.0.0.1:9000/mcp",
        "http://127.0.0.1:9000/mcp?key=secret",
    ] {
        let value=json!({"connections":[{"id":"test","name":"Test","endpoint":endpoint,"read_tools":["search"]}]}).to_string();
        assert!(McpRegistry::from_json(value.as_bytes()).is_err());
    }
    assert!(McpRegistry::from_json(br#"{"connections":[{"id":"test","name":"Test","endpoint":"http://127.0.0.1:9000/mcp","read_tools":[],"command":"untrusted"}]}"#).is_err());
}

#[tokio::test]
async fn discovery_and_invocation_pin_tools_and_expose_truthful_failures() {
    let fixture = Fixture::new().await;
    let registry = McpRegistry::from_json(&fixture.config).unwrap();
    assert!(registry.tool_names().is_empty());
    let view = registry.refresh("calendar").await.unwrap();
    assert_eq!(view.status, "discovered");
    assert_eq!(view.tools.len(), 2);
    let search = view.tools.iter().find(|t| t.name == "search").unwrap();
    let cancel = CancellationSignal::default();
    let answer = registry
        .call(&search.id, &json!({"query":"Tuesday"}), &cancel)
        .await;
    assert!(answer.ok, "{answer:?}");
    assert!(answer.content.contains("Tuesday 10:00"));
    fixture.mode.store(1, Ordering::SeqCst);
    let before = fixture
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r["method"] == "tools/call")
        .count();
    let changed = registry.call(&search.id, &json!({}), &cancel).await;
    assert!(!changed.ok);
    assert!(changed.content.contains("changed"));
    assert_eq!(
        before,
        fixture
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["method"] == "tools/call")
            .count()
    );
    let next = registry.refresh("calendar").await.unwrap();
    assert!(!next.tools.iter().any(|t| t.id == search.id));
    assert!(!registry.call(&search.id, &json!({}), &cancel).await.ok);
    fixture.mode.store(0, Ordering::SeqCst);
    registry.refresh("calendar").await.unwrap();
    fixture.mode.store(2, Ordering::SeqCst);
    let failure = registry.call(&search.id, &json!({}), &cancel).await;
    assert_eq!(failure.error_kind.as_deref(), Some("mcp_tool_error"));
    assert!(!failure.content.contains("PRIVATE_SERVER_ERROR"));
    fixture.mode.store(4, Ordering::SeqCst);
    assert!(registry.refresh("calendar").await.unwrap().tools.len() == 2);
    assert!(registry.call(&search.id, &json!({}), &cancel).await.ok);
    fixture.mode.store(5, Ordering::SeqCst);
    assert_eq!(
        registry.refresh("calendar").await.unwrap().status,
        "unavailable"
    );
    assert!(registry.tool_names().is_empty());
}

#[tokio::test]
async fn stopping_an_inflight_mcp_read_requests_cancellation_without_retrying() {
    let fixture = Fixture::new().await;
    let registry = McpRegistry::from_json(&fixture.config).unwrap();
    let tool = registry.refresh("calendar").await.unwrap().tools[0]
        .id
        .clone();
    fixture.mode.store(3, Ordering::SeqCst);
    let scope = tetonic_domain::work_scope::WorkScope::default();
    let cancel = scope.cancellation_signal();
    let args = json!({});
    let call = registry.call(&tool, &args, &cancel);
    let stop = async {
        while !fixture
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|r| r["method"] == "tools/call")
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        scope.cancel();
    };
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        tokio::join!(call, stop)
    })
    .await
    .unwrap();
    assert!(!result.ok);
    assert!(result.content.contains("not confirmed"));
    assert_eq!(
        fixture
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["method"] == "tools/call")
            .count(),
        1
    );
    assert!(fixture
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|r| r["method"] == "notifications/cancelled"));
}

mod managed;

#[tokio::test]
async fn checkpoint_readiness_requires_the_selected_manifest_and_a_rebuildable_inner_host() {
    use tetonic_domain::ToolHost;
    let fixture = Fixture::new().await;
    let registry = McpRegistry::from_json(&fixture.config).unwrap();
    let view = registry.refresh("calendar").await.unwrap();
    let search = view
        .tools
        .iter()
        .find(|tool| tool.name == "search")
        .unwrap()
        .id
        .clone();
    let host = McpToolHost {
        inner: Box::new(tetonic_tools::Tools::without_repository().unwrap()),
        registry: registry.clone(),
        selected: [search].into_iter().collect(),
        consumer: Arc::new(tetonic_runtime::InMemoryCapabilityStore::default()),
        runtime: tokio::runtime::Handle::current(),
    };
    assert!(host.checkpoint_ready());
    let mut retained = host.clone();
    retained.inner = Box::new(
        tetonic_tools::Tools::without_repository()
            .unwrap()
            .with_orchestration(true),
    );
    assert!(!retained.checkpoint_ready());
    fixture.mode.store(1, Ordering::SeqCst);
    registry.refresh("calendar").await.unwrap();
    assert!(
        !host.checkpoint_ready(),
        "changed selected manifests cannot be reconstructed"
    );
    fixture.mode.store(0, Ordering::SeqCst);
    registry.refresh("calendar").await.unwrap();
    assert!(host.checkpoint_ready());
    assert!(
        !fixture
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|call| call["method"] == "tools/call"),
        "readiness itself must not invoke an MCP tool"
    );
}
