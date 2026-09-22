//! Egress Guard — the privacy chokepoint.
//!
//! Enforces Charter Rights 1 (sanctuary) and the trust boundary: every outbound
//! request is **default-deny**. Loopback (which never leaves the machine) is
//! always allowed; anything else is denied until an explicit allow rule for a
//! user-enrolled node is added. Hosted inference uses separate, exact HTTPS
//! endpoint grants; those grants never widen the ordinary node allowlist.
//!
//! This crate is intentionally the *only* place in the engine that owns an HTTP
//! client, so the auditable "could this leak?" surface is one file.
//!
//! Hardening: authorization resolves the destination, checks the resolved IP
//! against the policy, and then the request is **pinned to that exact IP** — the
//! URL host is rewritten to the authorized address and the original name is sent
//! in the `Host` header — so the socket can never connect somewhere other than
//! what was authorized (closes the resolve→connect TOCTOU / DNS-rebinding gap).
//! Redirect-following is disabled, so a 3xx can't bounce a request to an
//! unauthorized host behind the guard's back. (A future custom hyper connector
//! could push the IP pin all the way down the stack; this is equivalent for the
//! plain-HTTP local runtimes we talk to.)

mod hosted;
mod ndjson;
mod pinned_tls;
pub use hosted::{BearerCredential, HostedCredential};

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Allow,
    Deny,
}

/// A single audited outbound decision. Streams to the (future) Network Activity
/// Panel and is the evidence the privacy gate checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EgressEvent {
    pub ts: String,
    pub initiator: String,
    pub host: String,
    pub resolved_ip: Option<String>,
    pub port: u16,
    pub decision: Action,
    pub reason: String,
}

/// An explicit allow rule — a user-enrolled, owned node. Loopback needs no rule
/// for enforcement (implicit inference port) but production worker clients pin
/// the configured bind into this list so the assembled guard is non-empty.
#[derive(Debug, Clone)]
pub struct AllowRule {
    pub label: String,
    pub ip: IpAddr,
    /// `None` = any port on that host.
    pub port: Option<u16>,
    /// Monotonic generation assigned by [`EgressGuard::allow_node`].
    pub generation: u64,
}

#[derive(Debug, Error)]
pub enum EgressError {
    #[error("egress denied to {host}:{port} — {reason}")]
    Denied {
        host: String,
        port: u16,
        reason: String,
    },
    #[error("could not resolve host '{0}'")]
    Resolve(String),
    #[error("invalid url '{0}'")]
    Url(String),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("stream decode: {0}")]
    StreamDecode(String),
}

/// Maximum egress decisions retained in memory for the activity panel.
pub const MAX_ACTIVITY_LOG: usize = 1_000;

/// Default-deny network guard. Construct with [`EgressGuard::new`]; widen the
/// node boundary via [`EgressGuard::allow_node`] (enrollment). Hosted POSTs
/// separately require [`EgressGuard::allow_hosted_endpoint`].
pub struct EgressGuard {
    hosted_endpoints: Mutex<Vec<String>>,
    allow: Mutex<Vec<AllowRule>>,
    log: Mutex<VecDeque<EgressEvent>>,
    next_generation: Mutex<u64>,
    /// Loopback port permitted for local inference (SEC2-E2-024). Default 11434.
    loopback_inference_port: Mutex<u16>,
    /// Shared client for IP-literal targets (no DNS to subvert).
    client: reqwest::Client,
    /// Resolution-pinned clients, cached by `host|sorted-addrs` so we keep
    /// connection pooling/keep-alive across requests instead of rebuilding a
    /// client (and its connector) every call. A changed authorized address set
    /// yields a new key ⇒ a fresh pinned client.
    clients: Mutex<HashMap<String, reqwest::Client>>,
    allow_private_hosted: Mutex<bool>,
}

impl Default for EgressGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl EgressGuard {
    pub fn new() -> Self {
        let allow_private = std::env::var("LOKAI_ALLOW_PRIVATE_EGRESS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        Self {
            hosted_endpoints: Mutex::new(Vec::new()),
            allow: Mutex::new(Vec::new()),
            log: Mutex::new(VecDeque::new()),
            next_generation: Mutex::new(1),
            loopback_inference_port: Mutex::new(11434),
            // Redirects are NOT followed: the guard authorizes one URL, and a 3xx
            // to another host must not silently bypass it. The caller sees the 3xx.
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(std::time::Duration::from_millis(500))
                .build()
                .expect("building egress http client"),
            clients: Mutex::new(HashMap::new()),
            allow_private_hosted: Mutex::new(allow_private),
        }
    }

    /// Configure whether enrolled hosted endpoints are permitted to resolve to
    /// private network addresses (e.g. enterprise VPC endpoints). Default is false.
    pub fn allow_private_hosted_endpoints(&self, allow: bool) {
        if let Ok(mut lock) = self.allow_private_hosted.lock() {
            *lock = allow;
        }
    }

    /// Check whether private network addresses are permitted for enrolled hosted endpoints.
    pub fn is_private_hosted_allowed(&self) -> bool {
        self.allow_private_hosted
            .lock()
            .map(|l| *l)
            .unwrap_or(false)
    }

    /// Enroll an owned node, widening the ordinary node allowlist. Hosted endpoint
    /// grants use a separate API. Returns the generation stamped on the rule.
    pub fn allow_node(&self, label: impl Into<String>, ip: IpAddr, port: Option<u16>) -> u64 {
        let label = label.into();
        let generation = {
            let mut next = self
                .next_generation
                .lock()
                .expect("egress generation poisoned");
            let g = *next;
            *next = next.saturating_add(1);
            g
        };
        self.allow
            .lock()
            .expect("egress allow poisoned")
            .push(AllowRule {
                label,
                ip,
                port,
                generation,
            });
        generation
    }

    /// Remove allow rules by label (e.g. ephemeral enrollment target).
    pub fn remove_allow_label(&self, label: &str) {
        self.allow
            .lock()
            .expect("egress allow poisoned")
            .retain(|r| r.label != label);
    }

    /// Drop every allow rule stamped with `generation` (in-process retract).
    pub fn retract_generation(&self, generation: u64) {
        self.allow
            .lock()
            .expect("egress allow poisoned")
            .retain(|r| r.generation != generation);
    }

    /// Pin loopback HTTP to the configured inference port (`LOKAI_OLLAMA`, SEC2-E2-024).
    pub fn configure_loopback_inference(&self, port: u16) {
        *self
            .loopback_inference_port
            .lock()
            .expect("egress loopback port poisoned") = port;
    }

    /// Worker Infer constructor: explicit `127.0.0.1:port` pin + loopback port.
    pub fn loopback_inference(port: u16) -> Self {
        let g = Self::new();
        g.configure_loopback_inference(port);
        g.allow_node(
            "loopback-inference",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            Some(port),
        );
        g
    }

    /// Pin the configured inference URL (loopback helper or `allow_node` of that IP).
    pub fn pinned_to_inference_url(url: &str) -> Self {
        let (host, port) = parse_inference_url(url);
        if let Ok(ip) = host.parse::<IpAddr>() {
            if ip.is_loopback() {
                return Self::loopback_inference(port);
            }
            let g = Self::new();
            g.allow_node("inference", ip, Some(port));
            return g;
        }
        if is_loopback_hostname(&host) {
            return Self::loopback_inference(port);
        }
        let g = Self::new();
        if let Ok(addrs) = (host.as_str(), port).to_socket_addrs() {
            for sa in addrs {
                g.allow_node("inference", sa.ip(), Some(sa.port()));
            }
        }
        g
    }

    /// Snapshot enrolled allow rules (loopback is implicit, not listed).
    pub fn allow_rules(&self) -> Vec<AllowRule> {
        self.allow.lock().expect("egress allow poisoned").clone()
    }

    /// Snapshot of recent outbound decisions (ring buffer, newest last).
    pub fn activity_log(&self) -> Vec<EgressEvent> {
        self.log
            .lock()
            .expect("egress log poisoned")
            .iter()
            .cloned()
            .collect()
    }

    fn record(&self, ev: EgressEvent) {
        tracing::info!(
            target: "egress",
            initiator = %ev.initiator,
            host = %ev.host,
            port = ev.port,
            decision = ?ev.decision,
            reason = %ev.reason,
            "egress decision"
        );
        let mut log = self.log.lock().expect("egress log poisoned");
        if log.len() >= MAX_ACTIVITY_LOG {
            log.pop_front();
        }
        log.push_back(ev);
    }

    /// Resolve `host:port` without blocking async worker threads.
    async fn resolve_host(host: &str, port: u16) -> Result<Vec<SocketAddr>, EgressError> {
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(vec![SocketAddr::new(ip, port)]);
        }
        let target = format!("{host}:{port}");
        let mut addrs = Vec::new();
        let lookup = tokio::net::lookup_host(&target)
            .await
            .map_err(|_| EgressError::Resolve(host.to_string()))?;
        for sa in lookup {
            addrs.push(sa);
        }
        if addrs.is_empty() {
            Err(EgressError::Resolve(host.to_string()))
        } else {
            Ok(addrs)
        }
    }

    /// Resolve the host, check every resolved address against the policy, and
    /// return the subset that is **authorized**. Default-deny: if nothing matches,
    /// the request is denied. The returned addresses are what the request is then
    /// pinned to (see [`EgressGuard::client_for`]), so the socket can only reach
    /// what was vetted here — closing the resolve→connect TOCTOU window.
    pub async fn ensure_allowed(
        &self,
        host: &str,
        port: u16,
        initiator: &str,
    ) -> Result<Vec<SocketAddr>, EgressError> {
        self.authorize(host, port, initiator).await
    }

    async fn authorize(
        &self,
        host: &str,
        port: u16,
        initiator: &str,
    ) -> Result<Vec<SocketAddr>, EgressError> {
        let resolved = Self::resolve_host(host, port).await?;

        let mut authorized: Vec<SocketAddr> = Vec::new();
        let mut reason: Option<String> = None;
        let inference_port = *self
            .loopback_inference_port
            .lock()
            .expect("egress loopback port poisoned");
        for sa in &resolved {
            let ip = sa.ip();
            // Loopback default: inference port only (SEC2-E2-024). Explicit allow
            // rules (e.g. ephemeral enrollment) may widen other loopback ports.
            if ip.is_loopback() {
                if sa.port() == inference_port {
                    authorized.push(*sa);
                    reason.get_or_insert_with(|| {
                        format!("loopback inference (port {inference_port})")
                    });
                } else {
                    for rule in self.allow.lock().expect("egress allow poisoned").iter() {
                        if rule.ip == ip && rule.port.is_none_or(|p| p == port) {
                            authorized.push(*sa);
                            reason.get_or_insert_with(|| format!("enrolled node: {}", rule.label));
                            break;
                        }
                    }
                }
                continue;
            }
            for rule in self.allow.lock().expect("egress allow poisoned").iter() {
                if rule.ip == ip && rule.port.is_none_or(|p| p == port) {
                    authorized.push(*sa);
                    reason.get_or_insert_with(|| format!("enrolled node: {}", rule.label));
                    break;
                }
            }
        }

        if let Some(reason) = reason {
            self.record(EgressEvent {
                ts: now(),
                initiator: initiator.to_string(),
                host: host.to_string(),
                resolved_ip: authorized.first().map(|sa| sa.ip().to_string()),
                port,
                decision: Action::Allow,
                reason,
            });
            return Ok(authorized);
        }

        self.record(EgressEvent {
            ts: now(),
            initiator: initiator.to_string(),
            host: host.to_string(),
            resolved_ip: resolved.first().map(|sa| sa.ip().to_string()),
            port,
            decision: Action::Deny,
            reason: "no matching allow rule (default deny)".to_string(),
        });
        Err(EgressError::Denied {
            host: host.to_string(),
            port,
            reason: "default-deny: host is outside the trust boundary".to_string(),
        })
    }

    /// Authorize `url` (default-deny) and return an HTTP client whose DNS
    /// resolution for this host is **pinned to the authorized addresses**. The URL
    /// keeps its hostname (so `Host`/SNI and reqwest's address fallback are
    /// preserved), but reqwest will only ever connect to addresses that passed
    /// policy — no second, unvetted DNS lookup at connect time.
    async fn pin(
        &self,
        url: &str,
        initiator: &str,
    ) -> Result<(String, reqwest::Client), EgressError> {
        let parsed = url::Url::parse(url).map_err(|_| EgressError::Url(url.to_string()))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| EgressError::Url(url.to_string()))?
            .to_string();
        let port = parsed.port_or_known_default().unwrap_or(80);
        let addrs = self.authorize(&host, port, initiator).await?;
        let addrs = filter_inference_pin_addrs(&addrs, initiator);
        Ok((url.to_string(), self.client_for(&host, &addrs)?))
    }

    /// Build the client used for an authorized request. For a hostname target we
    /// install a resolution override pinning it to exactly `addrs`, so the system
    /// resolver is bypassed at connect time (no rebinding window). For an IP-literal
    /// target there is no DNS to subvert, so the shared client is reused.
    fn client_for(&self, host: &str, addrs: &[SocketAddr]) -> Result<reqwest::Client, EgressError> {
        if host.parse::<IpAddr>().is_ok() {
            return Ok(self.client.clone());
        }
        // Cache key folds in the authorized address set so a re-resolution that
        // changes the addresses builds a fresh pinned client (never reuses a pool
        // pointing at addresses that no longer pass policy).
        let mut sorted: Vec<String> = addrs.iter().map(|a| a.to_string()).collect();
        sorted.sort();
        let key = format!("{host}|{}", sorted.join(","));
        {
            let cache = self.clients.lock().expect("egress client cache poisoned");
            if let Some(c) = cache.get(&key) {
                return Ok(c.clone());
            }
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_millis(500))
            .resolve_to_addrs(host, addrs)
            .build()
            .map_err(EgressError::Http)?;
        self.clients
            .lock()
            .expect("egress client cache poisoned")
            .insert(key, client.clone());
        Ok(client)
    }

    /// Active Canary Control Plane: verify that a pinned address is actively reachable
    /// on the network (within 250ms) before committing a connection pool or sending a prompt.
    pub async fn canary_check_target(
        &self,
        host: &str,
        port: u16,
        initiator: &str,
    ) -> Result<bool, EgressError> {
        let addrs = self.authorize(host, port, initiator).await?;
        let addrs = filter_inference_pin_addrs(&addrs, initiator);
        let Some(sa) = addrs.first() else {
            return Ok(false);
        };
        match tokio::time::timeout(
            std::time::Duration::from_millis(250),
            tokio::net::TcpStream::connect(sa),
        )
        .await
        {
            Ok(Ok(_)) => Ok(true),
            _ => {
                tracing::warn!(
                    host,
                    port,
                    initiator,
                    "egress canary check failed — target is unreachable"
                );
                Ok(false)
            }
        }
    }

    /// POST with a pinned server certificate (enrollment HTTPS). Authorizes first.
    pub async fn post_json_pinned(
        &self,
        url: &str,
        body: &serde_json::Value,
        initiator: &str,
        pinned_server_cert_der: &[u8],
    ) -> Result<serde_json::Value, EgressError> {
        use rustls::pki_types::ServerName;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio_rustls::TlsConnector;

        const ENROLL_SERVER_NAME: &str = "lokai-worker";

        let parsed = url::Url::parse(url).map_err(|_| EgressError::Url(url.to_string()))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| EgressError::Url(url.to_string()))?
            .to_string();
        let port = parsed.port_or_known_default().unwrap_or(443);
        let addrs = self.authorize(&host, port, initiator).await?;
        let addrs = filter_inference_pin_addrs(&addrs, initiator);
        let sa = addrs.first().copied().ok_or_else(|| EgressError::Denied {
            host: host.clone(),
            port,
            reason: "no authorized address".into(),
        })?;

        let tls = pinned_tls::client_config_pinned_server(pinned_server_cert_der)
            .map_err(|e| EgressError::Url(format!("tls: {e}")))?;
        let connector = TlsConnector::from(tls);
        let stream = tokio::net::TcpStream::connect(sa)
            .await
            .map_err(|e| EgressError::StreamDecode(format!("connect: {e}")))?;
        let server_name = ServerName::try_from(ENROLL_SERVER_NAME)
            .map_err(|e| EgressError::Url(format!("tls server name: {e}")))?;
        let mut tls = connector
            .connect(server_name, stream)
            .await
            .map_err(|e| EgressError::StreamDecode(format!("tls: {e}")))?;

        let body_str =
            serde_json::to_string(body).map_err(|e| EgressError::StreamDecode(e.to_string()))?;
        let path = parsed.path();
        let req = format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
            body_str.len()
        );
        tls.write_all(req.as_bytes())
            .await
            .map_err(|e| EgressError::StreamDecode(format!("write: {e}")))?;
        let mut raw = Vec::new();
        tls.read_to_end(&mut raw)
            .await
            .map_err(|e| EgressError::StreamDecode(format!("read: {e}")))?;
        let resp =
            std::str::from_utf8(&raw).map_err(|e| EgressError::StreamDecode(e.to_string()))?;
        let body = resp
            .split_once("\r\n\r\n")
            .map(|(_, b)| b.trim())
            .ok_or_else(|| EgressError::StreamDecode("missing http body".into()))?;
        serde_json::from_str(body).map_err(|e| EgressError::StreamDecode(e.to_string()))
    }

    /// The only sanctioned outbound POST. Authorizes (default-deny) and pins DNS.
    pub async fn post_json<T: Serialize + ?Sized>(
        &self,
        url: &str,
        body: &T,
        initiator: &str,
    ) -> Result<serde_json::Value, EgressError> {
        let (url, client) = self.pin(url, initiator).await?;
        let resp = client.post(url).json(body).send().await?;
        Ok(resp.json::<serde_json::Value>().await?)
    }

    /// The only sanctioned streaming POST. Authorizes (default-deny) first, then
    /// yields newline-delimited JSON values (NDJSON) as they arrive — the shape
    /// local runtimes (Ollama) use for token streaming. Still the sole socket owner.
    pub async fn post_ndjson_stream<T: Serialize + ?Sized>(
        &self,
        url: &str,
        body: &T,
        initiator: &str,
    ) -> Result<impl futures_util::Stream<Item = Result<serde_json::Value, EgressError>>, EgressError>
    {
        let (url, client) = self.pin(url, initiator).await?;
        let resp = client.post(url).json(body).send().await?;
        let mut bytes = resp.bytes_stream();

        let stream = async_stream::try_stream! {
            use futures_util::StreamExt;
            let mut decoder = ndjson::Decoder::default();
            while let Some(chunk) = bytes.next().await {
                let chunk = chunk?;
                decoder.push(&chunk);
                while let Some(record) = decoder.next() {
                    yield record?;
                }
            }
            if let Some(record) = decoder.finish() {
                yield record?;
            }
        };
        Ok(stream)
    }

    /// The only sanctioned outbound GET. Authorizes (default-deny) first.
    pub async fn get_json(
        &self,
        url: &str,
        initiator: &str,
    ) -> Result<serde_json::Value, EgressError> {
        let (url, client) = self.pin(url, initiator).await?;
        let resp = client.get(url).send().await?;
        Ok(resp.json::<serde_json::Value>().await?)
    }

    /// The only sanctioned outbound DELETE. Authorizes (default-deny) first.
    pub async fn delete_json<T: Serialize + ?Sized>(
        &self,
        url: &str,
        body: &T,
        initiator: &str,
    ) -> Result<serde_json::Value, EgressError> {
        let (url, client) = self.pin(url, initiator).await?;
        let resp = client.delete(url).json(body).send().await?;
        Ok(resp.json::<serde_json::Value>().await?)
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// When DNS returns loopback + enrolled LAN, inference must pin loopback only (SEC2-E2-025).
fn filter_inference_pin_addrs(addrs: &[SocketAddr], initiator: &str) -> Vec<SocketAddr> {
    if !initiator.starts_with("inference:") {
        return addrs.to_vec();
    }
    let loopback: Vec<SocketAddr> = addrs
        .iter()
        .filter(|sa| sa.ip().is_loopback())
        .copied()
        .collect();
    if !loopback.is_empty() {
        loopback
    } else {
        addrs.to_vec()
    }
}

fn parse_inference_url(url: &str) -> (String, u16) {
    if let Ok(parsed) = url::Url::parse(url) {
        let host = parsed
            .host_str()
            .unwrap_or("127.0.0.1")
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        let port = parsed.port_or_known_default().unwrap_or(11434);
        return (host, port);
    }
    ("127.0.0.1".into(), 11434)
}

fn is_loopback_hostname(host: &str) -> bool {
    host.trim_end_matches('.').eq_ignore_ascii_case("localhost")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn loopback_is_allowed() {
        let g = EgressGuard::new();
        let addrs = g.authorize("127.0.0.1", 11434, "test").await.unwrap();
        assert!(addrs.iter().all(|sa| sa.ip().is_loopback()));
        assert!(!addrs.is_empty());
    }

    #[tokio::test]
    async fn localhost_authorizes_all_loopback_addrs() {
        // localhost commonly resolves to both ::1 and 127.0.0.1. The guard must
        // return *every* loopback address so reqwest can fall back across families
        // (e.g. server only on IPv4) — pinning to a single addr would break that.
        let g = EgressGuard::new();
        let addrs = g.authorize("localhost", 11434, "test").await.unwrap();
        assert!(!addrs.is_empty());
        assert!(addrs.iter().all(|sa| sa.ip().is_loopback()));
        assert!(addrs.iter().all(|sa| sa.port() == 11434));
    }

    #[tokio::test]
    async fn public_host_is_denied_by_default() {
        let g = EgressGuard::new();
        // 8.8.8.8 is a literal public IP — must be denied with no allow rule.
        let r = g.authorize("8.8.8.8", 443, "test").await;
        assert!(matches!(r, Err(EgressError::Denied { .. })));
        // and the denial must be logged (verifiability).
        let log = g.activity_log();
        assert!(log.iter().any(|e| e.decision == Action::Deny));
    }

    #[test]
    fn inference_pins_loopback_only_when_mixed() {
        let addrs = vec![
            SocketAddr::from(([127, 0, 0, 1], 11434)),
            SocketAddr::from(([10, 0, 0, 5], 11434)),
        ];
        let filtered = super::filter_inference_pin_addrs(&addrs, "inference:ollama:chat");
        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].ip().is_loopback());
        let passthrough = super::filter_inference_pin_addrs(&addrs, "enrollment:probe");
        assert_eq!(passthrough.len(), 2);
    }

    #[tokio::test]
    async fn loopback_wrong_port_denied() {
        let g = EgressGuard::new();
        assert!(g.authorize("127.0.0.1", 8080, "test").await.is_err());
    }

    #[tokio::test]
    async fn loopback_configured_port_allowed() {
        let g = EgressGuard::new();
        g.configure_loopback_inference(8080);
        let addrs = g.authorize("127.0.0.1", 8080, "test").await.unwrap();
        assert!(addrs.iter().all(|sa| sa.ip().is_loopback()));
        assert!(g.authorize("127.0.0.1", 11434, "test").await.is_err());
    }

    #[tokio::test]
    async fn enrolled_node_is_allowed() {
        let g = EgressGuard::new();
        g.allow_node("workstation-2", "10.0.0.5".parse().unwrap(), Some(11434));
        let addrs = g.authorize("10.0.0.5", 11434, "test").await.unwrap();
        assert_eq!(addrs, vec![SocketAddr::from(([10, 0, 0, 5], 11434))]);
        // wrong port still denied
        assert!(g.authorize("10.0.0.5", 22, "test").await.is_err());
    }

    #[tokio::test]
    async fn loopback_enrollment_port_allowed_with_rule() {
        let g = EgressGuard::new();
        g.allow_node("_enroll:test", "127.0.0.1".parse().unwrap(), Some(9470));
        let addrs = g
            .authorize("127.0.0.1", 9470, "enroll:complete")
            .await
            .unwrap();
        assert_eq!(addrs, vec![SocketAddr::from(([127, 0, 0, 1], 9470))]);
        assert!(g.authorize("127.0.0.1", 9471, "test").await.is_err());
    }

    #[test]
    fn client_for_ip_literal_reuses_shared_client() {
        // IP-literal targets have no DNS to subvert, so we reuse the pooled client.
        let g = EgressGuard::new();
        let addrs = vec![SocketAddr::from(([127, 0, 0, 1], 11434))];
        assert!(g.client_for("127.0.0.1", &addrs).is_ok());
        // Hostname targets get a resolution-pinned client, cached across calls so
        // the connection pool is reused instead of rebuilt every request.
        assert!(g.client_for("localhost", &addrs).is_ok());
        assert!(g.client_for("localhost", &addrs).is_ok());
        assert_eq!(
            g.clients.lock().unwrap().len(),
            1,
            "repeat hostname+addrs reuses one cached client"
        );
    }

    #[test]
    fn activity_log_ring_buffer_caps_at_max() {
        let g = EgressGuard::new();
        for i in 0..MAX_ACTIVITY_LOG + 50 {
            g.record(EgressEvent {
                ts: format!("t{i}"),
                initiator: "test".into(),
                host: "127.0.0.1".into(),
                resolved_ip: None,
                port: 11434,
                decision: Action::Allow,
                reason: format!("event {i}"),
            });
        }
        let log = g.activity_log();
        assert_eq!(log.len(), MAX_ACTIVITY_LOG);
        assert_eq!(log.first().unwrap().reason, "event 50");
        assert_eq!(
            log.last().unwrap().reason,
            format!("event {}", MAX_ACTIVITY_LOG + 49)
        );
    }

    #[tokio::test]
    async fn canary_check_target_unreachable_fails_fast() {
        let g = EgressGuard::new();
        g.allow_node(
            "unreachable-node",
            "10.255.255.255".parse().unwrap(),
            Some(11434),
        );
        let active = g
            .canary_check_target("10.255.255.255", 11434, "canary:ping")
            .await
            .unwrap();
        assert!(
            !active,
            "unreachable target returns false in 250ms rather than hanging"
        );
    }

    #[test]
    fn loopback_inference_pins_configured_bind() {
        let g = EgressGuard::loopback_inference(11434);
        let rules = g.allow_rules();
        assert!(
            rules
                .iter()
                .any(|r| r.ip == IpAddr::V4(Ipv4Addr::LOCALHOST) && r.port == Some(11434)),
            "loopback_inference must list 127.0.0.1:11434 in allow_rules"
        );
    }

    #[test]
    fn pinned_to_inference_url_pins_non_loopback_ip() {
        let g = EgressGuard::pinned_to_inference_url("http://10.0.0.9:11434");
        let rules = g.allow_rules();
        assert!(
            rules
                .iter()
                .any(|r| r.ip == "10.0.0.9".parse::<IpAddr>().unwrap() && r.port == Some(11434)),
            "non-loopback Ollama bind must be an explicit allow_node"
        );
    }

    #[tokio::test]
    async fn retract_generation_denies_that_pin() {
        let g = EgressGuard::new();
        let gen = g.allow_node("n", "10.0.0.5".parse().unwrap(), Some(11434));
        g.authorize("10.0.0.5", 11434, "test")
            .await
            .expect("pin allows");
        g.retract_generation(gen);
        assert!(
            g.authorize("10.0.0.5", 11434, "test").await.is_err(),
            "retract_generation N must deny the retracted pin"
        );
        let gen2 = g.allow_node("n2", "10.0.0.6".parse().unwrap(), Some(11434));
        g.retract_generation(gen);
        g.authorize("10.0.0.6", 11434, "test")
            .await
            .expect("other generation still allowed");
        g.retract_generation(gen2);
        assert!(g.authorize("10.0.0.6", 11434, "test").await.is_err());
    }
}
