use super::*;

#[tokio::test]
async fn connection_errors_explain_recovery_without_assuming_a_token_was_rejected() {
    let fixture = Fixture::new().await;
    let registry = McpRegistry::from_json(&fixture.config).unwrap();
    for (status, explanation) in [
        (401, "requires sign-in"),
        (403, "Access was denied"),
        (404, "did not respond as an MCP service"),
        (405, "did not respond as an MCP service"),
        (200, "did not respond as an MCP service"),
        (302, "redirects elsewhere"),
        (429, "too many requests"),
        (503, "having trouble responding"),
    ] {
        fixture.http_status.store(status, Ordering::SeqCst);
        let view = registry.refresh("calendar").await.unwrap();
        assert_eq!(view.status, "unavailable");
        assert!(view.message.contains(explanation), "{}", view.message);
        assert!(!view.message.contains("PRIVATE_SERVICE_BODY"));
        assert!(!view.message.contains("saved token"));
        assert!(view.tools.is_empty());
        assert!(registry.tool_names().is_empty());
    }
    assert_eq!(
        fixture.calls.lock().unwrap().len(),
        8,
        "no automatic retries"
    );
    fixture.http_status.store(0, Ordering::SeqCst);
    assert_eq!(
        registry.refresh("calendar").await.unwrap().status,
        "discovered"
    );
}
