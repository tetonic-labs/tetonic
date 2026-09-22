//! Canonical digest for persisted run event payloads (M6, INV-RUN-003).
//!
//! This lives beside the store rather than in `lokai-run` so that the writer,
//! the verifier, and the reader all hash through one function. A second
//! implementation would let the digest be computed over a different encoding
//! than the one persisted, which is the failure this module exists to prevent.
//!
//! Scope: this detects corruption and a digest that never matched its payload.
//! It is **not** tamper-evidence — `payload_json` and `payload_digest` are
//! adjacent plaintext columns, so an actor able to write the database can
//! rewrite both consistently.

use lokai_domain::workspace::ContentDigest;
use sha2::{Digest, Sha256};

/// Digest of the exact bytes stored in `run_events.payload_json`.
pub fn digest_payload_json(payload_json: &str) -> ContentDigest {
    let hash = Sha256::digest(payload_json.as_bytes());
    ContentDigest::new(format!("sha256:{:x}", hash))
}

/// Digest of `payload` under the encoding the store persists.
///
/// Returns `Err` rather than hashing a substitute: a digest that does not
/// describe the payload is worse than no digest, because the guard downstream
/// cannot tell the difference.
pub fn digest_event_payload(
    payload: &serde_json::Value,
) -> Result<ContentDigest, serde_json::Error> {
    Ok(digest_payload_json(&serde_json::to_string(payload)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_matches_the_stored_encoding() {
        // D-2's exactness premise: the digest must be over the same bytes the
        // store writes into payload_json. Asserted rather than assumed.
        let payload = serde_json::json!({"cmd": "a", "n": 1});
        let stored = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            digest_event_payload(&payload).unwrap().0,
            digest_payload_json(&stored).0
        );
    }

    #[test]
    fn digest_is_stable_and_distinguishes_payloads() {
        let a = serde_json::json!({"cmd": "a"});
        let b = serde_json::json!({"cmd": "b"});
        assert_eq!(
            digest_event_payload(&a).unwrap().0,
            digest_event_payload(&a).unwrap().0
        );
        assert_ne!(
            digest_event_payload(&a).unwrap().0,
            digest_event_payload(&b).unwrap().0
        );
        assert!(digest_event_payload(&a).unwrap().0.starts_with("sha256:"));
    }

    #[test]
    fn empty_payload_does_not_collide_with_the_old_fallback() {
        // The pre-M6 fallback hashed the empty byte string, so any
        // serialization failure produced sha256:e3b0c442… . Nothing this
        // function can return may equal that value by accident.
        const EMPTY_BYTES_SHA256: &str =
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        for payload in [
            serde_json::json!(null),
            serde_json::json!(""),
            serde_json::json!({}),
        ] {
            assert_ne!(
                digest_event_payload(&payload).unwrap().0,
                EMPTY_BYTES_SHA256
            );
        }
    }
}
