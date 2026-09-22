//! Best-effort coordinator → worker revocation push (N0.3).

use std::net::IpAddr;

use lokai_egress::EgressGuard;
use lokai_enroll::{KeyPair, PublicKeyBytes};
use lokai_fabric_client::{fabric_request, FabricClientError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RevokeError {
    #[error("egress: {0}")]
    Egress(#[from] lokai_egress::EgressError),
    #[error("fabric: {0}")]
    Fabric(#[from] FabricClientError),
    #[error("missing worker fabric TLS certificate — re-enroll to enable push revoke")]
    NoServerCert,
    #[error("revoke rejected: {0}")]
    Rejected(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevokeRequest {
    pub coordinator_pubkey: PublicKeyBytes,
    pub epoch: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevokeResponse {
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    pub epoch: u64,
}

/// Push un-enroll to a worker over mTLS (best-effort; local DB revoke is authoritative).
pub async fn push_revoke(
    guard: &EgressGuard,
    ip: IpAddr,
    port: u16,
    server_cert: &[u8],
    coordinator: &KeyPair,
    epoch: u64,
    worker_id: Option<&str>,
) -> Result<RevokeResponse, RevokeError> {
    if server_cert.is_empty() {
        return Err(RevokeError::NoServerCert);
    }

    let body = RevokeRequest {
        coordinator_pubkey: coordinator.public(),
        epoch,
        worker_id: worker_id.map(String::from),
    };
    let json = serde_json::to_string(&body).unwrap_or_else(|_| "{}".into());
    let resp = fabric_request(
        guard,
        ip,
        port,
        server_cert,
        coordinator,
        "POST",
        "/v1/revoke",
        Some(&json),
        "estate:revoke",
    )
    .await?;

    let parsed: RevokeResponse = serde_json::from_str(&resp.body).unwrap_or(RevokeResponse {
        ok: resp.status == 200,
        error: None,
        epoch,
    });
    if !parsed.ok {
        return Err(RevokeError::Rejected(parsed.error.unwrap_or_else(|| {
            format!("worker rejected revoke (HTTP {})", resp.status)
        })));
    }
    Ok(parsed)
}
