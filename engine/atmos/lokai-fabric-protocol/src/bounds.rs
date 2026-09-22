//! Protocol message size limits (M5-1).

use serde::{Deserialize, Serialize};

use crate::{FabricError, FabricErrorCode};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSizeLimits {
    pub max_envelope_bytes: u32,
    pub max_payload_bytes: u32,
    pub max_error_message_bytes: u32,
}

/// Current fabric protocol version (v1).
pub const PROTOCOL_VERSION: u32 = 1;
pub const MIN_SUPPORTED_VERSION: u32 = 1;
pub const MAX_SUPPORTED_VERSION: u32 = 1;

/// Security-critical features that negotiation must never remove.
pub const IMMUTABLE_SECURITY_FEATURES: &[&str] = &[
    "task_identity",
    "lease_epoch",
    "input_digest",
    "revocation_epoch",
];

pub fn default_message_limits() -> MessageSizeLimits {
    MessageSizeLimits {
        max_envelope_bytes: 4 * 1024 * 1024,
        max_payload_bytes: 2 * 1024 * 1024,
        max_error_message_bytes: 4096,
    }
}

pub fn check_envelope_size(len: usize, limits: &MessageSizeLimits) -> Result<(), FabricError> {
    if len as u32 > limits.max_envelope_bytes {
        return Err(FabricError {
            code: FabricErrorCode::InputTooLarge,
            message: format!(
                "envelope {} bytes exceeds max {}",
                len, limits.max_envelope_bytes
            ),
            details: None,
        });
    }
    Ok(())
}

pub fn check_payload_size(len: usize, limits: &MessageSizeLimits) -> Result<(), FabricError> {
    if len as u32 > limits.max_payload_bytes {
        return Err(FabricError {
            code: FabricErrorCode::InputTooLarge,
            message: format!(
                "payload {} bytes exceeds max {}",
                len, limits.max_payload_bytes
            ),
            details: None,
        });
    }
    Ok(())
}

pub fn bound_error_message(msg: &str, limits: &MessageSizeLimits) -> String {
    let max = limits.max_error_message_bytes as usize;
    if msg.len() <= max {
        msg.to_string()
    } else {
        msg.chars().take(max).collect()
    }
}
