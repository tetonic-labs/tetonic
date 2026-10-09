use super::super::client::VERSION;
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub(crate) struct Fixture {
    pub config: Vec<u8>,
    pub mode: Arc<AtomicU8>,
    pub calls: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    pub async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
        let mode = Arc::new(AtomicU8::new(0));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let state = mode.clone();
        let captured = calls.clone();
        let task = tokio::spawn(async move {
            let mut children = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    socket = listener.accept() => {
                        let (socket, _) = socket.unwrap();
                        children.spawn(serve(socket, state.clone(), captured.clone()));

                    }
                    Some(result) = children.join_next(), if !children.is_empty() => { result.unwrap(); }
                }
            }
        });
        Self { config: json!({"connections":[{"id":"calendar","name":"Calendar","endpoint":endpoint,"read_tools":["search","lookup"]}]}).to_string().into_bytes(), mode, calls, task }
    }
}

async fn serve(
    mut socket: tokio::net::TcpStream,
    mode: Arc<AtomicU8>,
    captured: Arc<Mutex<Vec<Value>>>,
) {
    let mut bytes = Vec::new();
    let (end, length) = loop {
        let mut buf = [0; 4096];
        let n = socket.read(&mut buf).await.unwrap_or(0);
        if n == 0 {
            return;
        }
        bytes.extend_from_slice(&buf[..n]);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    let (k, v) = line.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            break (end + 4, length);
        }
    };
    while bytes.len() < end + length {
        let mut buf = [0; 4096];
        let n = socket.read(&mut buf).await.unwrap_or(0);
        if n == 0 {
            return;
        }
        bytes.extend_from_slice(&buf[..n]);
    }
    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
    if headers.starts_with("delete ") {
        let _ = socket.write_all(b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        return;
    }
    let body: Value = serde_json::from_slice(&bytes[end..end + length]).unwrap();
    let method = body["method"].as_str().unwrap();
    captured.lock().unwrap().push(body.clone());
    assert!(headers.contains("accept: application/json, text/event-stream"));
    assert!(headers.contains("mcp-protocol-version: 2025-11-25"));
    if method != "initialize" {
        assert!(headers.contains("mcp-session-id: fixture-session"));
    }
    let mode = mode.load(Ordering::SeqCst);
    if mode == 6 || (mode == 8 && !headers.contains("authorization: bearer test_service_token")) {
        let _ = socket
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await;
        return;
    }
    if mode == 3 && method == "tools/call" {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    if mode == 11 && method == "tools/call" {
        // The remote action may already have happened; drop its response.
        return;
    }
    let result = match method {
        "initialize" => {
            json!({"protocolVersion":VERSION,"capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}})
        }
        "tools/list" => json!({"tools":[
            {"name":"search","description":if mode==1 {"changed"} else {"Search calendar availability"},"inputSchema":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]},"annotations":{"readOnlyHint":mode!=9}},
            {"name":"lookup","description":"Read a named calendar","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":true}},
            {"name":"delete_event","description":if mode==10 {"Changed delete"} else {"Delete event"},"inputSchema":{"type":"object"},"annotations":{"readOnlyHint":false}},
            {"name":"unannotated","inputSchema":{"type":"object"}},
            {"name":"background_only","inputSchema":{"type":"object"},"execution":{"taskSupport":"required"}}
        ]}),
        "tools/call" if mode == 2 => {
            json!({"isError":true,"content":[{"type":"text","text":"Target is not adjacent; navigate to it before interacting."}],"structuredContent":{"success":false,"reason":"not_adjacent"}})
        }
        "tools/call" if body["params"]["name"] == "delete_event" => {
            json!({"content":[{"type":"text","text":"Event deleted"}],"structuredContent":{"deleted":true}})
        }
        "tools/call" => {
            json!({"content":[{"type":"text","text":"Tuesday 10:00 is available"}],"structuredContent":{"available":true}})
        }
        _ => Value::Null,
    };
    let (status, mime, text) = if method.starts_with("notifications/") {
        (202, "application/json", String::new())
    } else {
        let response = if mode == 12 && method == "tools/call" {
            json!({"jsonrpc":"2.0","id":body["id"],"error":{"code":-32603,"message":"PRIVATE_SERVER_ERROR"}})
        } else {
            json!({"jsonrpc":"2.0","id":if mode==5 {json!(999)} else {body["id"].clone()},"result":result})
        };
        if mode == 4 {
            (
                200,
                "text/event-stream",
                format!("event: message\r\ndata: {response}\r\n\r\n"),
            )
        } else {
            (200, "application/json", response.to_string())
        }
    };
    let response=format!("HTTP/1.1 {status} OK\r\nContent-Type: {mime}\r\nMCP-Session-Id: fixture-session\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",text.len());
    let _ = socket.write_all(response.as_bytes()).await;
}
