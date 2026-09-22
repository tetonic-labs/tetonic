use std::net::IpAddr;

use ed25519_dalek::Signature;
use hmac::{Hmac, Mac};
use lokai_egress::EgressGuard;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use thiserror::Error;

use crate::code::{decode_enrollment_code, EnrollmentCode, ENROLL_CODE_PREFIX};
use crate::crypto::{KeyPair, PublicKeyBytes};
use crate::resolve::{self, ResolveError};

type HmacSha256 = Hmac<Sha256>;

pub const PROOF_CONTEXT: &[u8] = b"lokai-enroll-v1-proof";
pub const REQUEST_SIGN_CONTEXT: &[u8] = b"lokai-enroll-v2-request";

#[derive(Debug, Error)]
pub enum EnrollError {
    #[error("code: {0}")]
    Code(#[from] crate::code::CodeError),
    #[error("egress: {0}")]
    Egress(#[from] lokai_egress::EgressError),
    #[error("http enrollment failed: {0}")]
    Http(String),
    #[error("worker rejected enrollment: {0}")]
    Rejected(String),
    #[error("resolve host: {0}")]
    Resolve(String),
    #[error("enrollment response does not match code: {0}")]
    ResponseMismatch(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollCompleteRequest {
    pub secret_proof: String,
    pub coordinator_pubkey: PublicKeyBytes,
    pub coordinator_signature_b64: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollCompleteResponse {
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    pub worker_id: String,
    pub worker_pubkey: PublicKeyBytes,
    pub audit_pubkey: PublicKeyBytes,
    pub fabric_port: u16,
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fabric_tls_cert_b64: Option<String>,
}

fn compute_secret_proof_bytes(secret: &[u8], worker_pk: &[u8], coordinator_pk: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC key");
    mac.update(PROOF_CONTEXT);
    mac.update(worker_pk);
    mac.update(coordinator_pk);
    mac.finalize().into_bytes().into()
}

pub fn compute_secret_proof(secret: &[u8], worker_pk: &[u8], coordinator_pk: &[u8]) -> String {
    base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        compute_secret_proof_bytes(secret, worker_pk, coordinator_pk),
    )
}

pub fn verify_secret_proof(
    secret: &[u8],
    worker_pk: &[u8],
    coordinator_pk: &[u8],
    proof_b64: &str,
) -> bool {
    use base64::Engine;
    let expected = compute_secret_proof_bytes(secret, worker_pk, coordinator_pk);
    let Ok(got) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(proof_b64) else {
        return false;
    };
    got.ct_eq(&expected).into()
}

fn request_sign_message(
    worker_pk: &[u8],
    coordinator_pk: &[u8],
    label: &str,
    secret_proof: &str,
) -> Vec<u8> {
    let mut msg = Vec::new();
    msg.extend_from_slice(REQUEST_SIGN_CONTEXT);
    for field in [
        worker_pk,
        coordinator_pk,
        label.as_bytes(),
        secret_proof.as_bytes(),
    ] {
        msg.extend_from_slice(&(field.len() as u64).to_be_bytes());
        msg.extend_from_slice(field);
    }
    msg
}

pub fn sign_enrollment_request(
    coordinator: &KeyPair,
    worker_pk: &[u8],
    secret_proof: &str,
    label: &str,
) -> String {
    let coordinator_pk = coordinator.public().0;
    let sig = coordinator.sign(&request_sign_message(
        worker_pk,
        &coordinator_pk,
        label,
        secret_proof,
    ));
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, sig)
}

pub fn verify_enrollment_request_signature(
    coordinator_pk: &[u8],
    worker_pk: &[u8],
    label: &str,
    secret_proof: &str,
    signature_b64: &str,
) -> bool {
    use base64::Engine;
    let Ok(verifying) = PublicKeyBytes(coordinator_pk.to_vec()).to_verifying() else {
        return false;
    };
    let Ok(sig_bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(signature_b64)
    else {
        return false;
    };
    let Ok(sig) = Signature::from_slice(&sig_bytes) else {
        return false;
    };
    verifying
        .verify_strict(
            &request_sign_message(worker_pk, coordinator_pk, label, secret_proof),
            &sig,
        )
        .is_ok()
}

pub fn verify_enrollment_response(
    code: &EnrollmentCode,
    resp: &EnrollCompleteResponse,
) -> Result<(), EnrollError> {
    if resp.worker_id != code.worker_id {
        return Err(EnrollError::ResponseMismatch(format!(
            "worker_id expected {}, got {}",
            code.worker_id, resp.worker_id
        )));
    }
    if resp.worker_pubkey != code.worker_pubkey {
        return Err(EnrollError::ResponseMismatch(
            "worker_pubkey does not match code".into(),
        ));
    }
    if resp.audit_pubkey != code.audit_pubkey {
        return Err(EnrollError::ResponseMismatch(
            "audit_pubkey does not match code".into(),
        ));
    }
    if resp.fabric_port != code.fabric_port {
        return Err(EnrollError::ResponseMismatch(format!(
            "fabric_port expected {}, got {}",
            code.fabric_port, resp.fabric_port
        )));
    }
    if resp.host != code.host {
        return Err(EnrollError::ResponseMismatch(format!(
            "host expected {}, got {}",
            code.host, resp.host
        )));
    }
    resp.worker_pubkey
        .to_verifying()
        .map_err(|e| EnrollError::ResponseMismatch(format!("worker_pubkey invalid: {e}")))?;
    resp.audit_pubkey
        .to_verifying()
        .map_err(|e| EnrollError::ResponseMismatch(format!("audit_pubkey invalid: {e}")))?;
    if let Some(code_cert) = &code.enroll_tls_cert_b64 {
        match &resp.fabric_tls_cert_b64 {
            Some(resp_cert) if resp_cert == code_cert => {}
            _ => {
                return Err(EnrollError::ResponseMismatch(
                    "fabric TLS cert does not match code".into(),
                ));
            }
        }
    }
    Ok(())
}

pub fn parse_user_code(input: &str) -> Result<EnrollmentCode, EnrollError> {
    let s = input.trim();
    let payload = if let Some(rest) = s.strip_prefix(ENROLL_CODE_PREFIX) {
        format!("{ENROLL_CODE_PREFIX}{rest}")
    } else {
        format!("{ENROLL_CODE_PREFIX}{s}")
    };
    Ok(decode_enrollment_code(&payload)?)
}

fn enrollment_scheme(code: &EnrollmentCode) -> &'static str {
    if code.enroll_tls_cert_b64.is_some() {
        "https"
    } else {
        "http"
    }
}

/// Coordinator-side: connect to worker enrollment listener and complete handshake.
pub async fn complete_enrollment(
    guard: &EgressGuard,
    code_input: &str,
    label: &str,
    coordinator: &KeyPair,
) -> Result<(EnrollmentCode, EnrollCompleteResponse, IpAddr), EnrollError> {
    let code = parse_user_code(code_input)?;
    let ip = resolve::resolve_host_ip(&code.host)
        .await
        .map_err(|e: ResolveError| EnrollError::Resolve(e.to_string()))?;
    let temp_label = format!("_enroll:{}", code.worker_id);
    guard.allow_node(&temp_label, ip, Some(code.enroll_port));

    let secret = code.secret_bytes()?;
    let proof = compute_secret_proof(&secret, &code.worker_pubkey.0, &coordinator.public().0);
    let signature = sign_enrollment_request(coordinator, &code.worker_pubkey.0, &proof, label);
    let body = EnrollCompleteRequest {
        secret_proof: proof,
        coordinator_pubkey: coordinator.public(),
        coordinator_signature_b64: signature,
        label: label.to_string(),
    };
    let scheme = enrollment_scheme(&code);
    let url = format!(
        "{scheme}://{}:{}/v1/enroll/complete",
        code.host, code.enroll_port
    );

    let result = async {
        let v = if let Some(cert_b64) = &code.enroll_tls_cert_b64 {
            let cert =
                base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, cert_b64)
                    .map_err(|e| EnrollError::Http(format!("bad enroll tls cert: {e}")))?;
            guard
                .post_json_pinned(
                    &url,
                    &serde_json::to_value(&body).unwrap(),
                    "enroll:complete",
                    &cert,
                )
                .await
                .map_err(|e| EnrollError::Http(e.to_string()))?
        } else {
            guard
                .post_json(
                    &url,
                    &serde_json::to_value(&body).unwrap(),
                    "enroll:complete",
                )
                .await
                .map_err(|e| EnrollError::Http(e.to_string()))?
        };
        let resp: EnrollCompleteResponse = serde_json::from_value(v)
            .map_err(|e| EnrollError::Http(format!("bad response: {e}")))?;
        if !resp.ok {
            return Err(EnrollError::Rejected(
                resp.error.unwrap_or_else(|| "unknown".into()),
            ));
        }
        verify_enrollment_response(&code, &resp)?;
        Ok(resp)
    }
    .await;

    guard.remove_allow_label(&temp_label);

    let resp = result?;
    Ok((code, resp, ip))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::{EnrollmentCode, DEFAULT_ENROLL_TTL, DEFAULT_FABRIC_PORT};
    use crate::server::{run_enrollment_server, EnrollmentServerConfig, EnrollmentServerOutcome};
    use lokai_egress::EgressGuard;
    use lokai_node::issue_self_signed_cert;
    use std::net::IpAddr;
    use std::time::Duration;

    #[test]
    fn request_signature_frames_variable_fields() {
        let coordinator = KeyPair::generate();
        let worker = KeyPair::generate().public().0;
        let sig = sign_enrollment_request(&coordinator, &worker, "A1B2", "node");
        assert!(!verify_enrollment_request_signature(
            &coordinator.public().0,
            &worker,
            "nodeA",
            "1B2",
            &sig,
        ));
    }

    #[test]
    fn proof_verifies() {
        let secret = b"test-secret-32-bytes-long!!!!!!";
        let w = vec![1u8; 32];
        let c = vec![2u8; 32];
        let p = compute_secret_proof(secret, &w, &c);
        assert!(verify_secret_proof(secret, &w, &c, &p));
        assert!(!verify_secret_proof(secret, &w, &c, "bad"));
    }

    #[test]
    fn request_signature_verifies() {
        let coord = KeyPair::generate();
        let w = KeyPair::generate().public().0;
        let proof = "proof";
        let sig = sign_enrollment_request(&coord, &w, proof, "gpu");
        assert!(verify_enrollment_request_signature(
            &coord.public().0,
            &w,
            "gpu",
            proof,
            &sig
        ));
        assert!(!verify_enrollment_request_signature(
            &coord.public().0,
            &w,
            "gpu",
            "wrong",
            &sig
        ));
    }

    #[test]
    fn response_mismatch_is_rejected() {
        let w = KeyPair::generate();
        let a = KeyPair::generate();
        let code = EnrollmentCode::new(&w, &a, "127.0.0.1", 9470, 9471, DEFAULT_ENROLL_TTL, None);
        let mut resp = EnrollCompleteResponse {
            ok: true,
            error: None,
            worker_id: code.worker_id.clone(),
            worker_pubkey: code.worker_pubkey.clone(),
            audit_pubkey: code.audit_pubkey.clone(),
            fabric_port: code.fabric_port,
            host: code.host.clone(),
            fabric_tls_cert_b64: None,
        };
        assert!(verify_enrollment_response(&code, &resp).is_ok());
        resp.worker_pubkey = KeyPair::generate().public();
        assert!(matches!(
            verify_enrollment_response(&code, &resp),
            Err(EnrollError::ResponseMismatch(_))
        ));
    }

    #[tokio::test]
    async fn complete_enrollment_e2e_through_egress() {
        let worker = KeyPair::generate();
        let audit = KeyPair::generate();
        let coordinator = KeyPair::generate();
        let (cert, key) = issue_self_signed_cert("lokai-worker").unwrap();
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
            Some(&cert),
        );
        let code_str = code.display_string().unwrap();

        let cfg = EnrollmentServerConfig {
            code: code.clone(),
            ttl: Duration::from_secs(5),
            bind_host: "127.0.0.1".into(),
            fabric_tls_cert: Some(cert),
            fabric_tls_key: Some(key),
        };
        let handle = tokio::spawn(async move { run_enrollment_server(cfg).await });

        tokio::time::sleep(Duration::from_millis(150)).await;

        let guard = EgressGuard::new();
        let (back_code, resp, ip) = complete_enrollment(&guard, &code_str, "gpu-box", &coordinator)
            .await
            .unwrap();
        assert_eq!(back_code.worker_id, code.worker_id);
        assert_eq!(resp.worker_pubkey, code.worker_pubkey);
        assert_eq!(ip, IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));

        match handle.await.unwrap() {
            EnrollmentServerOutcome::Completed { label, .. } => assert_eq!(label, "gpu-box"),
            other => panic!("unexpected {other:?}"),
        }
    }
}
