use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn scripted(
    responses: Vec<(&'static str, Value)>,
) -> (OllamaProvider, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let guard = Arc::new(EgressGuard::new());
    guard.configure_loopback_inference(address.port());
    let provider = OllamaProvider::new(format!("http://{address}"), guard);
    let server = tokio::spawn(async move {
        let mut bodies = Vec::new();
        for (expected, response) in responses {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0u8; 2048];
                let size = socket.read(&mut chunk).await.unwrap();
                assert!(size > 0);
                bytes.extend_from_slice(&chunk[..size]);
                if let Some(end) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let len: usize = headers
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .map(|n| n.trim().parse().unwrap())
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + len {
                        assert!(
                            headers.lines().next().unwrap().contains(expected),
                            "{headers}"
                        );
                        bodies.push(if len == 0 {
                            Value::Null
                        } else {
                            serde_json::from_slice(&bytes[end + 4..end + 4 + len]).unwrap()
                        });
                        break;
                    }
                }
            }
            let body = response.to_string();
            let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            socket.write_all(reply.as_bytes()).await.unwrap();
        }
        bodies
    });
    (provider, server)
}

fn resident(context: u32) -> Value {
    json!({"models":[{"name":"test-model:latest", "context_length":context, "size":100, "size_vram":100}]})
}

#[tokio::test]
async fn warmup_matches_allocation_deduplicates_success_and_retries_failure() {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let (provider, server) = scripted(vec![
            ("/api/ps", json!({"models":[]})),
            ("/api/generate", json!({"done":true})),
            ("/api/ps", resident(4096)),
            ("/api/ps", resident(4096)),
            ("/api/ps", resident(4096)),
            ("/api/generate", json!({"error":"load failed"})),
            ("/api/ps", resident(4096)),
            ("/api/generate", json!({"done":true})),
            ("/api/ps", resident(8192)),
            ("/api/ps", resident(8192)),
            ("/api/generate", json!({"done":true})),
            ("/api/ps", resident(8192)),
        ])
        .await;
        provider
            .prewarm_with_context("test-model", Some("30m"), Some(4096))
            .await
            .unwrap();
        provider
            .prewarm_with_context("test-model", Some("30m"), Some(4096))
            .await
            .unwrap();
        assert!(provider
            .prewarm_with_context("test-model", Some("30m"), Some(8192))
            .await
            .is_err());
        provider
            .prewarm_with_context("test-model", Some("30m"), Some(8192))
            .await
            .unwrap();
        provider
            .prewarm_with_context("test-model", Some("10m"), Some(8192))
            .await
            .unwrap();
        let requests = server.await.unwrap();
        let bodies: Vec<_> = requests.iter().filter(|b| !b.is_null()).collect();
        assert_eq!(bodies.len(), 4);
        assert_eq!(bodies[0]["options"]["num_ctx"], 4096);
        assert_eq!(bodies[1]["options"]["num_ctx"], 8192);
        assert_eq!(bodies[2]["options"]["num_ctx"], 8192);
        assert_eq!(bodies[3]["keep_alive"], "10m");
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn warmup_reloads_evicted_model_without_overriding_retention() {
    let (provider, server) = scripted(vec![
        ("/api/ps", json!({"models":[]})),
        ("/api/generate", json!({"done":true})),
        ("/api/ps", resident(4096)),
        ("/api/ps", json!({"models":[]})),
        ("/api/generate", json!({"done":true})),
        ("/api/ps", resident(4096)),
    ])
    .await;
    for _ in 0..2 {
        provider
            .prewarm_with_context("test-model", None, Some(4096))
            .await
            .unwrap();
    }
    let requests = server.await.unwrap();
    assert!(requests
        .iter()
        .filter(|b| !b.is_null())
        .all(|b| b.get("keep_alive").is_none()));
}

#[tokio::test]
async fn speculative_warmup_does_not_compete_with_another_resident_model() {
    let (provider, server) =
        scripted(vec![("/api/ps", json!({"models":[{"name":"other"}]}))]).await;
    provider
        .prewarm_with_context("test-model", None, Some(4096))
        .await
        .unwrap();
    assert_eq!(server.await.unwrap(), vec![Value::Null]);
}

#[tokio::test]
async fn unload_waits_for_target_release_without_unloading_other_runners() {
    let (provider, server) = scripted(vec![
        ("/api/ps", resident(4096)),
        ("/api/generate", json!({"done":true})),
        ("/api/ps", resident(4096)),
        ("/api/ps", json!({"models":[{"name":"other"}]})),
        ("/api/ps", json!({"models":[{"name":"other"}]})),
    ])
    .await;
    provider.unload_model("test-model").await.unwrap();
    provider.unload_model("test-model").await.unwrap();
    let requests = server.await.unwrap();
    let posts: Vec<_> = requests.iter().filter(|b| !b.is_null()).collect();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0]["model"], "test-model");
    assert_eq!(posts[0]["keep_alive"], 0);
}

fn spilled() -> Value {
    json!({"models":[{"name":"test-model:latest","context_length":4096,"size":100,"size_vram":96}]})
}

#[tokio::test]
async fn spilled_runner_is_released_and_recovered_before_prompt_is_sent() {
    let (provider, server) = scripted(vec![
        ("/api/ps", spilled()),
        ("/api/ps", spilled()),
        ("/api/generate", json!({"done":true})),
        ("/api/ps", json!({"models":[]})),
        ("/api/generate", json!({"done":true})),
        ("/api/ps", resident(4096)),
        (
            "/api/chat",
            json!({"done":true,"message":{"role":"assistant","content":"answer"}}),
        ),
        ("/api/ps", resident(4096)),
        // Reuse the recovered runner and allocation options next turn.
        ("/api/ps", resident(4096)),
        ("/api/generate", json!({"done":true})),
        ("/api/ps", resident(4096)),
        (
            "/api/chat",
            json!({"done":true,"message":{"role":"assistant","content":"answer"}}),
        ),
        ("/api/ps", resident(4096)),
    ])
    .await;
    for _ in 0..2 {
        let mut visible = String::new();
        provider
            .chat(
                ChatRequest {
                    model: "test-model".into(),
                    num_ctx: Some(4096),
                    messages: vec![Message::user("keep the entire prompt")],
                    outbound_scan: OutboundScan::from_scan(false),
                    ..Default::default()
                },
                &mut |t| visible.push_str(t),
            )
            .await
            .unwrap();
        assert_eq!(visible, "answer");
    }
    let requests = server.await.unwrap();
    assert_eq!(requests[2]["keep_alive"], 0);
    assert_eq!(requests[4]["prompt"], "");
    for index in [4, 6, 9, 11] {
        assert_eq!(requests[index]["options"]["num_ctx"], 4096);
        assert_eq!(requests[index]["options"]["num_gpu"], -1);
        assert_eq!(requests[index]["options"]["num_batch"], 128);
    }
    assert_eq!(
        requests[6]["messages"][0]["content"],
        "keep the entire prompt"
    );
}

#[tokio::test]
async fn persistent_spill_is_unloaded_without_sending_the_conversation() {
    let (provider, server) = scripted(vec![
        ("/api/ps", json!({"models":[]})),
        ("/api/generate", json!({"done":true})),
        ("/api/ps", spilled()),
        ("/api/ps", spilled()),
        ("/api/generate", json!({"done":true})),
        ("/api/ps", json!({"models":[]})),
        ("/api/generate", json!({"done":true})),
        ("/api/ps", spilled()),
        ("/api/ps", spilled()),
        ("/api/generate", json!({"done":true})),
        ("/api/ps", json!({"models":[]})),
    ])
    .await;
    let result = provider
        .chat(
            ChatRequest {
                model: "test-model".into(),
                num_ctx: Some(4096),
                outbound_scan: OutboundScan::from_scan(false),
                ..Default::default()
            },
            &mut |_| panic!("must not generate on CPU-offloaded runner"),
        )
        .await;
    assert!(matches!(
        result,
        Err(InferenceError::GpuSpillDetected { .. })
    ));
    assert!(server
        .await
        .unwrap()
        .iter()
        .all(|body| body.get("messages").is_none()));
}

#[tokio::test]
async fn unavailable_placement_does_not_allow_prompt_processing() {
    let (provider, server) = scripted(vec![
        ("/api/ps", json!({"models":[]})),
        ("/api/generate", json!({"done":true})),
        (
            "/api/ps",
            json!({"models":[{"name":"test-model","context_length":4096}]}),
        ),
    ])
    .await;
    let result = provider
        .chat(
            ChatRequest {
                model: "test-model".into(),
                num_ctx: Some(4096),
                outbound_scan: OutboundScan::from_scan(false),
                ..Default::default()
            },
            &mut |_| panic!("placement is unknown"),
        )
        .await;
    assert!(result.is_err());
    assert_eq!(server.await.unwrap().len(), 3);
}

#[test]
fn providers_for_one_runtime_share_the_allocation_lease() {
    let one = residency::runtime_admission("http://127.0.0.1:11434");
    let two = residency::runtime_admission("http://127.0.0.1:11434/");
    let other = residency::runtime_admission("http://127.0.0.1:11435");
    assert!(Arc::ptr_eq(&one, &two));
    assert!(!Arc::ptr_eq(&one, &other));
}
