//! Fabric ingress resource limits (ingress-guard-v1 § N4.3).

/// Max JSON body size for POST routes (`/v1/chat`, `/v1/revoke`, …).
pub const MAX_FABRIC_BODY_BYTES: usize = 8 * 1024 * 1024;

/// Max concurrent in-flight fabric connections per worker listener.
pub const MAX_FABRIC_CONNECTIONS: usize = 64;
/// One source cannot occupy the whole listener. Hosts behind a NAT share this cap.
pub const MAX_FABRIC_CONNECTIONS_PER_IP: usize = 8;

/// Absolute ingress phase deadlines; these never bound model execution time.
pub const TLS_HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
pub const HEADER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
pub const BODY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Max rows retained in worker `ingress_log` (oldest trimmed).
pub const INGRESS_LOG_RETAIN: i64 = 10_000;
