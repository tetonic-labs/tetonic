//! Push owner-activity to workers (coordinator → fabric).

use std::net::IpAddr;
use std::sync::Arc;

use lokai_enroll::KeyPair;
use serde::{Deserialize, Serialize};
use tetonic_egress::EgressGuard;

use crate::client::{fabric_request, FabricClientError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerActivityRequest {
    #[serde(default = "default_ttl")]
    pub ttl_sec: u64,
}

fn default_ttl() -> u64 {
    30
}

/// Tell a worker the owner is active (preempt circle jobs for `ttl_sec`).
pub async fn push_owner_activity(
    guard: &EgressGuard,
    ip: IpAddr,
    port: u16,
    server_cert: &[u8],
    coordinator: &KeyPair,
    ttl_sec: u64,
) -> Result<(), FabricClientError> {
    if server_cert.is_empty() {
        return Ok(());
    }
    let body = OwnerActivityRequest { ttl_sec };
    let json = serde_json::to_string(&body).unwrap_or_else(|_| r#"{"ttl_sec":30}"#.into());
    let resp = fabric_request(
        guard,
        ip,
        port,
        server_cert,
        coordinator,
        "POST",
        "/v1/owner/activity",
        Some(&json),
        "fabric:owner_activity",
    )
    .await?;
    if resp.status != 200 {
        tracing::debug!(
            "owner activity push to {ip}:{port} returned HTTP {}",
            resp.status
        );
    }
    Ok(())
}

/// Best-effort owner activity to all enrolled workers with fabric certs.
pub async fn push_owner_activity_all(
    guard: &EgressGuard,
    coordinator: &KeyPair,
    workers: &[(IpAddr, u16, Arc<[u8]>)],
    ttl_sec: u64,
) {
    for (ip, port, cert) in workers {
        if let Err(e) = push_owner_activity(guard, *ip, *port, cert, coordinator, ttl_sec).await {
            tracing::debug!("owner activity push to {ip}:{port}: {e}");
        }
    }
}
