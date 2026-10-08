//! Bounded MCP Streamable HTTP transport. Caller must explicitly enroll the
//! loopback port; no credential, redirect, DNS name or arbitrary remote endpoint.
use crate::{EgressError, EgressGuard, HostedCredential};
use futures_util::StreamExt;
use serde_json::Value;

mod endpoint;
#[cfg(test)]
mod tests;
pub use endpoint::mcp_endpoint;

pub struct McpReply {
    pub session: Option<String>,
    pub message: Option<Value>,
}

pub fn local_mcp_endpoint(value: &str) -> Result<(String, std::net::SocketAddr), EgressError> {
    let url =
        url::Url::parse(value).map_err(|_| EgressError::Url("invalid MCP endpoint".into()))?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("127.0.0.1" | "[::1]"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() == Some(0)
    {
        return Err(EgressError::Url(
            "MCP requires a numeric loopback HTTP endpoint without credentials, query or fragment"
                .into(),
        ));
    }
    let ip = if url.host_str() == Some("127.0.0.1") {
        std::net::Ipv4Addr::LOCALHOST.into()
    } else {
        std::net::Ipv6Addr::LOCALHOST.into()
    };
    Ok((
        url.to_string(),
        std::net::SocketAddr::new(ip, url.port_or_known_default().unwrap_or(80)),
    ))
}

impl EgressGuard {
    async fn local_mcp_client(
        &self,
        endpoint: &str,
    ) -> Result<(String, reqwest::Client), EgressError> {
        let (endpoint, address) = local_mcp_endpoint(endpoint)?;
        self.authorize(&address.ip().to_string(), address.port(), "mcp")
            .await?;
        let mut clients = self.clients.lock().expect("egress client cache poisoned");
        if let Some(client) = clients.get("mcp-loopback-direct") {
            return Ok((endpoint, client.clone()));
        }
        // Never route a local tool's arguments through a process/system proxy.
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_millis(500))
            .build()?;
        clients.insert("mcp-loopback-direct".into(), client.clone());
        Ok((endpoint, client))
    }
    /// Best-effort release of a short-lived MCP session; never follow redirects.
    pub async fn end_local_mcp(
        &self,
        endpoint: &str,
        session: &str,
        version: &str,
    ) -> Result<(), EgressError> {
        self.end_mcp(endpoint, session, version, None).await
    }
    pub async fn end_mcp(
        &self,
        endpoint: &str,
        session: &str,
        version: &str,
        credential: Option<&HostedCredential>,
    ) -> Result<(), EgressError> {
        let (url, client) = self.mcp_client(endpoint).await?;
        let mut request = client
            .delete(url)
            .timeout(std::time::Duration::from_millis(500))
            .header("MCP-Session-Id", session)
            .header("MCP-Protocol-Version", version);
        if let Some(credential) = credential {
            for (name, value) in &credential.headers {
                request = request.header(name, value);
            }
        }
        let response = request.send().await?;
        if response.status().is_success() || matches!(response.status().as_u16(), 404 | 405) {
            Ok(())
        } else {
            Err(EgressError::StreamDecode(
                "MCP session close was not confirmed".into(),
            ))
        }
    }
    pub async fn post_local_mcp(
        &self,
        endpoint: &str,
        body: &Value,
        session: Option<&str>,
        version: &str,
    ) -> Result<McpReply, EgressError> {
        self.post_mcp(endpoint, body, session, version, None).await
    }
    pub async fn post_mcp(
        &self,
        endpoint: &str,
        body: &Value,
        session: Option<&str>,
        version: &str,
        credential: Option<&HostedCredential>,
    ) -> Result<McpReply, EgressError> {
        let (url, client) = self.mcp_client(endpoint).await?;
        let mut request = client
            .post(url)
            .timeout(std::time::Duration::from_secs(8))
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", version)
            .json(body);
        if let Some(id) = session {
            request = request.header("MCP-Session-Id", id);
        }
        if let Some(credential) = credential {
            for (name, value) in &credential.headers {
                request = request.header(name, value);
            }
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(EgressError::StreamDecode(format!(
                "MCP HTTP {}",
                response.status().as_u16()
            )));
        }
        let session = response
            .headers()
            .get("MCP-Session-Id")
            .map(|v| v.to_str().unwrap_or("").to_owned());
        if session.as_ref().is_some_and(|id| {
            id.is_empty() || id.len() > 256 || !id.bytes().all(|c| (0x21..=0x7e).contains(&c))
        }) {
            return Err(EgressError::StreamDecode("invalid MCP session".into()));
        }
        if body.get("id").is_none() {
            return if response.status() == 202 {
                Ok(McpReply {
                    session,
                    message: None,
                })
            } else {
                Err(EgressError::StreamDecode(
                    "MCP notification was not accepted".into(),
                ))
            };
        }
        let mime = response
            .headers()
            .get("Content-Type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        let sse = mime == "text/event-stream";
        if !sse && mime != "application/json" {
            return Err(EgressError::StreamDecode(
                "unsupported MCP response type".into(),
            ));
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        let mut decoder = SseDecoder::default();
        let mut received = 0usize;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            received += chunk.len();
            if received > 131_072 {
                return Err(EgressError::StreamDecode(
                    "MCP response exceeds limit".into(),
                ));
            }
            if sse {
                for message in decoder.push(&chunk)? {
                    if message.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
                        && message.get("method").and_then(Value::as_str).is_some()
                        && message.get("id").is_none()
                        && message.get("result").is_none()
                        && message.get("error").is_none()
                    {
                        continue;
                    }
                    return checked_reply(session, message, body);
                }
            } else {
                bytes.extend_from_slice(&chunk);
            }
        }
        if sse {
            return Err(EgressError::StreamDecode(
                "MCP stream ended without a response".into(),
            ));
        }
        let message = serde_json::from_slice(&bytes)
            .map_err(|_| EgressError::StreamDecode("invalid MCP response".into()))?;
        checked_reply(session, message, body)
    }
}

/// Decode each byte once, retaining incomplete UTF-8 lines across HTTP chunks.
/// Streamable HTTP permits LF, CRLF and CR line endings; an event needs a blank
/// line. No reconnect/resume or server-initiated request support is implied.
#[derive(Default)]
struct SseDecoder {
    line: Vec<u8>,
    data: Vec<String>,
    after_cr: bool,
}
impl SseDecoder {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<Value>, EgressError> {
        let mut messages = Vec::new();
        for &byte in bytes {
            if self.after_cr && byte == b'\n' {
                self.after_cr = false;
                continue;
            }
            self.after_cr = byte == b'\r';
            if byte != b'\r' && byte != b'\n' {
                self.line.push(byte);
                continue;
            }
            let line = std::str::from_utf8(&self.line)
                .map_err(|_| EgressError::StreamDecode("invalid MCP UTF-8".into()))?;
            if line.is_empty() && !self.data.is_empty() {
                let data = self.data.join("\n");
                self.data.clear();
                if !data.is_empty() {
                    messages.push(
                        serde_json::from_str(&data)
                            .map_err(|_| EgressError::StreamDecode("invalid MCP event".into()))?,
                    );
                }
            } else if let Some(data) = line.strip_prefix("data:") {
                self.data
                    .push(data.strip_prefix(' ').unwrap_or(data).to_owned());
            } else if line == "data" {
                self.data.push(String::new());
            }
            self.line.clear();
        }
        Ok(messages)
    }
}

fn checked_reply(
    session: Option<String>,
    message: Value,
    request: &Value,
) -> Result<McpReply, EgressError> {
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || message.get("id") != request.get("id")
        || message.get("method").is_some()
        || message.get("result").is_some() == message.get("error").is_some()
    {
        return Err(EgressError::StreamDecode(
            "MCP response does not match request".into(),
        ));
    }
    Ok(McpReply {
        session,
        message: Some(message),
    })
}
