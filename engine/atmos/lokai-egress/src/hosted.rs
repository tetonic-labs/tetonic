//! Explicit HTTPS endpoint grants, separate from enrolled fabric nodes.
use std::net::IpAddr;
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::Value;

use crate::{Action, EgressError, EgressEvent, EgressGuard};

const MAX_BODY: usize = 8 * 1024 * 1024;

/// Runtime-only credential. Neither serializable nor printable in diagnostics.
#[derive(Clone)]
pub struct HostedCredential {
    pub(crate) headers: Vec<(reqwest::header::HeaderName, reqwest::header::HeaderValue)>,
}

impl std::fmt::Debug for HostedCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HostedCredential([REDACTED])")
    }
}

impl HostedCredential {
    pub fn bearer(secret: &str) -> Result<Self, EgressError> {
        if secret.is_empty()
            || secret
                .bytes()
                .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
        {
            return Err(failure("invalid bearer credential"));
        }
        let mut value = reqwest::header::HeaderValue::from_str(&format!("Bearer {secret}"))
            .map_err(|_| failure("invalid bearer credential"))?;
        value.set_sensitive(true);
        Ok(Self {
            headers: vec![(reqwest::header::AUTHORIZATION, value)],
        })
    }

    pub fn anthropic(api_key: &str) -> Result<Self, EgressError> {
        if api_key.is_empty()
            || api_key
                .bytes()
                .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
        {
            return Err(failure("invalid anthropic api key"));
        }
        let key_name = reqwest::header::HeaderName::from_static("x-api-key");
        let mut key_val = reqwest::header::HeaderValue::from_str(api_key)
            .map_err(|_| failure("invalid anthropic api key"))?;
        key_val.set_sensitive(true);

        let ver_name = reqwest::header::HeaderName::from_static("anthropic-version");
        let ver_val = reqwest::header::HeaderValue::from_static("2023-06-01");

        Ok(Self {
            headers: vec![(key_name, key_val), (ver_name, ver_val)],
        })
    }

    pub fn api_key(header_name: &str, api_key: &str) -> Result<Self, EgressError> {
        if api_key.is_empty()
            || api_key
                .bytes()
                .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
        {
            return Err(failure("invalid api key"));
        }
        let name = reqwest::header::HeaderName::from_bytes(header_name.as_bytes())
            .map_err(|_| failure("invalid header name"))?;
        let mut val = reqwest::header::HeaderValue::from_str(api_key)
            .map_err(|_| failure("invalid api key value"))?;
        val.set_sensitive(true);
        Ok(Self {
            headers: vec![(name, val)],
        })
    }

    pub fn custom(
        headers: Vec<(reqwest::header::HeaderName, reqwest::header::HeaderValue)>,
    ) -> Self {
        Self { headers }
    }
}

impl AsRef<HostedCredential> for HostedCredential {
    fn as_ref(&self) -> &HostedCredential {
        self
    }
}

/// Backward-compatible BearerCredential wrapper.
#[derive(Clone)]
pub struct BearerCredential(pub(crate) HostedCredential);

impl std::fmt::Debug for BearerCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BearerCredential([REDACTED])")
    }
}

impl BearerCredential {
    pub fn new(secret: &str) -> Result<Self, EgressError> {
        HostedCredential::bearer(secret).map(Self)
    }
}

impl From<BearerCredential> for HostedCredential {
    fn from(b: BearerCredential) -> Self {
        b.0
    }
}

impl AsRef<HostedCredential> for BearerCredential {
    fn as_ref(&self) -> &HostedCredential {
        &self.0
    }
}

fn failure(message: &str) -> EgressError {
    EgressError::StreamDecode(message.into())
}

fn endpoint(url: &str) -> Result<url::Url, EgressError> {
    let parsed = url::Url::parse(url).map_err(|_| failure("invalid hosted endpoint"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(failure(
            "hosted endpoint requires HTTPS without credentials, query, or fragment",
        ));
    }
    Ok(parsed)
}

/// Conservative public-unicast filter; hosted grants never grant LAN access.
fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, _, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || a == 0
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 198 && (18..=19).contains(&b))
                || (a == 192 && b == 0))
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            // Global unicast only, excluding protocol assignments and documentation.
            (s[0] & 0xe000) == 0x2000
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                && !(s[0] == 0x2002 || (s[0] == 0x3fff && s[1] < 0x1000))
        }
    }
}

impl EgressGuard {
    /// Explicit operator enrollment of one complete HTTPS API URL. Does not
    /// enable other URLs, ordinary HTTP methods, or fabric-worker enrollment.
    pub fn allow_hosted_endpoint(&self, url: &str) -> Result<(), EgressError> {
        let url = endpoint(url)?.to_string();
        let mut grants = self
            .hosted_endpoints
            .lock()
            .map_err(|_| failure("hosted grants unavailable"))?;
        if !grants.contains(&url) {
            grants.push(url);
        }
        Ok(())
    }

    pub fn revoke_hosted_endpoint(&self, url: &str) -> Result<(), EgressError> {
        let url = endpoint(url)?.to_string();
        self.hosted_endpoints
            .lock()
            .map_err(|_| failure("hosted grants unavailable"))?
            .retain(|entry| entry != &url);
        Ok(())
    }

    /// Bounded, authenticated JSON POST. No redirects, proxy inheritance, retry,
    /// or response-body logging. DNS is pinned while retaining TLS verification.
    pub async fn post_hosted_json<C: AsRef<HostedCredential>>(
        &self,
        url: &str,
        body: &Value,
        credential: &C,
    ) -> Result<Value, EgressError> {
        tokio::time::timeout(
            Duration::from_secs(120),
            self.post_hosted_inner(url, body, credential.as_ref()),
        )
        .await
        .map_err(|_| failure("hosted request timed out"))?
    }

    async fn post_hosted_inner(
        &self,
        url: &str,
        body: &Value,
        credential: &HostedCredential,
    ) -> Result<Value, EgressError> {
        let parsed = endpoint(url)?;
        let host = parsed
            .host_str()
            .ok_or_else(|| failure("missing hosted host"))?;
        let port = parsed.port_or_known_default().unwrap_or(443);
        let granted = self
            .hosted_endpoints
            .lock()
            .map_err(|_| failure("hosted grants unavailable"))?
            .contains(&parsed.to_string());
        self.record(EgressEvent {
            ts: crate::now(),
            initiator: "inference:hosted".into(),
            host: host.into(),
            resolved_ip: None,
            port,
            decision: if granted { Action::Allow } else { Action::Deny },
            reason: "explicit hosted endpoint grant".into(),
        });
        if !granted {
            return Err(EgressError::Denied {
                host: host.into(),
                port,
                reason: "hosted endpoint not enrolled".into(),
            });
        }
        let addrs = Self::resolve_host(host, port).await?;
        let private_allowed = self.is_private_hosted_allowed();
        if !private_allowed && addrs.iter().any(|a| !public_address(a.ip())) {
            self.record(EgressEvent {
                ts: crate::now(),
                initiator: "inference:hosted".into(),
                host: host.into(),
                resolved_ip: None,
                port,
                decision: Action::Deny,
                reason: "non-public hosted address".into(),
            });
            return Err(failure("hosted endpoint resolved to a non-public address"));
        }
        let mut addresses: Vec<_> = addrs.iter().map(ToString::to_string).collect();
        addresses.sort();
        let key = format!("hosted:{host}:{port}|{}", addresses.join(","));
        let cached = self
            .clients
            .lock()
            .map_err(|_| failure("HTTP client cache unavailable"))?
            .get(&key)
            .cloned();
        let client = if let Some(client) = cached {
            client
        } else {
            let has_proxy = std::env::var("HTTPS_PROXY").is_ok()
                || std::env::var("https_proxy").is_ok()
                || std::env::var("ALL_PROXY").is_ok()
                || std::env::var("all_proxy").is_ok();

            let mut builder = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(10));

            if !has_proxy {
                // Strict DNS pinning to prevent DNS rebinding when connecting directly
                builder = builder.resolve_to_addrs(host, &addrs);
            }

            let client = builder
                .build()
                .map_err(|_| failure("hosted HTTP client unavailable"))?;
            self.clients
                .lock()
                .map_err(|_| failure("HTTP client cache unavailable"))?
                .insert(key, client.clone());
            client
        };
        let bytes = serde_json::to_vec(body).map_err(|_| failure("invalid hosted request"))?;
        if bytes.len() > MAX_BODY {
            return Err(failure("hosted request exceeds 8 MiB"));
        }
        let mut req_builder = client
            .post(parsed)
            .header(reqwest::header::CONTENT_TYPE, "application/json");

        for (header_name, header_value) in &credential.headers {
            req_builder = req_builder.header(header_name.clone(), header_value.clone());
        }

        let resp = req_builder
            .body(bytes)
            .send()
            .await
            .map_err(|_| failure("hosted HTTP transport failed"))?;
        if !resp.status().is_success() {
            return Err(failure(&format!(
                "hosted HTTP status {} (not retried)",
                resp.status().as_u16()
            )));
        }
        let mut stream = resp.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| failure("hosted response interrupted"))?;
            if bytes.len().saturating_add(chunk.len()) > MAX_BODY {
                return Err(failure("hosted response exceeds 8 MiB"));
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| failure("invalid hosted response JSON"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_endpoint_and_hides_credentials() {
        for url in [
            "http://example.com/api",
            "https://key@example.com/api",
            "https://example.com/?key=x",
            "https://example.com/#x",
        ] {
            assert!(endpoint(url).is_err());
        }
        assert!(endpoint("https://example.com/v1/chat/completions").is_ok());
        assert_eq!(
            format!("{:?}", BearerCredential::new("test-secret").unwrap()),
            "BearerCredential([REDACTED])"
        );
        assert!(BearerCredential::new("x\r\ny").is_err());
    }

    #[test]
    fn rejects_private_and_special_addresses() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "192.168.1.1",
            "198.18.0.1",
            "224.0.0.1",
            "::1",
            "::ffff:127.0.0.1",
            "fc00::1",
            "2001:db8::1",
            "2002:7f00:1::",
        ] {
            assert!(!public_address(ip.parse().unwrap()), "{ip}");
        }
        assert!(public_address("8.8.8.8".parse().unwrap()));
        assert!(public_address("2606:4700:4700::1111".parse().unwrap()));
    }

    #[tokio::test]
    async fn grants_are_exact_revocable_and_default_denied() {
        let guard = EgressGuard::new();
        let key = BearerCredential::new("test").unwrap();
        let url = "https://example.invalid/v1/chat/completions";
        guard.allow_hosted_endpoint(url).unwrap();
        assert!(matches!(
            guard
                .post_hosted_json("https://example.invalid/other", &Value::Null, &key)
                .await,
            Err(EgressError::Denied { .. })
        ));
        guard.revoke_hosted_endpoint(url).unwrap();
        assert!(matches!(
            guard.post_hosted_json(url, &Value::Null, &key).await,
            Err(EgressError::Denied { .. })
        ));
        assert!(guard.allow_rules().is_empty());
    }

    #[tokio::test]
    async fn hosted_permission_cannot_target_enrolled_loopback() {
        let guard = EgressGuard::loopback_inference(11434);
        let url = "https://127.0.0.1:11434/v1/chat/completions";
        guard.allow_hosted_endpoint(url).unwrap();
        let key = BearerCredential::new("test").unwrap();
        let error = guard
            .post_hosted_json(url, &Value::Null, &key)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("non-public"));
        assert_eq!(guard.activity_log().last().unwrap().decision, Action::Deny);
    }

    #[test]
    fn anthropic_credential_formats_headers_correctly() {
        let cred = HostedCredential::anthropic("test-anthropic-key").unwrap();
        assert_eq!(cred.headers.len(), 2);
        assert_eq!(cred.headers[0].0.as_str(), "x-api-key");
        assert_eq!(cred.headers[0].1.to_str().unwrap(), "test-anthropic-key");
        assert_eq!(cred.headers[1].0.as_str(), "anthropic-version");
        assert_eq!(cred.headers[1].1.to_str().unwrap(), "2023-06-01");
        assert_eq!(format!("{cred:?}"), "HostedCredential([REDACTED])");
    }

    #[tokio::test]
    async fn private_hosted_allowed_when_explicitly_enabled() {
        let guard = EgressGuard::new();
        assert!(!guard.is_private_hosted_allowed());
        guard.allow_private_hosted_endpoints(true);
        assert!(guard.is_private_hosted_allowed());
    }
}
