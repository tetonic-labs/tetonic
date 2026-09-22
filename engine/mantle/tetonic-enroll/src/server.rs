use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::time::Instant;
use tokio_rustls::TlsAcceptor;
use tracing::info;

use crate::bind_policy::plaintext_enrollment_permitted;
use crate::code::EnrollmentCode;
use crate::handshake::{
    verify_enrollment_request_signature, verify_secret_proof, EnrollCompleteRequest,
    EnrollCompleteResponse,
};
use crate::http::{read_http_post_json, write_http_response};
use crate::tls::build_enrollment_server_config;

// Enrollment is short-lived; these budgets include unauthenticated clients.
const MAX_CONNECTIONS: usize = 16;
const MAX_CONNECTIONS_PER_IP: usize = 4;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct EnrollmentServerConfig {
    pub code: EnrollmentCode,
    pub ttl: Duration,
    pub bind_host: String,
    /// Worker fabric TLS cert (DER) for HTTPS enrollment and returned to coordinator.
    pub fabric_tls_cert: Option<Vec<u8>>,
    /// Private key matching `fabric_tls_cert` (required when cert is set).
    pub fabric_tls_key: Option<Vec<u8>>,
}

#[derive(Debug)]
pub enum EnrollmentServerOutcome {
    Completed {
        coordinator_pubkey: Vec<u8>,
        label: String,
    },
    Expired,
    Failed(String),
}

/// Bind enrollment listener until success, TTL, or error. Single-use.
pub async fn run_enrollment_server(cfg: EnrollmentServerConfig) -> EnrollmentServerOutcome {
    if let Err(reason) = plaintext_enrollment_permitted(&cfg.bind_host) {
        return EnrollmentServerOutcome::Failed(reason);
    }

    let tls_acceptor = match (&cfg.fabric_tls_cert, &cfg.fabric_tls_key) {
        (Some(cert), Some(key)) => match build_enrollment_server_config(cert, key) {
            Ok(config) => Some(TlsAcceptor::from(config)),
            Err(e) => return EnrollmentServerOutcome::Failed(e.to_string()),
        },
        (None, None) => None,
        _ => {
            return EnrollmentServerOutcome::Failed(
                "enrollment TLS requires both fabric_tls_cert and fabric_tls_key".into(),
            );
        }
    };

    let addr = format!("{}:{}", cfg.bind_host, cfg.code.enroll_port);
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => return EnrollmentServerOutcome::Failed(format!("bind {addr}: {e}")),
    };
    info!(
        "enrollment listening on {addr} ({}, expires {})",
        if tls_acceptor.is_some() {
            "TLS"
        } else {
            "plain HTTP"
        },
        cfg.code.expires_at
    );

    let completed = Arc::new(Mutex::new(None::<(Vec<u8>, String)>));
    // The public code's expiry and local TTL both bound the entire listener,
    // including TLS, request reads, response writes, and completion locking.
    let remaining = (cfg.code.expires_at - Utc::now())
        .to_std()
        .unwrap_or_default();
    let deadline = Instant::now() + cfg.ttl.min(remaining);
    let secret = match cfg.code.secret_bytes() {
        Ok(s) => Arc::new(s),
        Err(e) => return EnrollmentServerOutcome::Failed(e.to_string()),
    };
    let cfg = Arc::new(cfg);
    let mut connections = tokio::task::JoinSet::new();
    let mut peers = std::collections::HashMap::<
        std::net::IpAddr,
        std::sync::Weak<tokio::sync::Semaphore>,
    >::new();
    let outcome = loop {
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline) => break EnrollmentServerOutcome::Expired,
            result = connections.join_next(), if !connections.is_empty() => {
                if matches!(result, Some(Ok(Some(())))) {
                    let done = completed.lock().await;
                    if let Some((pk, label)) = done.as_ref() {
                        break EnrollmentServerOutcome::Completed {
                            coordinator_pubkey: pk.clone(), label: label.clone(),
                        };
                    }
                }
            }
            accepted = listener.accept() => {
                let (mut stream, remote) = match accepted {
                    Ok(connection) => connection,
                    Err(error) => break EnrollmentServerOutcome::Failed(error.to_string()),
                };
                // Reject excess connections instead of creating waiting tasks.
                if connections.len() >= MAX_CONNECTIONS { continue; }
                peers.retain(|_, limit| limit.strong_count() > 0);
                let limit = peers.get(&remote.ip()).and_then(std::sync::Weak::upgrade)
                    .unwrap_or_else(|| {
                        let limit = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS_PER_IP));
                        peers.insert(remote.ip(), Arc::downgrade(&limit));
                        limit
                    });
                let Ok(permit) = limit.try_acquire_owned() else { continue; };
                let cfg = cfg.clone();
                let secret = secret.clone();
                let completed = completed.clone();
                let acceptor = tls_acceptor.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    let operation = async {
                        if let Some(acceptor) = acceptor {
                            let mut tls = acceptor.accept(stream).await.ok()?;
                            handle_connection(&mut tls, &cfg, &secret, &completed).await
                        } else {
                            handle_connection(&mut stream, &cfg, &secret, &completed).await
                        }
                    };
                    // Absolute, not reset by a client dripping bytes.
                    tokio::time::timeout_at(
                        deadline.min(Instant::now() + CONNECTION_TIMEOUT), operation,
                    ).await.ok().flatten()
                });
            }
        }
    };
    connections.abort_all();
    while connections.join_next().await.is_some() {}
    // A fully written success owns the slot even if the expiry branch won
    // the same scheduler turn. Never report expiry after acknowledging success.
    if let Some((coordinator_pubkey, label)) = completed.lock().await.take() {
        return EnrollmentServerOutcome::Completed {
            coordinator_pubkey,
            label,
        };
    }
    outcome
}

type EnrollmentCompletionSlot = Arc<Mutex<Option<(Vec<u8>, String)>>>;

async fn handle_connection<S>(
    stream: &mut S,
    cfg: &EnrollmentServerConfig,
    secret: &[u8],
    completed: &EnrollmentCompletionSlot,
) -> Option<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    if Utc::now() > cfg.code.expires_at {
        let _ = write_http_response(stream, 400, r#"{"ok":false,"error":"expired"}"#).await;
        return None;
    }

    let req = match read_http_post_json(stream).await {
        Some(v) => v,
        None => {
            let _ = write_http_response(stream, 400, r#"{"ok":false,"error":"bad request"}"#).await;
            return None;
        }
    };
    let Ok(body): Result<EnrollCompleteRequest, _> = serde_json::from_value(req) else {
        let _ = write_http_response(stream, 400, r#"{"ok":false,"error":"invalid json"}"#).await;
        return None;
    };

    if body.coordinator_pubkey.to_verifying().is_err() {
        let _ = write_http_response(
            stream,
            400,
            r#"{"ok":false,"error":"invalid coordinator key"}"#,
        )
        .await;
        return None;
    }

    if !verify_secret_proof(
        secret,
        &cfg.code.worker_pubkey.0,
        &body.coordinator_pubkey.0,
        &body.secret_proof,
    ) {
        let _ = write_http_response(stream, 403, r#"{"ok":false,"error":"invalid secret"}"#).await;
        return None;
    }

    if !verify_enrollment_request_signature(
        &body.coordinator_pubkey.0,
        &cfg.code.worker_pubkey.0,
        &body.label,
        &body.secret_proof,
        &body.coordinator_signature_b64,
    ) {
        let _ = write_http_response(
            stream,
            403,
            r#"{"ok":false,"error":"invalid coordinator signature"}"#,
        )
        .await;
        return None;
    }

    let mut done = completed.lock().await;
    if done.is_some() {
        let _ =
            write_http_response(stream, 409, r#"{"ok":false,"error":"already enrolled"}"#).await;
        return None;
    }

    let fabric_tls_cert_b64 = cfg
        .fabric_tls_cert
        .as_ref()
        .map(|c| base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, c));
    let resp = EnrollCompleteResponse {
        ok: true,
        error: None,
        worker_id: cfg.code.worker_id.clone(),
        worker_pubkey: cfg.code.worker_pubkey.clone(),
        audit_pubkey: cfg.code.audit_pubkey.clone(),
        fabric_port: cfg.code.fabric_port,
        host: cfg.code.host.clone(),
        fabric_tls_cert_b64,
    };
    let json = serde_json::to_string(&resp).unwrap_or_else(|_| r#"{"ok":false}"#.into());
    if write_http_response(stream, 200, &json).await.is_err() {
        return None;
    }

    *done = Some((body.coordinator_pubkey.0.clone(), body.label.clone()));
    info!(
        "enrollment completed for coordinator {}",
        hex_fp(&body.coordinator_pubkey.0)
    );
    Some(())
}

fn hex_fp(pk: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(pk);
    h.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::{EnrollmentCode, DEFAULT_ENROLL_TTL, DEFAULT_FABRIC_PORT};
    use crate::crypto::KeyPair;
    use crate::handshake::{compute_secret_proof, sign_enrollment_request};

    fn enroll_request(
        coord: &KeyPair,
        code: &EnrollmentCode,
        secret: &[u8],
        label: &str,
    ) -> EnrollCompleteRequest {
        let proof = compute_secret_proof(secret, &code.worker_pubkey.0, &coord.public().0);
        EnrollCompleteRequest {
            secret_proof: proof.clone(),
            coordinator_pubkey: coord.public(),
            coordinator_signature_b64: sign_enrollment_request(
                coord,
                &code.worker_pubkey.0,
                &proof,
                label,
            ),
            label: label.into(),
        }
    }

    #[tokio::test]
    async fn server_accepts_valid_enrollment() {
        let worker = KeyPair::generate();
        let audit = KeyPair::generate();
        let coord = KeyPair::generate();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let code = EnrollmentCode::new(
            &worker,
            &audit,
            "127.0.0.1",
            port,
            DEFAULT_FABRIC_PORT,
            DEFAULT_ENROLL_TTL,
            None,
        );
        let secret = code.secret_bytes().unwrap();
        let body = enroll_request(&coord, &code, &secret, "test-gpu");

        let cfg = EnrollmentServerConfig {
            code: code.clone(),
            ttl: Duration::from_secs(5),
            bind_host: "127.0.0.1".into(),
            fabric_tls_cert: None,
            fabric_tls_key: None,
        };

        let port = code.enroll_port;
        let handle = tokio::spawn(async move { run_enrollment_server(cfg).await });

        tokio::time::sleep(Duration::from_millis(100)).await;

        let client = reqwest::Client::new();
        let url = format!("http://127.0.0.1:{port}/v1/enroll/complete");
        let resp = client.post(&url).json(&body).send().await.unwrap();
        assert!(resp.status().is_success());

        match handle.await.unwrap() {
            EnrollmentServerOutcome::Completed { label, .. } => assert_eq!(label, "test-gpu"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn server_rejects_invalid_secret() {
        let worker = KeyPair::generate();
        let audit = KeyPair::generate();
        let coord = KeyPair::generate();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let code = EnrollmentCode::new(
            &worker,
            &audit,
            "127.0.0.1",
            port,
            DEFAULT_FABRIC_PORT,
            DEFAULT_ENROLL_TTL,
            None,
        );

        let cfg = EnrollmentServerConfig {
            code: code.clone(),
            ttl: Duration::from_secs(5),
            bind_host: "127.0.0.1".into(),
            fabric_tls_cert: None,
            fabric_tls_key: None,
        };

        let enroll_port = code.enroll_port;
        let handle = tokio::spawn(async move { run_enrollment_server(cfg).await });

        tokio::time::sleep(Duration::from_millis(100)).await;

        let body = EnrollCompleteRequest {
            secret_proof: "invalid-proof".into(),
            coordinator_pubkey: coord.public(),
            coordinator_signature_b64: "bad".into(),
            label: "bad".into(),
        };
        let client = reqwest::Client::new();
        let url = format!("http://127.0.0.1:{enroll_port}/v1/enroll/complete");
        let resp = client.post(&url).json(&body).send().await.unwrap();
        assert_eq!(resp.status(), 403);

        handle.abort();
    }

    #[tokio::test]
    async fn server_rejects_bad_coordinator_signature() {
        let worker = KeyPair::generate();
        let audit = KeyPair::generate();
        let coord = KeyPair::generate();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let code = EnrollmentCode::new(
            &worker,
            &audit,
            "127.0.0.1",
            port,
            DEFAULT_FABRIC_PORT,
            DEFAULT_ENROLL_TTL,
            None,
        );
        let secret = code.secret_bytes().unwrap();
        let mut body = enroll_request(&coord, &code, &secret, "gpu");
        body.coordinator_signature_b64 = "AAAA".into();

        let cfg = EnrollmentServerConfig {
            code: code.clone(),
            ttl: Duration::from_secs(5),
            bind_host: "127.0.0.1".into(),
            fabric_tls_cert: None,
            fabric_tls_key: None,
        };

        let enroll_port = code.enroll_port;
        let handle = tokio::spawn(async move { run_enrollment_server(cfg).await });
        tokio::time::sleep(Duration::from_millis(100)).await;

        let client = reqwest::Client::new();
        let url = format!("http://127.0.0.1:{enroll_port}/v1/enroll/complete");
        let resp = client.post(&url).json(&body).send().await.unwrap();
        assert_eq!(resp.status(), 403);
        handle.abort();
    }

    #[tokio::test]
    async fn parallel_enrollment_only_one_succeeds() {
        let worker = KeyPair::generate();
        let audit = KeyPair::generate();
        let coord_a = KeyPair::generate();
        let coord_b = KeyPair::generate();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let code = EnrollmentCode::new(
            &worker,
            &audit,
            "127.0.0.1",
            port,
            DEFAULT_FABRIC_PORT,
            DEFAULT_ENROLL_TTL,
            None,
        );
        let secret = code.secret_bytes().unwrap();

        let cfg = EnrollmentServerConfig {
            code: code.clone(),
            ttl: Duration::from_secs(5),
            bind_host: "127.0.0.1".into(),
            fabric_tls_cert: None,
            fabric_tls_key: None,
        };

        let enroll_port = code.enroll_port;
        let handle = tokio::spawn(async move { run_enrollment_server(cfg).await });

        tokio::time::sleep(Duration::from_millis(100)).await;

        let url = format!("http://127.0.0.1:{enroll_port}/v1/enroll/complete");
        let client = reqwest::Client::new();
        let (resp_a, resp_b) = tokio::join!(
            client
                .post(&url)
                .json(&enroll_request(&coord_a, &code, &secret, "a"))
                .send(),
            client
                .post(&url)
                .json(&enroll_request(&coord_b, &code, &secret, "b"))
                .send(),
        );

        let ok_count = [resp_a, resp_b]
            .into_iter()
            .filter(|r| {
                r.as_ref()
                    .map(|resp| resp.status().is_success())
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(ok_count, 1, "exactly one parallel enrollment may succeed");

        match handle.await.unwrap() {
            EnrollmentServerOutcome::Completed { .. } => {}
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn server_rejects_non_loopback_plaintext_bind() {
        let worker = KeyPair::generate();
        let audit = KeyPair::generate();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let code = EnrollmentCode::new(
            &worker,
            &audit,
            "192.168.1.10",
            port,
            DEFAULT_FABRIC_PORT,
            DEFAULT_ENROLL_TTL,
            None,
        );
        let cfg = EnrollmentServerConfig {
            code,
            ttl: DEFAULT_ENROLL_TTL,
            bind_host: "0.0.0.0".into(),
            fabric_tls_cert: None,
            fabric_tls_key: None,
        };

        match run_enrollment_server(cfg).await {
            EnrollmentServerOutcome::Failed(msg) => {
                assert!(msg.contains("non-loopback"), "got: {msg}");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    fn deadline_config(tls: bool, ttl: Duration) -> EnrollmentServerConfig {
        let worker = KeyPair::generate();
        let audit = KeyPair::generate();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let code = EnrollmentCode::new(
            &worker,
            &audit,
            "127.0.0.1",
            port,
            DEFAULT_FABRIC_PORT,
            DEFAULT_ENROLL_TTL,
            None,
        );
        let (cert, key) = if tls {
            let (cert, key) = tetonic_node::issue_self_signed_cert("worker").unwrap();
            (Some(cert), Some(key))
        } else {
            (None, None)
        };
        EnrollmentServerConfig {
            code,
            ttl,
            bind_host: "127.0.0.1".into(),
            fabric_tls_cert: cert,
            fabric_tls_key: key,
        }
    }

    async fn connect_when_ready(port: u16) -> tokio::net::TcpStream {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(Ok(socket)) = tokio::time::timeout(
                    Duration::from_millis(50),
                    tokio::net::TcpStream::connect(("127.0.0.1", port)),
                )
                .await
                {
                    return socket;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn expiry_closes_silent_tls_and_plaintext_connections() {
        use tokio::io::AsyncReadExt;
        for tls in [false, true] {
            let cfg = deadline_config(tls, Duration::from_millis(400));
            let port = cfg.code.enroll_port;
            let task = tokio::spawn(run_enrollment_server(cfg));
            let mut silent = connect_when_ready(port).await;
            let outcome = tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .expect("a stalled handshake/read must not defeat TTL")
                .unwrap();
            assert!(matches!(outcome, EnrollmentServerOutcome::Expired));
            let mut byte = [0];
            let result = tokio::time::timeout(Duration::from_secs(1), silent.read(&mut byte))
                .await
                .unwrap();
            assert!(
                matches!(result, Ok(0) | Err(_)),
                "expiry must join and close sockets"
            );
        }
    }

    #[tokio::test]
    async fn stalled_request_does_not_block_valid_enrollment() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let cfg = deadline_config(false, Duration::from_secs(5));
        let port = cfg.code.enroll_port;
        let coord = KeyPair::generate();
        let body = enroll_request(
            &coord,
            &cfg.code,
            &cfg.code.secret_bytes().unwrap(),
            "valid",
        );
        let task = tokio::spawn(run_enrollment_server(cfg));
        let mut silent = connect_when_ready(port).await;
        silent.write_all(b"POST /v1/enroll/complete HTTP/1.1\r\nHost: worker\r\nContent-Length: 400\r\n\r\n{").await.unwrap();
        let response = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{port}/v1/enroll/complete"))
            .timeout(Duration::from_secs(2))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        let outcome = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(outcome, EnrollmentServerOutcome::Completed { label, .. } if label == "valid")
        );
        let mut byte = [0];
        let result = tokio::time::timeout(Duration::from_secs(1), silent.read(&mut byte))
            .await
            .unwrap();
        assert!(matches!(result, Ok(0) | Err(_)));
    }
}
