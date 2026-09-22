//! LSP-style message framing: `Content-Length: N\r\n\r\n` + N bytes of UTF-8
//! JSON. Chosen over newline-delimited JSON because messages stream, are large,
//! and contain multi-line code/diffs (the editor's stack already speaks this
//! framing for LSP).

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

/// Maximum JSON-RPC frame body size (SEC-005). 32 MiB covers large diffs while
/// blocking allocation-based DoS on the stdio pipe.
pub const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;

/// Read one framed message body from `r`. Returns `Ok(None)` on a clean EOF
/// (peer closed the pipe) so the caller can shut down gracefully. Unknown
/// headers are ignored; only `Content-Length` is required.
pub async fn read_frame<R: AsyncRead + Unpin>(
    r: &mut BufReader<R>,
) -> std::io::Result<Option<Vec<u8>>> {
    let mut content_length: Option<usize> = None;
    let mut header_bytes = 0usize;
    loop {
        let mut line = String::new();
        let n = r
            .take((8193 - header_bytes) as u64)
            .read_line(&mut line)
            .await?;
        header_bytes += n;
        if header_bytes > 8192 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "RPC headers exceed 8192 bytes",
            ));
        }
        if n == 0 {
            // EOF. If we were mid-headers that's a truncated frame, but a clean
            // EOF before any header is the normal shutdown path.
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break; // blank line terminates the header block
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse().ok();
        }
        // Other headers (e.g. Content-Type) are accepted and ignored.
    }

    let len = content_length.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "framed message missing Content-Length header",
        )
    })?;
    if len > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("frame Content-Length {len} exceeds maximum {MAX_FRAME_BYTES}"),
        ));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(Some(buf))
}

/// Wrap `bytes` in a `Content-Length` frame.
pub fn frame(bytes: &[u8]) -> Vec<u8> {
    let header = format!("Content-Length: {}\r\n\r\n", bytes.len());
    let mut out = Vec::with_capacity(header.len() + bytes.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(bytes);
    out
}

/// Write one framed message to `w` and flush.
pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, bytes: &[u8]) -> std::io::Result<()> {
    w.write_all(&frame(bytes)).await?;
    w.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn round_trips_a_single_frame() {
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let framed = frame(body);
        let mut r = BufReader::new(&framed[..]);
        let got = read_frame(&mut r).await.unwrap().unwrap();
        assert_eq!(got, body);
    }

    #[tokio::test]
    async fn reads_back_to_back_frames() {
        let a = br#"{"a":1}"#;
        let b = br#"{"b":2}"#;
        let mut stream = frame(a);
        stream.extend_from_slice(&frame(b));
        let mut r = BufReader::new(&stream[..]);
        assert_eq!(read_frame(&mut r).await.unwrap().unwrap(), a);
        assert_eq!(read_frame(&mut r).await.unwrap().unwrap(), b);
        assert!(read_frame(&mut r).await.unwrap().is_none()); // clean EOF
    }

    #[tokio::test]
    async fn tolerates_extra_headers_and_crlf() {
        let body = br#"{"ok":true}"#;
        let raw = format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        let mut stream = raw.into_bytes();
        stream.extend_from_slice(body);
        let mut r = BufReader::new(&stream[..]);
        assert_eq!(read_frame(&mut r).await.unwrap().unwrap(), body);
    }

    #[tokio::test]
    async fn clean_eof_returns_none() {
        let empty: &[u8] = b"";
        let mut r = BufReader::new(empty);
        assert!(read_frame(&mut r).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn rejects_oversized_content_length() {
        let raw = format!("Content-Length: {}\r\n\r\n", MAX_FRAME_BYTES + 1);
        let mut r = BufReader::new(raw.as_bytes());
        let err = read_frame(&mut r).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("exceeds maximum"));
    }
}
