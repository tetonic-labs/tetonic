use crate::{Action, EgressError, EgressEvent, EgressGuard};

/// Service tokens are bound to one complete endpoint. Remote connections require
/// public HTTPS; private networks use the separately enrolled loopback profile.
pub fn mcp_endpoint(value: &str) -> Result<String, EgressError> {
    if value.starts_with("http:") {
        return super::local_mcp_endpoint(value).map(|(url, _)| url);
    }
    let url = crate::hosted::endpoint(value)?;
    if url.port_or_known_default() == Some(0) {
        return Err(EgressError::Url("Invalid MCP port".into()));
    }
    Ok(url.to_string())
}

impl EgressGuard {
    pub(super) async fn mcp_client(
        &self,
        value: &str,
    ) -> Result<(String, reqwest::Client), EgressError> {
        if value.starts_with("http:") {
            return self.local_mcp_client(value).await;
        }
        let endpoint = mcp_endpoint(value)?;
        let url = crate::hosted::endpoint(&endpoint)?;
        let host = url
            .host_str()
            .ok_or_else(|| EgressError::Url("Missing MCP host".into()))?;
        let port = url.port_or_known_default().unwrap_or(443);
        let granted = self
            .hosted_endpoints
            .lock()
            .expect("endpoint grants poisoned")
            .contains(&endpoint);
        self.record(EgressEvent {
            ts: crate::now(),
            initiator: "mcp".into(),
            host: host.into(),
            resolved_ip: None,
            port,
            decision: if granted { Action::Allow } else { Action::Deny },
            reason: "explicit MCP endpoint grant".into(),
        });
        if !granted {
            return Err(EgressError::Denied {
                host: host.into(),
                port,
                reason: "MCP endpoint not enrolled".into(),
            });
        }
        let addresses = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            Self::resolve_host(host, port),
        )
        .await
        .map_err(|_| EgressError::Url("MCP DNS lookup timed out".into()))??;
        if addresses.is_empty()
            || addresses
                .iter()
                .any(|a| !crate::hosted::public_address(a.ip()))
        {
            return Err(EgressError::Url(
                "Remote MCP endpoint resolved to a non-public address".into(),
            ));
        }
        // Revalidate DNS every request; pin the checked answers into this client.
        // No ambient proxy, redirect, or TLS bypass can forward a service token.
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(5))
            .resolve_to_addrs(host, &addresses)
            .build()?;
        Ok((endpoint, client))
    }
}
