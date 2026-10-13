//! Exercise the actual one-process UI/API entry without a frontend dev server.
use std::{
    io::{Read, Write},
    net::TcpStream,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Engine(Child);
impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(address: &str, path: &str, bearer: Option<&str>) -> (u16, String) {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let auth = bearer
        .map(|value| format!("Authorization: Bearer {value}\r\n"))
        .unwrap_or_default();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\n{auth}Connection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = response.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, response)
}

#[test]
fn built_product_and_authenticated_workspace_share_one_ephemeral_port() {
    let root = tempfile::tempdir().unwrap();
    let ui = root.path().join("ui");
    std::fs::create_dir_all(ui.join("assets")).unwrap();
    std::fs::write(ui.join("index.html"), "<main>PRODUCT_ENTRY</main>").unwrap();
    std::fs::write(ui.join("assets/app.js"), "/* PRODUCT_SCRIPT */").unwrap();
    let connection = root.path().join("connection.url");
    let mut engine = Engine(
        Command::new(env!("CARGO_BIN_EXE_tetonic"))
            .args([
                "ui",
                "--model",
                "unused-test-model",
                "--port",
                "0",
                "--database",
            ])
            .arg(root.path().join("workspace.db"))
            .arg("--ui-dir")
            .arg(&ui)
            .arg("--connection-file")
            .arg(&connection)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    let link = loop {
        assert!(
            engine.0.try_wait().unwrap().is_none(),
            "Engine exited before publishing its connection"
        );
        assert!(
            Instant::now() < deadline,
            "Engine did not publish its connection"
        );
        if let Ok(value) = std::fs::read_to_string(&connection) {
            if let Ok(link) = url::Url::parse(&value) {
                if link.fragment().is_some_and(|value| value.len() == 72) {
                    break link;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    let address = format!("{}:{}", link.host_str().unwrap(), link.port().unwrap());
    let token = link.fragment().unwrap().strip_prefix("connect=").unwrap();
    let (status, page) = get(&address, "/?engine=local", None);
    assert_eq!(status, 200);
    assert!(page.contains("PRODUCT_ENTRY"));
    assert!(!page.contains(token));
    assert_eq!(get(&address, "/assets/app.js", None).0, 200);
    assert_eq!(get(&address, "/connection.url", None).0, 404);
    assert_eq!(get(&address, "/api/local/workspace", None).0, 401);
    assert_eq!(get(&address, "/api/local/workspace", Some("wrong")).0, 401);
    let (status, snapshot) = get(&address, "/api/local/workspace", Some(token));
    assert_eq!(status, 200);
    assert!(snapshot.contains("\"tasks\":[]"));
    assert!(!snapshot.contains(token));
}
