//! Enrollment listener bind policy (AR1-4 / V9).
//!
//! Plain HTTP enrollment must not listen on LAN interfaces without explicit opt-in.

/// True when `host` resolves to a loopback address (or common loopback names).
pub fn is_loopback_bind(host: &str) -> bool {
    match host.trim().to_ascii_lowercase().as_str() {
        "127.0.0.1" | "localhost" | "::1" => true,
        other => other
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false),
    }
}

/// Plain HTTP enrollment is permitted only on loopback unless the operator opts in.
pub fn plaintext_enrollment_permitted(bind_host: &str) -> Result<(), String> {
    if is_loopback_bind(bind_host) {
        return Ok(());
    }
    if std::env::var("LOKAI_ALLOW_LAN_ENROLL")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        return Ok(());
    }
    Err(format!(
        "enrollment refuses plain HTTP on non-loopback bind `{bind_host}` — \
         use 127.0.0.1 (default), set LOKAI_BIND_HOST to a loopback address, \
         or set LOKAI_ALLOW_LAN_ENROLL=1 only on a trusted LAN with firewall rules"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_hosts_are_recognized() {
        assert!(is_loopback_bind("127.0.0.1"));
        assert!(is_loopback_bind("localhost"));
        assert!(is_loopback_bind("::1"));
    }

    #[test]
    fn lan_hosts_are_not_loopback() {
        assert!(!is_loopback_bind("0.0.0.0"));
        assert!(!is_loopback_bind("192.168.1.10"));
    }

    #[test]
    fn plaintext_requires_loopback_by_default() {
        assert!(plaintext_enrollment_permitted("127.0.0.1").is_ok());
        assert!(plaintext_enrollment_permitted("192.168.1.10").is_err());
    }
}
