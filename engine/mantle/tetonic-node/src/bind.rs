//! Listen-address policy (network-safety: loopback-first).
//!
//! Enrollment plain-HTTP guard lives in `tetonic_enroll::plaintext_enrollment_permitted`.

/// Default fabric/enrollment bind host. Override with `LOKAI_BIND_HOST`.
pub const DEFAULT_BIND_HOST: &str = "127.0.0.1";

/// Resolve the host to bind enrollment/fabric listeners.
///
/// - Default: `127.0.0.1` (safest for dev and same-machine homelab).
/// - `LOKAI_BIND_HOST`: explicit host (e.g. LAN IP or tailnet address).
/// - `LOKAI_BIND_ALL=1`: bind `0.0.0.0` (requires deliberate opt-in).
pub fn resolve_listen_host() -> (String, Option<String>) {
    if std::env::var("LOKAI_BIND_ALL").is_ok() {
        return (
            "0.0.0.0".into(),
            Some(
                "LOKAI_BIND_ALL is set — enrollment/fabric listening on all interfaces. \
                 Ensure your firewall restricts inbound access."
                    .into(),
            ),
        );
    }
    let host = std::env::var("LOKAI_BIND_HOST").unwrap_or_else(|_| DEFAULT_BIND_HOST.into());
    (host, None)
}
