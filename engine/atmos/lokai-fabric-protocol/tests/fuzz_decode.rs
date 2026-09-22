//! Decoder fuzz smoke tests (M5-1) — random bytes must not panic.

use lokai_fabric_protocol::{decode_envelope_json_lossy, default_message_limits, FabricErrorCode};

#[test]
fn decoder_fuzzing_smoke() {
    let limits = default_message_limits();
    for seed in 0u64..512 {
        let mut bytes = Vec::with_capacity(64);
        let mut x = seed;
        for _ in 0..64 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
            bytes.push((x >> 33) as u8);
        }
        match decode_envelope_json_lossy(&bytes, Some(&limits)) {
            Ok(_) => {}
            Err(e) => {
                assert!(
                    matches!(
                        e.code,
                        FabricErrorCode::InvalidEnvelope
                            | FabricErrorCode::InputTooLarge
                            | FabricErrorCode::UnsupportedProtocolVersion
                    ),
                    "unexpected error {:?}",
                    e.code
                );
            }
        }
    }
}

#[test]
fn decoder_fuzzing_valid_envelope_subset() {
    let limits = default_message_limits();
    let valid = br#"{"protocol_version":1,"message_id":"m","message_type":"JobOffer","coordinator_id":"c","worker_id":"w","sent_at":"2026-01-01T00:00:00Z","revocation_epoch":1,"trace_context":{"trace_id":"t","span_id":"s"},"payload":{}}"#;
    for end in 10..valid.len() {
        let slice = &valid[..end];
        let _ = decode_envelope_json_lossy(slice, Some(&limits));
    }
}
