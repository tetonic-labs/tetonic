use super::super::results::tool_result;
use super::*;

#[test]
fn domain_errors_preserve_actionable_text_and_structured_receipts() {
    let result = tool_result(
        "interact",
        &json!({"isError":true,"content":[{"type":"text","text":"Not adjacent; move closer"}],"structuredContent":{"action_id":"receipt-42","success":false}}),
    );
    assert!(!result.ok);
    assert_eq!(result.error_kind.as_deref(), Some("mcp_tool_error"));
    assert!(result
        .to_model_string()
        .contains("Not adjacent; move closer"));
    assert!(result.to_model_string().contains("receipt-42"));
    assert!(
        !result.summary.contains("Not adjacent"),
        "service feedback is content, not an audit summary"
    );
}

#[test]
fn resources_and_structured_only_content_are_returned_without_following_links() {
    let result = tool_result(
        "observe",
        &json!({"content":[
            {"type":"resource","resource":{"uri":"village://local","text":"Visible tree"}},
            {"type":"resource_link","uri":"https://example.invalid/result","name":"Reference"}
        ]}),
    );
    assert!(
        result.ok
            && result.content.contains("Visible tree")
            && result.content.contains("example.invalid")
    );
    let structured = tool_result(
        "move",
        &json!({"content":[],"structuredContent":{"arrived":false,"accepted":true}}),
    );
    assert!(structured.ok && structured.content.contains("\"arrived\":false"));
}

#[test]
fn unusable_results_do_not_imply_that_a_remote_action_was_rolled_back() {
    for value in [
        json!({}),
        json!({"content":[],"isError":"true"}),
        json!({"content":[],"structuredContent":null}),
        json!({"content":[{"type":"image","data":"..."}]}),
        json!({"content":[{"type":"text","text":"x".repeat(32_769)}]}),
    ] {
        let result = tool_result("action", &value);
        assert!(!result.ok);
        assert!(result
            .content
            .contains("remote completion is not confirmed"));
    }
}
