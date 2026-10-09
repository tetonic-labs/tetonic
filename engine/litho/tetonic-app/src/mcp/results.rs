//! Tool failures are domain feedback for the agent, unlike private transport or
//! JSON-RPC error bodies. Preserve bounded content on both success and failure.
use super::*;

pub(super) fn tool_result(name: &str, value: &Value) -> ToolOutcome {
    if value.get("isError").is_some_and(|v| !v.is_boolean())
        || value
            .get("structuredContent")
            .is_some_and(|v| !v.is_object())
    {
        return ToolOutcome::fail("MCP result has invalid status or structured data; remote completion is not confirmed. Inspect service state before retrying.", "unsupported_result");
    }
    let Some(content) = value.get("content").and_then(Value::as_array) else {
        return ToolOutcome::fail("MCP result has no content; remote completion is not confirmed. Inspect service state before retrying.", "unsupported_result");
    };
    let mut parts = Vec::new();
    for block in content {
        match block.get("type").and_then(Value::as_str) {
            Some("text") if block.get("text").is_some_and(Value::is_string) => {
                parts.push(block["text"].as_str().unwrap().to_owned());
            }
            Some("resource") if block.pointer("/resource/text").is_some_and(Value::is_string) => {
                parts.push(block["resource"].to_string());
            }
            Some("resource_link") if block.get("uri").is_some_and(Value::is_string) => {
                // Retain a reference as data. Never fetch a server-supplied URL.
                parts.push(block.to_string());
            }
            _ => return ToolOutcome::fail("MCP result contains unsupported media; remote completion is not confirmed. Inspect service state before retrying. Supported content: text, structured data, text resources and resource links.", "unsupported_result"),
        }
    }
    if let Some(structured) = value.get("structuredContent") {
        parts.push(structured.to_string());
    }
    let text = parts.join("\n");
    if text.len() > 32_768 {
        return ToolOutcome::fail("MCP result exceeds 32 KiB; remote completion is not confirmed. Inspect service state before retrying with a narrower request.", "result_too_large");
    }
    let failed = value.get("isError") == Some(&Value::Bool(true));
    ToolOutcome {
        ok: !failed,
        summary: format!(
            "{name} {}",
            if failed {
                "reported a tool error"
            } else {
                "returned a result"
            }
        ),
        content: if failed {
            format!("MCP tool error (service feedback):\n{text}")
        } else {
            text
        },
        error_kind: failed.then(|| "mcp_tool_error".into()),
        change: None,
    }
}
