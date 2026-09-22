//! Shared index utilities.

use chrono::Utc;

pub(crate) fn now() -> String {
    Utc::now().to_rfc3339()
}

/// Deterministic FNV-1a 64-bit content hash (hex) — drives incrementality.
pub(crate) fn content_hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

pub(crate) fn cap_chars(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        s.chars().take(n).collect()
    } else {
        s.to_string()
    }
}
