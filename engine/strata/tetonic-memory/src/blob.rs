//! zstd file-change blobs (SEC2-E2-027 bounded).

use crate::{Result, StoreError};

/// zstd compression level for file-change blobs (small + fast).
pub(crate) const ZSTD_LEVEL: i32 = 3;
/// Max uncompressed text stored per file-change side (SEC2-E2-027).
pub(crate) const MAX_BLOB_INPUT_BYTES: usize = 4 * 1024 * 1024;
/// Max bytes emitted when decompressing a stored blob (SEC2-E2-027).
pub(crate) const MAX_BLOB_DECOMPRESSED_BYTES: usize = 16 * 1024 * 1024;

type ChangeTuple = (
    i64,
    String,
    String,
    String,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
);

pub(crate) fn decode_blob(blob: Option<Vec<u8>>) -> Result<Option<String>> {
    match blob {
        Some(bytes) => {
            if bytes.len() > MAX_BLOB_INPUT_BYTES {
                return Err(StoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("zstd blob exceeds max compressed size ({MAX_BLOB_INPUT_BYTES} bytes)"),
                )));
            }
            let raw = decode_zstd_limited(&bytes, MAX_BLOB_DECOMPRESSED_BYTES)?;
            String::from_utf8(raw).map(Some).map_err(|e| {
                StoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("file change blob is not valid UTF-8: {e}"),
                ))
            })
        }
        None => Ok(None),
    }
}

fn decode_zstd_limited(bytes: &[u8], max_out: usize) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut decoder = zstd::stream::read::Decoder::new(bytes)?;
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = decoder.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if out.len() + n > max_out {
            return Err(StoreError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("zstd blob exceeds max decompressed size ({max_out} bytes)"),
            )));
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok(out)
}

pub(crate) fn encode_blob(text: &str) -> Result<Vec<u8>> {
    if text.len() > MAX_BLOB_INPUT_BYTES {
        return Err(StoreError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("file change content exceeds max input size ({MAX_BLOB_INPUT_BYTES} bytes)"),
        )));
    }
    Ok(zstd::encode_all(text.as_bytes(), ZSTD_LEVEL)?)
}

pub(crate) fn decode_change_tuples(rows: Vec<ChangeTuple>) -> Result<Vec<crate::FileChangeRow>> {
    let mut out = Vec::with_capacity(rows.len());
    for (id, tool_call_id, path, change_kind, before, after) in rows {
        out.push(crate::FileChangeRow {
            id,
            tool_call_id,
            path,
            change_kind,
            before: decode_blob(before)?,
            after: decode_blob(after)?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_decompress_over_limit() {
        let payload = "Z".repeat(MAX_BLOB_DECOMPRESSED_BYTES + 1);
        let compressed = zstd::encode_all(payload.as_bytes(), ZSTD_LEVEL).unwrap();
        let err = decode_blob(Some(compressed)).unwrap_err();
        assert!(err.to_string().contains("max decompressed size"));
    }

    #[test]
    fn rejects_invalid_utf8_after_decompress() {
        let mut raw = vec![0xFF, 0xFE];
        raw.extend_from_slice(b"hello");
        let compressed = zstd::encode_all(raw.as_slice(), ZSTD_LEVEL).unwrap();
        let err = decode_blob(Some(compressed)).unwrap_err();
        assert!(err.to_string().contains("UTF-8"));
    }
}
