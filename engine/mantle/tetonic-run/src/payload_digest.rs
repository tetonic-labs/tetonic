//! Content digests for persisted run event payloads (R04 / M3-4, M6).
//!
//! The implementation lives in `tetonic_memory::payload_digest` so that the
//! writer here and the verifier in the store cannot drift apart; this module is
//! the `lokai-run` entry point to it.

use tetonic_domain::workspace::ContentDigest;

/// Digest of the JSON bytes that are persisted as `payload_json`.
///
/// Fallible by design (M6): the pre-M6 version hashed the empty byte string
/// when serialization failed, which stored a well-formed digest describing
/// nothing. A caller that cannot digest its payload must not commit it.
pub fn digest_event_payload(
    payload: &serde_json::Value,
) -> Result<ContentDigest, serde_json::Error> {
    tetonic_memory::payload_digest::digest_event_payload(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn different_payloads_differ() {
        let a = digest_event_payload(&serde_json::json!({"cmd": "a"})).unwrap();
        let b = digest_event_payload(&serde_json::json!({"cmd": "b"})).unwrap();
        assert!(!a.0.is_empty());
        assert!(!b.0.is_empty());
        assert_ne!(a.0, b.0);
        assert!(a.0.starts_with("sha256:"));
    }

    #[test]
    fn identical_payloads_match() {
        let p = serde_json::json!({"x": 1});
        assert_eq!(
            digest_event_payload(&p).unwrap().0,
            digest_event_payload(&p).unwrap().0
        );
    }

    #[test]
    fn digest_is_unchanged_by_the_move_to_lokai_memory() {
        // Pre-M6 this hashed serde_json::to_vec(payload); it now hashes
        // to_string(payload).as_bytes(). Those are the same bytes, and this
        // pins that so existing persisted rows stay verifiable.
        let payload = serde_json::json!({"cmd": "x", "n": 7});
        let expected = {
            use sha2::{Digest, Sha256};
            let bytes = serde_json::to_vec(&payload).unwrap();
            format!("sha256:{:x}", Sha256::digest(&bytes))
        };
        assert_eq!(digest_event_payload(&payload).unwrap().0, expected);
    }
}
