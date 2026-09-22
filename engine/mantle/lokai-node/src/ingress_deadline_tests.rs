use super::*;
use lokai_domain::key_storage::KeyStorage;
use lokai_enroll::KeyPair;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpSocket, TcpStream};
use tokio_rustls::{client::TlsStream, TlsConnector};

struct Fixture {
    port: u16,
    connector: TlsConnector,
    shutdown: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<anyhow::Result<()>>,
    server: Arc<FabricServer>,
    _dir: tempfile::TempDir,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

impl Fixture {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkerStore::open(dir.path().join("worker.db")).unwrap();
        let coord = KeyPair::generate();
        store
            .upsert_coordinator_pin("estate", &coord.public().0, "test")
            .unwrap();
        let (cert, key) = issue_self_signed_cert("worker").unwrap();
        let keys = Arc::new(crate::tls_identity::tests::MemoryKeys::default());
        store
            .publish_tls_identity(None, &cert, &keys.create(&key).unwrap())
            .unwrap();
        let (client_cert, client_key) =
            crate::tls::client_cert_from_keypair(&coord, "coord").unwrap();
        let connector = TlsConnector::from(
            crate::tls::build_client_config(&cert, &client_cert, &client_key).unwrap(),
        );
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server = Arc::new(FabricServer::with_key_storage(keys));
        let task = {
            let shutdown = shutdown.clone();
            let server = server.clone();
            tokio::task::spawn_blocking(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(async move {
                        server
                            .run(
                                &store,
                                FabricListenConfig {
                                    bind_host: "127.0.0.1".into(),
                                    port,
                                    ollama_base: "http://127.0.0.1:1".into(),
                                },
                                shutdown,
                            )
                            .await
                    })
            })
        };
        let fixture = Self {
            port,
            connector,
            shutdown,
            task,
            server,
            _dir: dir,
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        fixture
    }

    async fn tls(&self) -> TlsStream<TcpStream> {
        let socket = TcpStream::connect(("127.0.0.1", self.port)).await.unwrap();
        self.connector
            .connect(ServerName::try_from("127.0.0.1").unwrap(), socket)
            .await
            .unwrap()
    }

    async fn stop(mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        tokio::time::timeout(Duration::from_secs(2), &mut self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}

#[tokio::test]
async fn silent_tls_expires_and_listener_remains_usable() {
    let fixture = Fixture::start().await;
    let mut silent = TcpStream::connect(("127.0.0.1", fixture.port))
        .await
        .unwrap();
    let mut byte = [0];
    let result = tokio::time::timeout(
        TLS_HANDSHAKE_TIMEOUT + Duration::from_secs(3),
        silent.read(&mut byte),
    )
    .await
    .unwrap();
    assert!(
        matches!(result, Ok(0) | Err(_)),
        "silent TLS socket must close"
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if fixture
                .server
                .log()
                .events()
                .iter()
                .any(|e| e.reason.contains("handshake deadline"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the deadline must also be recorded as a denial");
    let mut valid = fixture.tls().await;
    valid
        .write_all(b"GET /missing HTTP/1.1\r\nHost: worker\r\n\r\n")
        .await
        .unwrap();
    let mut response = String::new();
    tokio::time::timeout(Duration::from_secs(2), valid.read_to_string(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(response.starts_with("HTTP/1.1 404"));
    fixture.stop().await;
}

#[tokio::test]
async fn source_saturation_preserves_other_source_and_shutdown_closes_sockets() {
    let fixture = Fixture::start().await;
    let mut attackers = Vec::new();
    for _ in 0..MAX_FABRIC_CONNECTIONS_PER_IP {
        let socket = TcpSocket::new_v4().unwrap();
        socket.bind("127.0.0.2:0".parse().unwrap()).unwrap();
        attackers.push(
            socket
                .connect(([127, 0, 0, 1], fixture.port).into())
                .await
                .unwrap(),
        );
    }
    // Same-source excess must be refused before the TLS deadline elapses.
    let socket = TcpSocket::new_v4().unwrap();
    socket.bind("127.0.0.2:0".parse().unwrap()).unwrap();
    let mut excess = socket
        .connect(([127, 0, 0, 1], fixture.port).into())
        .await
        .unwrap();
    let mut byte = [0];
    let result = tokio::time::timeout(Duration::from_secs(2), excess.read(&mut byte))
        .await
        .unwrap();
    assert!(matches!(result, Ok(0) | Err(_)));
    let mut valid = fixture.tls().await;
    valid
        .write_all(b"GET /missing HTTP/1.1\r\nHost: worker\r\n\r\n")
        .await
        .unwrap();
    let mut response = String::new();
    tokio::time::timeout(Duration::from_secs(2), valid.read_to_string(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(response.starts_with("HTTP/1.1 404"));
    fixture.stop().await;
    for mut socket in attackers {
        let result = tokio::time::timeout(Duration::from_secs(1), socket.read(&mut byte))
            .await
            .unwrap();
        assert!(
            matches!(result, Ok(0) | Err(_)),
            "shutdown must close every tracked connection"
        );
    }
}

#[tokio::test]
async fn incomplete_headers_and_dripping_body_have_absolute_deadlines() {
    let fixture = Fixture::start().await;
    let mut headers = fixture.tls().await;
    headers
        .write_all(b"POST /v1/negotiate HTTP/1.1\r\nHost: ")
        .await
        .unwrap();
    let mut body = fixture.tls().await;
    body.write_all(
        b"POST /v1/negotiate HTTP/1.1\r\nHost: worker\r\nContent-Length: 100000\r\n\r\n",
    )
    .await
    .unwrap();
    let (mut reader, mut writer) = tokio::io::split(body);
    let dripper = tokio::spawn(async move {
        loop {
            if writer.write_all(b" ").await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
    let (header_result, body_result) = tokio::join!(
        async {
            let mut output = Vec::new();
            tokio::time::timeout(
                crate::limits::HEADER_TIMEOUT + Duration::from_secs(3),
                headers.read_to_end(&mut output),
            )
            .await
        },
        async {
            let mut output = String::new();
            tokio::time::timeout(
                crate::limits::BODY_TIMEOUT + Duration::from_secs(3),
                reader.read_to_string(&mut output),
            )
            .await
            .unwrap()
            .unwrap();
            output
        }
    );
    assert!(
        header_result.is_ok(),
        "incomplete headers must close before deadline"
    );
    dripper.abort();
    let _ = dripper.await;
    assert!(body_result.starts_with("HTTP/1.1 408"), "{body_result}");
    fixture.stop().await;
}
