//! Minimal HTTP/1.1 POST parser for the enrollment listener.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const ENROLL_PATH: &str = "/v1/enroll/complete";
const MAX_REQUEST_BYTES: usize = 64 * 1024;

pub enum ParseResult {
    Complete(serde_json::Value),
    Incomplete,
    Invalid,
}

/// Parse a POST to `/v1/enroll/complete` when the full request is available.
pub fn parse_http_post_json(raw: &[u8]) -> ParseResult {
    if raw.len() > MAX_REQUEST_BYTES {
        return ParseResult::Invalid;
    }
    let Some(header_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return if raw.len() >= MAX_REQUEST_BYTES {
            ParseResult::Invalid
        } else {
            ParseResult::Incomplete
        };
    };
    let headers = match std::str::from_utf8(&raw[..header_end]) {
        Ok(s) => s,
        Err(_) => return ParseResult::Invalid,
    };
    let mut lines = headers.split("\r\n");
    let Some(request_line) = lines.next() else {
        return ParseResult::Invalid;
    };
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(path), Some(_version)) = (parts.next(), parts.next(), parts.next())
    else {
        return ParseResult::Invalid;
    };
    if method != "POST" || path != ENROLL_PATH {
        return ParseResult::Invalid;
    }
    let content_length = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("Content-Length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .next();
    let Some(content_length) = content_length else {
        return ParseResult::Invalid;
    };
    if content_length > MAX_REQUEST_BYTES {
        return ParseResult::Invalid;
    }
    let body_start = header_end + 4;
    let body_end = body_start + content_length;
    if raw.len() < body_end {
        return ParseResult::Incomplete;
    }
    let body = match std::str::from_utf8(&raw[body_start..body_end]) {
        Ok(s) => s,
        Err(_) => return ParseResult::Invalid,
    };
    match serde_json::from_str(body) {
        Ok(v) => ParseResult::Complete(v),
        Err(_) => ParseResult::Invalid,
    }
}

/// Read until a complete enrollment POST is received or the connection fails.
pub async fn read_http_post_json<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Option<serde_json::Value> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = match reader.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return None,
        };
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > MAX_REQUEST_BYTES {
            return None;
        }
        match parse_http_post_json(&buf) {
            ParseResult::Complete(v) => return Some(v),
            ParseResult::Incomplete => continue,
            ParseResult::Invalid => return None,
        }
    }
    match parse_http_post_json(&buf) {
        ParseResult::Complete(v) => Some(v),
        _ => None,
    }
}

pub async fn write_http_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    status: u16,
    body: &str,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        409 => "Conflict",
        _ => "Error",
    };
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    writer.write_all(resp.as_bytes()).await?;
    writer.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_post() {
        let raw = b"POST /v1/enroll/complete HTTP/1.1\r\nHost: x\r\nContent-Length: 2\r\n\r\n{}";
        assert!(matches!(
            parse_http_post_json(raw),
            ParseResult::Complete(_)
        ));
    }

    #[test]
    fn rejects_wrong_path() {
        let raw = b"POST /v1/other HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}";
        assert!(matches!(parse_http_post_json(raw), ParseResult::Invalid));
    }

    #[test]
    fn rejects_missing_content_length() {
        let raw = b"POST /v1/enroll/complete HTTP/1.1\r\nHost: x\r\n\r\n{}";
        assert!(matches!(parse_http_post_json(raw), ParseResult::Invalid));
    }

    #[test]
    fn incomplete_until_body_arrives() {
        let partial = b"POST /v1/enroll/complete HTTP/1.1\r\nContent-Length: 2\r\n\r\n";
        assert!(matches!(
            parse_http_post_json(partial),
            ParseResult::Incomplete
        ));
        let mut full = partial.to_vec();
        full.extend_from_slice(b"{}");
        assert!(matches!(
            parse_http_post_json(&full),
            ParseResult::Complete(_)
        ));
    }

    #[test]
    fn rejects_truncated_body() {
        let raw = b"POST /v1/enroll/complete HTTP/1.1\r\nContent-Length: 10\r\n\r\n{}";
        assert!(matches!(parse_http_post_json(raw), ParseResult::Incomplete));
    }
}
