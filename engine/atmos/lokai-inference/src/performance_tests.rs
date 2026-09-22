//! Deterministic HTTP regressions: no live model or GPU required.
use super::*;
use std::sync::atomic::AtomicUsize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Server {
    task: tokio::task::JoinHandle<()>,
    requests: Arc<std::sync::Mutex<Vec<String>>>,
    peak: Arc<AtomicUsize>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn server(fail_first_ps: bool) -> (OllamaProvider, Server) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let guard = Arc::new(EgressGuard::new());
    guard.configure_loopback_inference(address.port());
    let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
    let peak = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    let ps_count = Arc::new(AtomicUsize::new(0));
    let (seen, high) = (requests.clone(), peak.clone());
    let task = tokio::spawn(async move {
        let mut handlers = tokio::task::JoinSet::new();
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (seen, high, active, ps_count) =
                (seen.clone(), high.clone(), active.clone(), ps_count.clone());
            handlers.spawn(async move {
                let mut bytes = Vec::new();
                let path = loop {
                    let mut buf = [0; 4096];
                    let n = socket.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(end) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]);
                        let length = header.lines().find_map(|l| {
                            l.to_lowercase().strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        }).unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break header.split_whitespace().nth(1).unwrap().to_string();
                        }
                    }
                };
                seen.lock().unwrap().push(path.clone());
                let mut status = "200 OK";
                let body = match path.as_str() {
                    "/api/tags" => json!({"models": (0..8).map(|i|
                        json!({"name": format!("model-{i}")})).collect::<Vec<_>>()}),
                    "/api/ps" => {
                        let count = ps_count.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                        if fail_first_ps && count == 0 {
                            status = "500 Internal Server Error";
                            json!({"error":"temporary failure"})
                        } else {
                            json!({"models":[{"name":"model-0","size":100,"size_vram":100},
                                {"name":"draft","size":10,"size_vram":10}]})
                        }
                    }
                    "/api/show" => {
                        let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                        high.fetch_max(count, Ordering::SeqCst);
                        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                        active.fetch_sub(1, Ordering::SeqCst);
                        json!({"capabilities":["tools"],"details":{"parameter_size":"3B"}})
                    }
                    "/api/generate" => json!({"done":true}),
                    "/api/chat" => json!({"message":{"role":"assistant","content":"unchanged"},
                        "done":true,"eval_count":1,"eval_duration":1000000}),
                    _ => panic!("Unexpected request: {path}"),
                };
                let body = format!("{body}\n");
                let reply = format!("HTTP/1.1 {status}\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                socket.write_all(reply.as_bytes()).await.unwrap();
            });
        }
    });
    (
        OllamaProvider::new(format!("http://{address}"), guard),
        Server {
            task,
            requests,
            peak,
        },
    )
}

#[tokio::test]
async fn simultaneous_status_reads_share_one_request_and_invalidation_refreshes() {
    let (provider, server) = server(false).await;
    let results = futures_util::future::join_all((0..16).map(|_| provider.get_ps_cached())).await;
    assert!(results
        .iter()
        .all(|r| r.as_ref().unwrap()["models"][1]["name"] == "draft"));
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    provider.invalidate_ps_cache().await;
    provider.get_ps_cached().await.unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn failed_status_read_is_not_cached() {
    let (provider, server) = server(true).await;
    assert!(provider.get_ps_cached().await.is_err());
    assert!(provider.get_ps_cached().await.is_ok());
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn inventory_reads_overlap_with_a_bound_and_preserve_order() {
    let (provider, server) = server(false).await;
    let inventory = provider
        .model_inventory_capabilities(&["model-0".into()])
        .await;
    assert_eq!(
        inventory
            .iter()
            .map(|m| m.local_name.clone())
            .collect::<Vec<_>>(),
        (0..8).map(|i| format!("model-{i}")).collect::<Vec<_>>()
    );
    assert!(inventory.iter().all(|m| m.tool_call_support));
    assert_eq!(
        inventory[0].load_state,
        lokai_fabric_protocol::ModelLoadState::Warm
    );
    assert_eq!(
        inventory[1].load_state,
        lokai_fabric_protocol::ModelLoadState::Cold
    );
    let peak = server.peak.load(Ordering::SeqCst);
    assert!(peak > 1 && peak <= 4, "peak={peak}");
}

#[tokio::test]
async fn snapshot_fetches_inventory_once() {
    let (provider, server) = server(false).await;
    let snapshot = provider.local_fabric_snapshot().await;
    assert!(snapshot.nodes[0].healthy);
    assert_eq!(
        *server.requests.lock().unwrap(),
        vec!["/api/tags", "/api/ps"]
    );
}

#[tokio::test]
async fn chat_preserves_other_resident_models_and_streamed_output() {
    let (provider, server) = server(false).await;
    let mut tokens = String::new();
    let response = provider
        .chat(
            ChatRequest {
                model: "model-0".into(),
                messages: vec![Message::user("test")],
                outbound_scan: OutboundScan::from_scan(false),
                ..Default::default()
            },
            &mut |t| tokens.push_str(t),
        )
        .await
        .unwrap();
    assert_eq!(tokens, "unchanged");
    assert_eq!(response.message.content, tokens);
    assert_eq!(
        *server.requests.lock().unwrap(),
        vec![
            "/api/ps",
            "/api/generate",
            "/api/ps",
            "/api/chat",
            "/api/ps"
        ]
    );
}

#[tokio::test]
#[ignore = "isolated metadata I/O benchmark; not model or task latency"]
async fn benchmark_inventory_metadata_reads() {
    let (provider, _server) = server(false).await;
    let started = std::time::Instant::now();
    let tags = provider.list_model_tags().await.unwrap();
    for tag in tags {
        provider.model_info(&tag.name).await.unwrap();
    }
    let sequential = started.elapsed().as_secs_f64();
    let started = std::time::Instant::now();
    let inventory = provider.model_inventory_capabilities(&[]).await;
    let concurrent = started.elapsed().as_secs_f64();
    assert_eq!(inventory.len(), 8);
    println!("8 independent metadata reads, 30ms simulated service time: sequential={:.2}ms concurrent={:.2}ms speedup={:.2}x (I/O path only)", sequential * 1000.0, concurrent * 1000.0, sequential / concurrent);
}
