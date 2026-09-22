//! Bounded envelope decoding (M5-1).

use serde::de::DeserializeOwned;

use crate::{
    bounds::{check_envelope_size, check_payload_size, default_message_limits},
    validate::{validate_mandatory_envelope_fields, validate_protocol_version},
    FabricEnvelope, FabricError, FabricErrorCode, MessageSizeLimits,
};

pub fn decode_envelope_json<T: DeserializeOwned>(
    bytes: &[u8],
    limits: &MessageSizeLimits,
) -> Result<FabricEnvelope<T>, FabricError> {
    check_envelope_size(bytes.len(), limits)?;
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| FabricError {
        code: FabricErrorCode::InvalidEnvelope,
        message: format!("decode failed: {e}"),
        details: None,
    })?;
    if let Some(version) = v.get("protocol_version").and_then(|x| x.as_u64()) {
        validate_protocol_version(&crate::ProtocolVersion(version as u32))?;
    } else {
        return Err(FabricError {
            code: FabricErrorCode::InvalidEnvelope,
            message: "missing protocol_version".into(),
            details: None,
        });
    }
    if let Some(payload) = v.get("payload") {
        let payload_len = serde_json::to_vec(payload)
            .map(|b| b.len())
            .unwrap_or(usize::MAX);
        check_payload_size(payload_len, limits)?;
    }
    let envelope: FabricEnvelope<T> = serde_json::from_value(v).map_err(|e| FabricError {
        code: FabricErrorCode::InvalidEnvelope,
        message: format!("decode failed: {e}"),
        details: None,
    })?;
    validate_mandatory_envelope_fields(&envelope)?;
    Ok(envelope)
}

pub fn decode_envelope_json_lossy(
    bytes: &[u8],
    limits: Option<&MessageSizeLimits>,
) -> Result<serde_json::Value, FabricError> {
    let default_limits = default_message_limits();
    let limits = limits.unwrap_or(&default_limits);
    check_envelope_size(bytes.len(), limits)?;
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| FabricError {
        code: FabricErrorCode::InvalidEnvelope,
        message: "invalid json envelope".into(),
        details: None,
    })?;
    if let Some(version) = v.get("protocol_version").and_then(|x| x.as_u64()) {
        validate_protocol_version(&crate::ProtocolVersion(version as u32))?;
    } else {
        return Err(FabricError {
            code: FabricErrorCode::InvalidEnvelope,
            message: "missing protocol_version".into(),
            details: None,
        });
    }
    Ok(v)
}
