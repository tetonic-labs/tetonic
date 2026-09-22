//! Fabric listener — mTLS + default-deny ingress (N0.2).

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use lokai_memory::WorkerStore;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio_rustls::TlsAcceptor;
use tracing::info;

use crate::bind::resolve_listen_host;
use crate::conn::serve_fabric_connection;
use crate::event::{IngressEvent, IngressLog};
use crate::fabric::{self, fabric_state};
use crate::limits::{MAX_FABRIC_CONNECTIONS, MAX_FABRIC_CONNECTIONS_PER_IP, TLS_HANDSHAKE_TIMEOUT};
use crate::scheduler::WorkerScheduler;
use crate::tls::build_server_config;
#[cfg(test)]
use crate::tls::issue_self_signed_cert;
use crate::trust::TrustStore;
use lokai_enroll::DEFAULT_FABRIC_PORT;

#[derive(Debug, Clone)]
pub struct FabricListenConfig {
    pub bind_host: String,
    pub port: u16,
    pub ollama_base: String,
}

impl Default for FabricListenConfig {
    fn default() -> Self {
        let (host, _) = resolve_listen_host();
        Self {
            bind_host: host,
            port: DEFAULT_FABRIC_PORT,
            ollama_base: "http://localhost:11434".into(),
        }
    }
}

pub struct FabricServer {
    log: IngressLog,
    keys: Arc<dyn lokai_domain::key_storage::KeyStorage>,
    scheduler: Arc<WorkerScheduler>,
}

impl FabricServer {
    pub fn new() -> Self {
        Self::with_key_storage(Arc::new(lokai_secrets::key_storage::PlatformKeyStorage))
    }

    pub fn with_key_storage(keys: Arc<dyn lokai_domain::key_storage::KeyStorage>) -> Self {
        Self {
            keys,
            log: IngressLog::default(),
            scheduler: Arc::new(WorkerScheduler::new()),
        }
    }

    pub fn scheduler(&self) -> &Arc<WorkerScheduler> {
        &self.scheduler
    }

    pub fn log(&self) -> &IngressLog {
        &self.log
    }

    /// Run fabric listener until error, shutdown, or process exit.
    pub async fn run(
        &self,
        store: &WorkerStore,
        cfg: FabricListenConfig,
        shutdown: Arc<AtomicBool>,
    ) -> anyhow::Result<()> {
        let pins = store.list_coordinator_pins()?;
        let trust = Arc::new(TrustStore::from_coordinator_pins(&pins)?);
        let worker_db_path = store
            .db_path()
            .map_err(|e| anyhow::anyhow!("worker db path: {e}"))?
            .to_path_buf();
        let log = self.log.with_db(&worker_db_path);

        let identity = crate::load_or_create_tls_identity(store, self.keys.as_ref())?;
        let tls_config = build_server_config(
            &identity.certificate,
            identity.private_key.as_ref(),
            trust.clone(),
        )?;
        let acceptor = TlsAcceptor::from(tls_config);

        let addr: SocketAddr = format!("{}:{}", cfg.bind_host, cfg.port).parse()?;
        let listener = TcpListener::bind(addr).await?;
        info!("fabric listening on {addr} (mTLS required, owner coordinators only)");

        let ollama = fabric::default_ollama(&cfg.ollama_base);
        let ollama_base = cfg.ollama_base.clone();
        let scheduler = self.scheduler.clone();
        let conn_limit = Arc::new(Semaphore::new(MAX_FABRIC_CONNECTIONS));
        let mut connections = tokio::task::JoinSet::new();
        let mut peers =
            std::collections::HashMap::<std::net::IpAddr, std::sync::Weak<Semaphore>>::new();

        let result = loop {
            // Reap completed tasks before admitting more: released permits
            // must not allow an unbounded backlog of completed JoinSet entries.
            while connections.try_join_next().is_some() {}
            if shutdown.load(Ordering::Relaxed) {
                info!("fabric listener stopped (shutdown)");
                break Ok(());
            }
            let (stream, remote) = tokio::select! {
                res = listener.accept() => match res {
                    Ok(connection) => connection,
                    Err(error) => break Err(error.into()),
                },
                _ = connections.join_next(), if !connections.is_empty() => continue,
                _ = tokio::time::sleep(Duration::from_millis(100)) => continue,
            };
            if shutdown.load(Ordering::Relaxed) {
                drop(stream);
                break Ok(());
            }

            let permit = match conn_limit.clone().try_acquire_owned() {
                Ok(p) => p,
                Err(_) => {
                    log.record(IngressEvent::deny(remote, "connection limit exceeded"));
                    drop(stream);
                    continue;
                }
            };
            // Weak entries disappear after the last task releases its permit.
            // Pruning on admission bounds this map by active connections.
            peers.retain(|_, limit| limit.strong_count() > 0);
            let peer_limit = peers
                .get(&remote.ip())
                .and_then(std::sync::Weak::upgrade)
                .unwrap_or_else(|| {
                    let limit = Arc::new(Semaphore::new(MAX_FABRIC_CONNECTIONS_PER_IP));
                    peers.insert(remote.ip(), Arc::downgrade(&limit));
                    limit
                });
            let Ok(peer_permit) = peer_limit.try_acquire_owned() else {
                log.record(IngressEvent::deny(
                    remote,
                    "per-source connection limit exceeded",
                ));
                continue;
            };

            let acceptor = acceptor.clone();
            let log = log.clone();
            let ollama = ollama.clone();
            let ollama_base = ollama_base.clone();
            let trust = trust.clone();
            let worker_db_path = worker_db_path.clone();
            let scheduler = scheduler.clone();

            connections.spawn(async move {
                let _permit = permit;
                let _peer_permit = peer_permit;
                let tls = match tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, acceptor.accept(stream))
                    .await
                {
                    Ok(Ok(s)) => s,
                    Ok(Err(e)) => {
                        log.record(IngressEvent::deny(
                            remote,
                            format!("tls handshake failed: {e}"),
                        ));
                        return;
                    }
                    Err(_) => {
                        log.record(IngressEvent::deny(
                            remote,
                            "tls handshake deadline exceeded",
                        ));
                        return;
                    }
                };

                let coordinator_pk = tls
                    .get_ref()
                    .1
                    .peer_certificates()
                    .and_then(|certs| fabric::coordinator_pk_from_tls_certs(certs))
                    .unwrap_or([0u8; 32]);
                let peer_id = if coordinator_pk != [0u8; 32] {
                    TrustStore::peer_id_for_pubkey(&coordinator_pk)
                } else {
                    "unknown".into()
                };
                let estate_id = trust
                    .estate_id_for(&coordinator_pk)
                    .unwrap_or_else(|| "estate_local".into());

                let state = fabric_state(
                    peer_id,
                    coordinator_pk,
                    ollama,
                    ollama_base,
                    trust,
                    worker_db_path,
                    estate_id,
                    scheduler,
                );

                serve_fabric_connection(tls, remote, state, log).await;
            });
        };
        connections.abort_all();
        while connections.join_next().await.is_some() {}
        result
    }
}

impl Default for FabricServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lokai_enroll::KeyPair;
    use lokai_inference::{DataClass, FabricJob, JobPriority};
    use lokai_memory::WorkerStore;
    use rustls::pki_types::ServerName;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_rustls::TlsConnector;

    use super::*;
    use crate::conn::serve_fabric_connection;
    use crate::event::IngressDecision;
    use crate::fabric::{self, fabric_state};
    use crate::revoke::RevokeRequest;
    use crate::tls::{build_client_config, client_cert_from_keypair};

    async fn spawn_test_server(
        store: &WorkerStore,
        _coord: &KeyPair,
    ) -> (u16, Arc<FabricServer>, Arc<TrustStore>) {
        let server = Arc::new(FabricServer::new());
        let pins = store.list_coordinator_pins().unwrap();
        let trust = Arc::new(TrustStore::from_coordinator_pins(&pins).unwrap());
        let worker_db_path = store.db_path().unwrap().to_path_buf();
        let (cert, key) = issue_self_signed_cert("test-worker").unwrap();
        store
            .publish_tls_identity(
                None,
                &cert,
                &lokai_domain::key_storage::SecretKeyRef("test-only".into()),
            )
            .unwrap();
        let tls_config = build_server_config(&cert, &key, trust.clone()).unwrap();
        let acceptor = TlsAcceptor::from(tls_config);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server2 = server.clone();
        let trust_for_return = trust.clone();
        let scheduler = server2.scheduler().clone();
        let log = server2.log().with_db(&worker_db_path);

        tokio::spawn(async move {
            loop {
                let (stream, remote) = listener.accept().await.unwrap();
                let acceptor = acceptor.clone();
                let log = log.clone();
                let ollama = fabric::default_ollama("http://localhost:11434");
                let trust = trust.clone();
                let worker_db_path = worker_db_path.clone();
                let scheduler = scheduler.clone();
                tokio::spawn(async move {
                    let tls = match acceptor.accept(stream).await {
                        Ok(s) => s,
                        Err(e) => {
                            log.record(IngressEvent::deny(
                                remote,
                                format!("tls handshake failed: {e}"),
                            ));
                            return;
                        }
                    };
                    let coordinator_pk = tls
                        .get_ref()
                        .1
                        .peer_certificates()
                        .and_then(|certs| fabric::coordinator_pk_from_tls_certs(certs))
                        .unwrap_or([0u8; 32]);
                    let peer_id = if coordinator_pk != [0u8; 32] {
                        TrustStore::peer_id_for_pubkey(&coordinator_pk)
                    } else {
                        "unknown".into()
                    };
                    let estate_id = trust
                        .estate_id_for(&coordinator_pk)
                        .unwrap_or_else(|| "estate".into());
                    let state = fabric_state(
                        peer_id,
                        coordinator_pk,
                        ollama,
                        "http://localhost:11434".into(),
                        trust,
                        worker_db_path,
                        estate_id,
                        scheduler,
                    );
                    serve_fabric_connection(tls, remote, state, log).await;
                });
            }
        });

        (port, server, trust_for_return)
    }

    async fn tls_get(port: u16, store: &WorkerStore, coord: &KeyPair, req: &str) -> String {
        let cert = store.tls_identity().unwrap().unwrap().cert_der;
        let (client_cert, client_key) = client_cert_from_keypair(coord, "coord").unwrap();
        let client_config = build_client_config(&cert, &client_cert, &client_key).unwrap();
        let connector = TlsConnector::from(client_config);
        let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        let mut tls = connector
            .connect(ServerName::try_from("127.0.0.1").unwrap(), stream)
            .await
            .unwrap();
        tls.write_all(req.as_bytes()).await.unwrap();
        let mut body = String::new();
        tls.read_to_string(&mut body).await.unwrap();
        body
    }

    #[tokio::test]
    async fn unauthenticated_plain_probe_denied() {
        let dir = std::env::temp_dir().join(format!("lokai-node-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("worker.db");
        let store = WorkerStore::open(&db).unwrap();
        let coord = KeyPair::generate();
        store
            .upsert_coordinator_pin("estate", &coord.public().0, "test")
            .unwrap();

        let (port, server) = {
            let (p, s, _) = spawn_test_server(&store, &coord).await;
            (p, s)
        };
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        stream
            .write_all(b"GET /v1/health HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        let mut buf = [0u8; 256];
        let n = stream.read(&mut buf).await.unwrap_or(0);
        let snippet = std::str::from_utf8(&buf[..n]).unwrap_or("");
        assert!(
            !snippet.contains("200 OK") && !snippet.contains("\"healthy\""),
            "plain HTTP must not reach fabric routes"
        );

        tokio::time::sleep(Duration::from_millis(50)).await;
        let denies: Vec<_> = server
            .log()
            .events()
            .into_iter()
            .filter(|e| e.decision == IngressDecision::Deny)
            .collect();
        assert!(!denies.is_empty(), "expected deny ingress event");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn unknown_client_cert_denied() {
        let dir = std::env::temp_dir().join(format!("lokai-node-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("worker.db");
        let store = WorkerStore::open(&db).unwrap();
        let coord = KeyPair::generate();
        let bad = KeyPair::generate();
        store
            .upsert_coordinator_pin("estate", &coord.public().0, "test")
            .unwrap();

        let (port, server) = {
            let (p, s, _) = spawn_test_server(&store, &coord).await;
            (p, s)
        };
        tokio::time::sleep(Duration::from_millis(50)).await;

        let (cert, _key) = issue_self_signed_cert("test-worker").unwrap();
        let (bad_cert, bad_key) = client_cert_from_keypair(&bad, "bad").unwrap();
        let client_config = build_client_config(&cert, &bad_cert, &bad_key).unwrap();
        let connector = TlsConnector::from(client_config);
        let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        let result = connector
            .connect(ServerName::try_from("127.0.0.1").unwrap(), stream)
            .await;
        assert!(result.is_err());

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(server
            .log()
            .events()
            .iter()
            .any(|e| e.decision == IngressDecision::Deny));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn client_cert_without_matching_private_key_rejected() {
        let coord = KeyPair::generate();
        let impostor = KeyPair::generate();
        let (server_cert, _) = issue_self_signed_cert("worker").unwrap();
        let (stolen_cert, _) = client_cert_from_keypair(&coord, "coord").unwrap();
        let (_, impostor_key) = client_cert_from_keypair(&impostor, "bad").unwrap();
        assert!(
            build_client_config(&server_cert, &stolen_cert, &impostor_key).is_err(),
            "cert/key mismatch must be rejected (SEC-001)"
        );
    }

    #[tokio::test]
    async fn pinned_client_reaches_health() {
        let dir = std::env::temp_dir().join(format!("lokai-node-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("worker.db");
        let store = WorkerStore::open(&db).unwrap();
        let coord = KeyPair::generate();
        store
            .upsert_coordinator_pin("estate", &coord.public().0, "test")
            .unwrap();

        let (port, server) = {
            let (p, s, _) = spawn_test_server(&store, &coord).await;
            (p, s)
        };
        tokio::time::sleep(Duration::from_millis(50)).await;

        let body = tls_get(
            port,
            &store,
            &coord,
            "GET /v1/health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(body.contains("200") || body.contains("\"healthy\""));
        assert!(server
            .log()
            .events()
            .iter()
            .any(|e| e.decision == IngressDecision::Allow));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn revoke_drops_pin_and_blocks_health() {
        let dir = std::env::temp_dir().join(format!("lokai-node-revoke-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("worker.db");
        let store = WorkerStore::open(&db).unwrap();
        let coord = KeyPair::generate();
        store
            .upsert_coordinator_pin("estate", &coord.public().0, "test")
            .unwrap();

        let (port, _server, trust) = spawn_test_server(&store, &coord).await;
        tokio::time::sleep(Duration::from_millis(50)).await;

        let body = RevokeRequest {
            coordinator_pubkey: coord.public(),
            epoch: 1,
            worker_id: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        let req = format!(
            "POST /v1/revoke HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
            json.len()
        );
        let resp = tls_get(port, &store, &coord, &req).await;
        assert!(resp.contains("200") || resp.contains("\"ok\":true"));

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(store.list_coordinator_pins().unwrap().is_empty());
        let mut pk = [0u8; 32];
        pk.copy_from_slice(&coord.public().0);
        assert!(!trust.is_pinned_pubkey(&pk));
        assert!(trust.authorize_owner_pubkey(&pk).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn private_data_class_logged_as_deny() {
        let dir = std::env::temp_dir().join(format!("lokai-node-private-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("worker.db");
        let store = WorkerStore::open(&db).unwrap();
        let coord = KeyPair::generate();
        store
            .upsert_coordinator_pin("estate", &coord.public().0, "test")
            .unwrap();

        let (port, server) = {
            let (p, s, _) = spawn_test_server(&store, &coord).await;
            (p, s)
        };
        tokio::time::sleep(Duration::from_millis(50)).await;

        let job = FabricJob {
            job_id: "j1".into(),
            attempt_id: None,
            estate_id: "estate".into(),
            session_id: None,
            agent_id: "a".into(),
            step_index: 0,
            model: "m".into(),
            tier: None,
            messages: vec![],
            tools: vec![],
            options: Default::default(),
            priority: JobPriority::OwnerInteractive,
            data_class: DataClass::Secret,
            disclosure_tier: Default::default(),
            audit_envelope: None,
            circle_id: None,
            consumer_peer_id: None,
            policy_epoch: 0,
            turn_affinity: None,
        };
        let json = serde_json::to_string(&job).unwrap();
        let req = format!(
            "POST /v1/chat HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
            json.len()
        );
        let resp = tls_get(port, &store, &coord, &req).await;
        assert!(resp.contains("403") || resp.contains("secret") || resp.contains("private"));

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            server
                .log()
                .events()
                .iter()
                .any(|e| e.decision == IngressDecision::Deny
                    && e.route.as_deref() == Some("/v1/chat")),
            "secret class must deny at ingress audit"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn stale_policy_epoch_logged_as_deny() {
        let dir = std::env::temp_dir().join(format!("lokai-node-epoch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("worker.db");
        let store = WorkerStore::open(&db).unwrap();
        let coord = KeyPair::generate();
        store
            .upsert_coordinator_pin("estate", &coord.public().0, "test")
            .unwrap();
        store
            .bump_coordinator_epoch("estate", &coord.public().0, 5)
            .unwrap();

        let (port, server) = {
            let (p, s, _) = spawn_test_server(&store, &coord).await;
            (p, s)
        };
        tokio::time::sleep(Duration::from_millis(50)).await;

        let job = FabricJob {
            job_id: "j1".into(),
            attempt_id: None,
            estate_id: "estate".into(),
            session_id: None,
            agent_id: "a".into(),
            step_index: 0,
            model: "m".into(),
            tier: None,
            messages: vec![],
            tools: vec![],
            options: Default::default(),
            priority: JobPriority::OwnerInteractive,
            data_class: DataClass::RepositorySource,
            disclosure_tier: Default::default(),
            audit_envelope: None,
            circle_id: None,
            consumer_peer_id: None,
            policy_epoch: 0,
            turn_affinity: None,
        };
        let json = serde_json::to_string(&job).unwrap();
        let req = format!(
            "POST /v1/chat HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
            json.len()
        );
        let resp = tls_get(port, &store, &coord, &req).await;
        assert!(resp.contains("403") || resp.contains("stale_policy_epoch"));

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            server
                .log()
                .events()
                .iter()
                .any(|e| e.decision == IngressDecision::Deny),
            "stale epoch must deny at ingress audit"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn ingress_events_persist_to_worker_db() {
        let dir = std::env::temp_dir().join(format!("lokai-node-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("worker.db");
        let store = WorkerStore::open(&db).unwrap();
        let coord = KeyPair::generate();
        store
            .upsert_coordinator_pin("estate", &coord.public().0, "test")
            .unwrap();

        let (port, _) = {
            let (p, s, _) = spawn_test_server(&store, &coord).await;
            (p, s)
        };
        tokio::time::sleep(Duration::from_millis(50)).await;

        let _ = tls_get(
            port,
            &store,
            &coord,
            "GET /v1/health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        )
        .await;
        tokio::time::sleep(Duration::from_millis(150)).await;

        let count = store.ingress_event_count().unwrap();
        assert!(count >= 1, "ingress rows must persist to worker.db");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
#[path = "ingress_deadline_tests.rs"]
mod ingress_deadline_tests;
