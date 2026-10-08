//! Deterministic local HTTP fixture, no paid or installed model.
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub(super) async fn inference() -> (String, Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let (header_end, length) = loop {
                let mut buf = [0; 4096];
                let n = stream.read(&mut buf).await.unwrap();
                if n == 0 {
                    return;
                }
                bytes.extend_from_slice(&buf[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    break (end + 4, length);
                }
            };
            while bytes.len() < header_end + length {
                let mut buf = [0; 4096];
                let n = stream.read(&mut buf).await.unwrap();
                assert_ne!(n, 0);
                bytes.extend_from_slice(&buf[..n]);
            }
            let header = String::from_utf8_lossy(&bytes[..header_end]);
            let response = if header.starts_with("POST /api/chat ") {
                let request: Value =
                    serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
                let mut requests = captured.lock().unwrap();
                requests.push(request);
                let (tool, arguments) = match requests.len() {
                    1 => ("read_file", json!({"path":"fixture.txt"})),
                    2 => (
                        "ask_human",
                        json!({"question":"Who is the audience?", "why":"The tone depends on their experience", "options":["Beginners", "Experts"]}),
                    ),
                    3 => ("finish", json!({"summary":"Cobalt orchard for beginners"})),
                    _ => panic!("completed inference must not replay"),
                };
                let message = json!({"role":"assistant", "content":"", "tool_calls":[{"function":{"name":tool,"arguments":arguments}}]});
                let mut reply = json!({"model":"qwen3.5:latest", "message":message, "done":true});
                reply["prompt_eval_count"] = json!(10);
                reply["eval_count"] = json!(20);
                reply
            } else if header.starts_with("POST /api/generate ") {
                json!({"model":"qwen3.5:latest", "done":true, "response":""})
            } else {
                json!({"models":[{"name":"qwen3.5:latest", "model":"qwen3.5:latest", "size":1, "size_vram":1}], "capabilities":["completion","tools"]})
            };
            let body = format!("{response}\n");
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });
    (url, requests, task)
}
