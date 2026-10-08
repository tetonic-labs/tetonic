use super::*;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn remote_mcp_is_exactly_granted_and_private_destinations_are_rejected() {
    let guard = EgressGuard::new();
    let credential = HostedCredential::bearer("TEST_ONLY").unwrap();
    let endpoint = "https://example.invalid/mcp";
    assert!(matches!(
        guard
            .post_mcp(
                endpoint,
                &json!({"id":1}),
                None,
                "2025-11-25",
                Some(&credential)
            )
            .await,
        Err(EgressError::Denied { .. })
    ));
    guard.allow_hosted_endpoint(endpoint).unwrap();
    assert!(matches!(
        guard
            .post_mcp(
                "https://example.invalid/other",
                &json!({"id":1}),
                None,
                "2025-11-25",
                Some(&credential)
            )
            .await,
        Err(EgressError::Denied { .. })
    ));
    for url in [
        "https://127.0.0.1/mcp",
        "https://169.254.169.254/mcp",
        "https://[::1]/mcp",
    ] {
        guard.allow_hosted_endpoint(url).unwrap();
        assert!(guard
            .post_mcp(url, &json!({"id":1}), None, "2025-11-25", Some(&credential))
            .await
            .is_err());
    }
    for url in [
        "http://example.com/mcp",
        "https://token@example.com/mcp",
        "https://example.com/mcp?key=secret",
        "https://example.com/mcp#secret",
        "https://example.com:0/mcp",
    ] {
        assert!(mcp_endpoint(url).is_err());
    }
}

#[tokio::test]
async fn service_credentials_are_headers_and_never_follow_a_redirect() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let trap = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let trap_address = trap.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 8192];
        let n = socket.read(&mut bytes).await.unwrap();
        let text = String::from_utf8_lossy(&bytes[..n]);
        assert!(text
            .to_lowercase()
            .contains("authorization: bearer fixture_secret"));
        assert!(!text.lines().next().unwrap().contains("SECRET"));
        let response = format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{trap_address}/stolen\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        socket.write_all(response.as_bytes()).await.unwrap();
    });
    let guard = EgressGuard::new();
    guard.allow_node("fixture", address.ip(), Some(address.port()));
    let token = HostedCredential::bearer("FIXTURE_SECRET").unwrap();
    let result = guard
        .post_mcp(
            &format!("http://{address}/mcp"),
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
            None,
            "2025-11-25",
            Some(&token),
        )
        .await;
    assert!(result.is_err());
    server.await.unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), trap.accept())
            .await
            .is_err()
    );
}

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
