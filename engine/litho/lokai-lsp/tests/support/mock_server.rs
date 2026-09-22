//! Minimal stdio LSP stub for `lokai-lsp` integration tests.

use std::env;
use std::io::BufWriter;
use std::path::Path;

use lokai_lsp::framing::{read_frame, write_frame};
use serde_json::{json, Value};

fn main() {
    let crash_after_init = env::args().any(|a| a == "--crash-after-init");
    let stall_input = env::args().any(|a| a == "--stall-input");
    let root = env::current_dir().expect("cwd");
    let mut stdin = std::io::BufReader::new(std::io::stdin());
    let mut stdout = BufWriter::new(std::io::stdout());

    loop {
        let bytes = match read_frame(&mut stdin) {
            Ok(Some(b)) => b,
            Ok(None) => break,
            Err(e) => {
                eprintln!("mock read: {e}");
                break;
            }
        };
        let msg: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("mock json: {e}");
                continue;
            }
        };

        if msg.get("method").and_then(|m| m.as_str()) == Some("initialize") {
            let id = msg["id"].as_i64().unwrap_or(0);
            let resp = json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "capabilities": {},
                    "serverInfo": { "name": "lokai-mock", "version": "0" }
                }
            });
            write_frame(&mut stdout, resp.to_string().as_bytes()).unwrap();
            if stall_input {
                // Keep the pipe open without consuming it; the client must time
                // out its write rather than waiting for the child to exit.
                std::thread::sleep(std::time::Duration::from_secs(60));
                break;
            }
            if crash_after_init {
                break;
            }
            continue;
        }

        if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/definition") {
            let id = msg["id"].as_i64().unwrap_or(0);
            let uri = msg["params"]["textDocument"]["uri"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| file_uri(&root.join("main.rs")));
            let resp = json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "uri": uri,
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 3 }
                    }
                }
            });
            write_frame(&mut stdout, resp.to_string().as_bytes()).unwrap();
            continue;
        }

        if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/didOpen")
            || msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/didChange")
        {
            let uri = msg["params"]["textDocument"]["uri"].as_str().unwrap_or("");
            let note = json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": {
                    "uri": uri,
                    "diagnostics": [{
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 0, "character": 1 }
                        },
                        "severity": 1,
                        "message": "mock error",
                        "source": "mock"
                    }]
                }
            });
            write_frame(&mut stdout, note.to_string().as_bytes()).unwrap();
        }
    }
}

fn file_uri(path: &Path) -> String {
    url::Url::from_file_path(path)
        .expect("file uri")
        .to_string()
}
