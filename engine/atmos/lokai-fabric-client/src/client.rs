//! mTLS fabric HTTP client (coordinator → worker). N0.4.
//!
//! All connections go through [`EgressGuard::ensure_allowed`] before opening a socket.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::tls_handshake::{
    supported_ed25519_schemes, verify_tls12_handshake_signature, verify_tls13_handshake_signature,
};
use ed25519_dalek::pkcs8::EncodePrivateKey;
use ed25519_dalek::SigningKey;
use lokai_egress::EgressGuard;
use lokai_enroll::KeyPair;
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair as RcgenKeyPair};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use rustls::{DigitallySignedStruct, Error as RustlsError, SignatureScheme};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

pub const FABRIC_SERVER_NAME: &str = "lokai-worker";

pub const FABRIC_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
pub const FABRIC_CHAT_TIMEOUT: Duration = Duration::from_secs(120);

/// Upper bound on a single fabric HTTP response body (JSON or NDJSON aggregate).
pub const MAX_FABRIC_BODY_BYTES: usize = 8 * 1024 * 1024;

fn fabric_deadlines(path: &str) -> (Duration, Duration) {
    if path.starts_with("/v1/chat") || path.starts_with("/v1/jobs") {
        (Duration::from_millis(500), FABRIC_CHAT_TIMEOUT)
    } else {
        (Duration::from_millis(500), FABRIC_PROBE_TIMEOUT)
    }
}

#[derive(Debug, Error)]
pub enum FabricClientError {
    #[error("protocol: {0}")]
    Protocol(#[from] lokai_fabric_protocol::FabricError),
    #[error("egress: {0}")]
    Egress(#[from] lokai_egress::EgressError),
    #[error("tls: {0}")]
    Tls(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("http: {0}")]
    Http(String),
    #[error("missing worker fabric TLS certificate")]
    NoServerCert,
    #[error("request timed out ({phase}): {path}")]
    Timeout { phase: &'static str, path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Parse an HTTP/1.1 response when the full message is available in memory.
#[allow(dead_code)] // sync parser; exercised in unit tests (async path uses `read_http_response`)
pub fn parse_http_response(
    raw: &[u8],
    max_body: usize,
) -> Result<ParsedHttpResponse, FabricClientError> {
    if raw.len() > max_body.saturating_add(16 * 1024) {
        return Err(FabricClientError::Http("response too large".into()));
    }
    let Some(header_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Err(FabricClientError::Http("incomplete response".into()));
    };
    let headers = std::str::from_utf8(&raw[..header_end])
        .map_err(|_| FabricClientError::Http("invalid header encoding".into()))?;
    let status = parse_status_line(headers)?;
    let content_length = parse_content_length(headers)?;
    let body_start = header_end + 4;
    let body = if let Some(len) = content_length {
        if len > max_body {
            return Err(FabricClientError::Http("Content-Length exceeds cap".into()));
        }
        let body_end = body_start + len;
        if raw.len() < body_end {
            return Err(FabricClientError::Http("truncated body".into()));
        }
        raw[body_start..body_end].to_vec()
    } else {
        let rest = &raw[body_start..];
        if rest.len() > max_body {
            return Err(FabricClientError::Http("body exceeds cap".into()));
        }
        rest.to_vec()
    };
    Ok(ParsedHttpResponse { status, body })
}

fn parse_status_line(headers: &str) -> Result<u16, FabricClientError> {
    let line = headers
        .lines()
        .next()
        .ok_or_else(|| FabricClientError::Http("missing status line".into()))?;
    line.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| FabricClientError::Http("invalid status line".into()))
}

fn parse_content_length(headers: &str) -> Result<Option<usize>, FabricClientError> {
    for line in headers.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("Content-Length") {
            let len: usize = value
                .trim()
                .parse()
                .map_err(|_| FabricClientError::Http("invalid Content-Length".into()))?;
            return Ok(Some(len));
        }
    }
    Ok(None)
}

async fn read_bounded_body<R: AsyncRead + Unpin>(
    reader: &mut R,
    content_length: Option<usize>,
    max_bytes: usize,
) -> Result<Vec<u8>, FabricClientError> {
    let mut out = Vec::new();
    let mut chunk = [0u8; 4096];
    if let Some(len) = content_length {
        if len > max_bytes {
            return Err(FabricClientError::Http("Content-Length exceeds cap".into()));
        }
        out.reserve(len);
        while out.len() < len {
            let to_read = chunk.len().min(len - out.len());
            let n = reader.read(&mut chunk[..to_read]).await?;
            if n == 0 {
                return Err(FabricClientError::Http("truncated body".into()));
            }
            out.extend_from_slice(&chunk[..n]);
        }
        return Ok(out);
    }
    loop {
        let n = reader.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        if out.len() + n > max_bytes {
            return Err(FabricClientError::Http("body exceeds cap".into()));
        }
        out.extend_from_slice(&chunk[..n]);
    }
    Ok(out)
}

async fn read_http_response<R: AsyncRead + Unpin>(
    reader: &mut R,
    max_body: usize,
) -> Result<ParsedHttpResponse, FabricClientError> {
    use tokio::io::AsyncBufReadExt;

    let mut buf_reader = tokio::io::BufReader::new(reader);
    let mut status = 0u16;
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        buf_reader.read_line(&mut line).await?;
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if status == 0 {
            status = parse_status_line(trimmed)?;
        }
        headers.push_str(&line);
    }
    let content_length = parse_content_length(&headers)?;
    let body = read_bounded_body(buf_reader.get_mut(), content_length, max_body).await?;
    Ok(ParsedHttpResponse { status, body })
}

/// Client cert whose public key matches `kp.public()` (coordinator mTLS identity).
pub fn client_cert_from_keypair(
    kp: &KeyPair,
    cn: &str,
) -> Result<(Vec<u8>, Vec<u8>), FabricClientError> {
    let signing = SigningKey::from_bytes(&kp.signing_bytes());
    let pkcs8 = signing
        .to_pkcs8_der()
        .map_err(|e| FabricClientError::Tls(e.to_string()))?;
    let pkcs8_der = PrivatePkcs8KeyDer::from(pkcs8.as_bytes().to_vec());
    let rc_key = RcgenKeyPair::from_pkcs8_der_and_sign_algo(&pkcs8_der, &rcgen::PKCS_ED25519)
        .map_err(|e| FabricClientError::Tls(e.to_string()))?;
    let mut params = CertificateParams::new(vec![cn.to_string()])
        .map_err(|e| FabricClientError::Tls(e.to_string()))?;
    params.distinguished_name = DistinguishedName::new();
    params.distinguished_name.push(DnType::CommonName, cn);
    let cert = params
        .self_signed(&rc_key)
        .map_err(|e| FabricClientError::Tls(e.to_string()))?;
    Ok((cert.der().to_vec(), rc_key.serialize_der()))
}

#[derive(Debug)]
struct PinnedServerVerifier {
    cert: Vec<u8>,
}

impl ServerCertVerifier for PinnedServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        if end_entity.as_ref() == self.cert.as_slice() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(RustlsError::General("unknown server certificate".into()))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls12_handshake_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls13_handshake_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        supported_ed25519_schemes()
    }
}

pub fn build_client_config(
    trusted_server_cert: &[u8],
    client_cert: &[u8],
    client_key: &[u8],
) -> Result<Arc<rustls::ClientConfig>, FabricClientError> {
    let cert = CertificateDer::from(client_cert.to_vec());
    let key = PrivateKeyDer::try_from(client_key.to_vec())
        .map_err(|e| FabricClientError::Tls(e.to_string()))?;
    let mut config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedServerVerifier {
            cert: trusted_server_cert.to_vec(),
        }))
        .with_client_auth_cert(vec![cert], key)
        .map_err(|e| FabricClientError::Tls(e.to_string()))?;
    config.resumption = rustls::client::Resumption::disabled();
    Ok(Arc::new(config))
}

struct FabricTls {
    stream: TlsStream<TcpStream>,
}

async fn open_fabric_tls(
    guard: &EgressGuard,
    ip: IpAddr,
    port: u16,
    server_cert: &[u8],
    coordinator: &KeyPair,
    path: &str,
    initiator: &str,
) -> Result<FabricTls, FabricClientError> {
    if server_cert.is_empty() {
        return Err(FabricClientError::NoServerCert);
    }
    guard
        .ensure_allowed(&ip.to_string(), port, initiator)
        .await?;

    let (client_cert, client_key) = client_cert_from_keypair(coordinator, "coordinator")?;
    let client_config = build_client_config(server_cert, &client_cert, &client_key)?;
    let connector = TlsConnector::from(client_config);

    let (connect_timeout, _) = fabric_deadlines(path);
    let stream = match tokio::time::timeout(connect_timeout, TcpStream::connect((ip, port))).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(FabricClientError::Io(e)),
        Err(_) => {
            tracing::warn!(ip = %ip, port, path, initiator, "fabric connect timed out");
            return Err(FabricClientError::Timeout {
                phase: "connect",
                path: path.to_string(),
            });
        }
    };
    let server_name = ServerName::try_from(FABRIC_SERVER_NAME).expect("static server name");
    let tls = connector.connect(server_name, stream).await?;
    Ok(FabricTls { stream: tls })
}

fn build_http_request(method: &str, path: &str, body: Option<&str>, ndjson: bool) -> String {
    if let Some(body) = body {
        let accept = if ndjson {
            "Accept: application/x-ndjson\r\n"
        } else {
            ""
        };
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {FABRIC_SERVER_NAME}\r\n{accept}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    } else {
        let accept = if ndjson {
            "Accept: application/x-ndjson\r\n"
        } else {
            ""
        };
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {FABRIC_SERVER_NAME}\r\n{accept}Connection: close\r\n\r\n"
        )
    }
}

#[derive(Debug, Clone)]
pub struct FabricHttpResponse {
    pub status: u16,
    pub body: String,
}

/// One mTLS HTTP/1.1 request to a worker fabric listener.
#[allow(clippy::too_many_arguments)]
pub async fn fabric_request(
    guard: &EgressGuard,
    ip: IpAddr,
    port: u16,
    server_cert: &[u8],
    coordinator: &KeyPair,
    method: &str,
    path: &str,
    body: Option<&str>,
    initiator: &str,
) -> Result<FabricHttpResponse, FabricClientError> {
    let (_, read_timeout) = fabric_deadlines(path);
    let mut conn =
        open_fabric_tls(guard, ip, port, server_cert, coordinator, path, initiator).await?;
    let req = build_http_request(method, path, body, false);

    let parsed = match tokio::time::timeout(read_timeout, async {
        conn.stream.write_all(req.as_bytes()).await?;
        read_http_response(&mut conn.stream, MAX_FABRIC_BODY_BYTES).await
    })
    .await
    {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            tracing::warn!(ip = %ip, port, path, initiator, "fabric read timed out");
            return Err(FabricClientError::Timeout {
                phase: "read",
                path: path.to_string(),
            });
        }
    };

    Ok(FabricHttpResponse {
        status: parsed.status,
        body: String::from_utf8(parsed.body).map_err(|e| FabricClientError::Http(e.to_string()))?,
    })
}

/// mTLS HTTP/1.1 expecting an NDJSON body (`application/x-ndjson` or legacy single JSON).
#[allow(clippy::too_many_arguments)]
pub async fn fabric_request_ndjson(
    guard: &EgressGuard,
    ip: IpAddr,
    port: u16,
    server_cert: &[u8],
    coordinator: &KeyPair,
    method: &str,
    path: &str,
    body: Option<&str>,
    initiator: &str,
    mut on_line: impl FnMut(&str) -> Result<(), FabricClientError>,
) -> Result<u16, FabricClientError> {
    use tokio::io::AsyncBufReadExt;

    let (_, read_timeout) = fabric_deadlines(path);
    let mut conn =
        open_fabric_tls(guard, ip, port, server_cert, coordinator, path, initiator).await?;
    let req = build_http_request(method, path, body, true);

    let status = match tokio::time::timeout(read_timeout, async {
        conn.stream.write_all(req.as_bytes()).await?;
        let mut reader = tokio::io::BufReader::new(&mut conn.stream);
        let mut status = 0u16;
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).await?;
            let h = header.trim_end();
            if h.is_empty() {
                break;
            }
            if status == 0 && !h.contains(':') {
                status = parse_status_line(h)?;
            }
        }
        let mut line = String::new();
        let mut total = 0usize;
        loop {
            line.clear();
            let n = reader.read_line(&mut line).await?;
            if n == 0 {
                break;
            }
            total += n;
            if total > MAX_FABRIC_BODY_BYTES {
                return Err(FabricClientError::Http("ndjson body exceeds cap".into()));
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                continue;
            }
            on_line(trimmed)?;
        }
        Ok::<u16, FabricClientError>(status)
    })
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            tracing::warn!(ip = %ip, port, path, initiator, "fabric read timed out");
            return Err(FabricClientError::Timeout {
                phase: "read",
                path: path.to_string(),
            });
        }
    };

    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_http_response_with_content_length() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        let parsed = parse_http_response(raw, 1024).unwrap();
        assert_eq!(parsed.status, 200);
        assert_eq!(parsed.body, b"hello");
    }

    #[test]
    fn parse_http_response_rejects_oversized_length() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 999999\r\n\r\n";
        assert!(parse_http_response(raw, 1024).is_err());
    }

    #[test]
    fn parse_http_response_rejects_truncated_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nhi";
        assert!(parse_http_response(raw, 1024).is_err());
    }

    #[test]
    fn parse_status_line_ok() {
        assert_eq!(
            parse_status_line("HTTP/1.1 503 Service Unavailable").unwrap(),
            503
        );
    }
}
