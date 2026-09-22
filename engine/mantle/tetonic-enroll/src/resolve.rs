//! Async host resolution for enrollment (never block Tokio workers on DNS).

use std::net::{IpAddr, Ipv4Addr};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("could not resolve host '{0}'")]
    Host(String),
}

/// Resolve a hostname or literal to an IP for egress allow rules.
pub async fn resolve_host_ip(host: &str) -> Result<IpAddr, ResolveError> {
    if host.eq_ignore_ascii_case("localhost") {
        return Ok(IpAddr::V4(Ipv4Addr::LOCALHOST));
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(ip);
    }
    let target = format!("{host}:0");
    let mut addrs = tokio::net::lookup_host(&target)
        .await
        .map_err(|_| ResolveError::Host(host.to_string()))?;
    addrs
        .next()
        .map(|sa| sa.ip())
        .ok_or_else(|| ResolveError::Host(host.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn literal_ip_skips_dns() {
        let ip = resolve_host_ip("127.0.0.1").await.unwrap();
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::LOCALHOST));
    }

    #[tokio::test]
    async fn localhost_resolves() {
        let ip = resolve_host_ip("localhost").await.unwrap();
        assert!(ip.is_loopback());
    }
}
