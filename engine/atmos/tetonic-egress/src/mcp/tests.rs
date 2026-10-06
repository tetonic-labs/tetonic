use super::*;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[test]
fn sse_handles_fragmented_utf8_multiline_events_and_all_line_endings() {
    for newline in ["\n", "\r\n", "\r"] {
        let wire = [
            ": keepalive",
            "",
            "event: message",
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}",
            "",
            "data: {\"jsonrpc\":\"2.0\",\"id\":1,",
            "data: \"result\":{\"text\":\"café 🌍\"}}",
            "",
            "",
        ]
        .join(newline);
        let mut decoder = SseDecoder::default();
        let messages: Vec<_> = wire
            .as_bytes()
            .iter()
            .flat_map(|byte| decoder.push(&[*byte]).unwrap())
            .collect();
        assert_eq!(messages.len(), 2);
        let reply = checked_reply(None, messages[1].clone(), &json!({"id":1})).unwrap();
        assert_eq!(reply.message.unwrap()["result"]["text"], "café 🌍");
    }
    let mut decoder = SseDecoder::default();
    assert!(
        decoder.push(b"data: {\"id\":1}").unwrap().is_empty(),
        "unterminated events are not responses"
    );
    assert!(SseDecoder::default().push(b"data: \xff\n\n").is_err());
    assert!(SseDecoder::default().push(b"data: not json\n\n").is_err());
}

#[test]
fn replies_must_match_the_request_and_cannot_start_server_work() {
    for value in [
        json!({"jsonrpc":"2.0","id":2,"result":{}}),
        json!({"jsonrpc":"1.0","id":1,"result":{}}),
        json!({"jsonrpc":"2.0","id":1,"result":{},"error":{}}),
        json!({"jsonrpc":"2.0","id":1,"method":"sampling/createMessage","params":{}}),
    ] {
        assert!(checked_reply(None, value, &json!({"id":1})).is_err());
    }
}

// Actual HTTP responses exercise the existing redirect-disabled, pinned client.
async fn transport_response(
    status: &str,
    headers: &str,
    body: &str,
) -> Result<McpReply, EgressError> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let response = format!(
        "HTTP/1.1 {status}\r\n{headers}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        let _ = socket.read(&mut request).await;
        let _ = socket.write_all(response.as_bytes()).await;
    });
    let guard = EgressGuard::new();
    guard.allow_node("fixture", address.ip(), Some(address.port()));
    let response = guard
        .post_local_mcp(
            &format!("http://{address}/mcp"),
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
            None,
            "2025-11-25",
        )
        .await;
    task.await.unwrap();
    response
}

#[tokio::test]
async fn transport_rejects_redirects_oversize_and_unsupported_responses() {
    let redirect =
        transport_response("302 Found", "Location: http://127.0.0.1:1/private", "").await;
    assert!(redirect.err().unwrap().to_string().contains("MCP HTTP 302"));
    for mime in ["application/json", "text/event-stream"] {
        let result = transport_response(
            "200 OK",
            &format!("Content-Type: {mime}"),
            &" ".repeat(131_073),
        )
        .await;
        assert!(result.err().unwrap().to_string().contains("exceeds limit"));
    }
    assert!(
        transport_response("200 OK", "Content-Type: text/html", "<html>login</html>")
            .await
            .is_err()
    );
    assert!(transport_response(
        "200 OK",
        "Content-Type: application/json\r\nMCP-Session-Id: invalid session",
        "{}"
    )
    .await
    .is_err());
    assert!(transport_response(
        "200 OK",
        "Content-Type: text/event-stream",
        "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}"
    )
    .await
    .is_err());
}
