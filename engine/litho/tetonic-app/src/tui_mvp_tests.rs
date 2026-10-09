//! Product-door tests with real management, tools, and brokered HTTP inference.
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(crate) async fn inference_server_with_tool(
    finish_tool: bool,
    tool: &str,
    arguments: Value,
) -> (String, Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    inference_server_with_behavior(finish_tool, tool, arguments, false).await
}

pub(crate) async fn inference_server_with_behavior(
    finish_tool: bool,
    tool: &str,
    arguments: Value,
    hang_chat: bool,
) -> (String, Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    inference_server_with_usage(finish_tool, tool, arguments, hang_chat, Some((10, 20))).await
}

pub(crate) async fn inference_server_with_usage(
    finish_tool: bool,
    tool: &str,
    arguments: Value,
    hang_chat: bool,
    usage: Option<(u64, u64)>,
) -> (String, Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    let tool = tool.to_owned();
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
            if hang_chat && header.starts_with("POST /api/chat ") {
                let request: Value =
                    serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
                captured.lock().unwrap().push(request);
                std::future::pending::<()>().await;
            }
            let response = if header.starts_with("POST /api/chat ") {
                let request: Value =
                    serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
                let mut requests = captured.lock().unwrap();
                requests.push(request);
                let message = if tool == "structured" {
                    json!({"role":"assistant","content":arguments["summary"]})
                } else if tool == "sequence"
                    && requests.len() <= arguments.as_array().unwrap().len()
                {
                    json!({"role":"assistant","content":"","tool_calls":[{"function":arguments[requests.len()-1]}]})
                } else if requests.len() == 1 {
                    json!({"role":"assistant", "content":"", "tool_calls":[{"function":{"name":tool,"arguments":arguments.clone()}}]})
                } else if finish_tool {
                    json!({"role":"assistant", "content":"", "tool_calls":[{"function":{"name":"finish","arguments":{"summary":"The fixture contains cobalt orchard."}}}]})
                } else {
                    json!({"role":"assistant", "content":"The fixture contains cobalt orchard."})
                };
                let mut reply = json!({"model":"qwen3.5:latest", "message":message, "done":true});
                if let Some((input, output)) = usage {
                    reply["prompt_eval_count"] = json!(input);
                    reply["eval_count"] = json!(output);
                }
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
