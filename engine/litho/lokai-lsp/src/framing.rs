//! Sync LSP `Content-Length` framing (same wire format as `lokai-rpc`).

use std::io::{BufRead, Read, Write};

use thiserror::Error;

/// Maximum JSON-RPC frame body size — matches `lokai-rpc` (SEC-005).
pub const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;

pub const MAX_HEADER_BYTES: usize = 8 * 1024;

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("frame headers exceed maximum {MAX_HEADER_BYTES}")]
    HeaderTooLarge,
    #[error("invalid or duplicate Content-Length header")]
    InvalidLength,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("framed message missing Content-Length header")]
    MissingLength,
    #[error("frame Content-Length {0} exceeds maximum {MAX_FRAME_BYTES}")]
    Oversized(usize),
    #[error("invalid utf-8 in frame body")]
    Utf8,
}

pub fn frame(bytes: &[u8]) -> Vec<u8> {
    let header = format!("Content-Length: {}\r\n\r\n", bytes.len());
    let mut out = Vec::with_capacity(header.len() + bytes.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(bytes);
    out
}

pub fn write_frame<W: Write>(w: &mut W, bytes: &[u8]) -> Result<(), FrameError> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(FrameError::Oversized(bytes.len()));
    }
    w.write_all(&frame(bytes))?;
    w.flush()?;
    Ok(())
}

pub fn read_frame<R: BufRead>(r: &mut R) -> Result<Option<Vec<u8>>, FrameError> {
    let mut content_length: Option<usize> = None;
    let mut header_bytes = 0;
    loop {
        let mut line = Vec::new();
        let remaining = MAX_HEADER_BYTES - header_bytes;
        let n = r.take(remaining as u64 + 1).read_until(b'\n', &mut line)?;
        if n > remaining {
            return Err(FrameError::HeaderTooLarge);
        }
        if n == 0 && header_bytes == 0 {
            return Ok(None);
        }
        if n == 0 || !line.ends_with(b"\n") {
            return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into());
        }
        header_bytes += n;
        let line = std::str::from_utf8(&line).map_err(|_| FrameError::InvalidLength)?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, rest)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("Content-Length") {
                if content_length.is_some() {
                    return Err(FrameError::InvalidLength);
                }
                let value = rest.trim();
                if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(FrameError::InvalidLength);
                }
                content_length = Some(value.parse().map_err(|_| FrameError::InvalidLength)?);
            }
        }
    }
    let len = content_length.ok_or(FrameError::MissingLength)?;
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::Oversized(len));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(Some(buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trip_frame() {
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let mut r = Cursor::new(frame(body));
        let got = read_frame(&mut r).unwrap().unwrap();
        assert_eq!(got, body);
    }

    #[test]
    fn rejects_oversized_content_length() {
        let raw = format!("Content-Length: {}\r\n\r\n", MAX_FRAME_BYTES + 1);
        let mut r = Cursor::new(raw.into_bytes());
        assert!(matches!(
            read_frame(&mut r).unwrap_err(),
            FrameError::Oversized(n) if n == MAX_FRAME_BYTES + 1
        ));
    }

    #[test]
    fn header_limits_cover_unterminated_lines_and_many_small_lines() {
        for data in [
            vec![b'x'; MAX_HEADER_BYTES * 2],
            b"X: x\r\n".repeat(MAX_HEADER_BYTES),
        ] {
            let mut reader = Cursor::new(data);
            assert!(matches!(
                read_frame(&mut reader),
                Err(FrameError::HeaderTooLarge)
            ));
            assert!(reader.position() <= MAX_HEADER_BYTES as u64 + 1);
        }
    }

    #[test]
    fn rejects_ambiguous_lengths_and_truncated_frames() {
        for data in [
            "Content-Length: 1\r\ncontent-length: 2\r\n\r\nx",
            "Content-Length: invalid\r\n\r\n",
            "Content-Length: +1\r\n\r\nx",
            "Content-Length: 2\r\n\r\nx",
            "Content-Length: 2",
            "Content-Length: 2\r\n",
        ] {
            assert!(read_frame(&mut Cursor::new(data)).is_err(), "{data:?}");
        }
        assert!(read_frame(&mut Cursor::new(b"")).unwrap().is_none());
        assert_eq!(
            read_frame(&mut Cursor::new(b"content-length: 1\r\n\r\nx"))
                .unwrap()
                .unwrap(),
            b"x"
        );
    }
}
