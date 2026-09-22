use serde::{Deserialize, Serialize};

use crate::{bounds::IMMUTABLE_SECURITY_FEATURES, FabricError, FabricErrorCode, MessageSizeLimits};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionNegotiationRequest {
    pub min_supported_version: u32,
    pub max_supported_version: u32,
    pub required_features: Vec<String>,
    pub optional_features: Vec<String>,
    pub software_version: String,
    pub message_size_limits: MessageSizeLimits,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionNegotiationResponse {
    pub negotiated_version: u32,
    pub active_features: Vec<String>,
    pub message_size_limits: MessageSizeLimits,
}

pub fn negotiate_versions(
    request: &VersionNegotiationRequest,
    local_min: u32,
    local_max: u32,
) -> Result<VersionNegotiationResponse, FabricError> {
    if request.max_supported_version < local_min || request.min_supported_version > local_max {
        return Err(FabricError {
            code: FabricErrorCode::UnsupportedProtocolVersion,
            message: "no compatible protocol version".into(),
            details: None,
        });
    }
    let negotiated = request.min_supported_version.max(local_min).min(local_max);
    let mut active: Vec<String> = request
        .required_features
        .iter()
        .chain(request.optional_features.iter())
        .cloned()
        .collect();
    active.sort_unstable();
    active.dedup();
    for required in IMMUTABLE_SECURITY_FEATURES {
        if !active.iter().any(|f| f == required) {
            return Err(FabricError {
                code: FabricErrorCode::InvalidEnvelope,
                message: format!("negotiation cannot omit security feature {required}"),
                details: None,
            });
        }
    }
    Ok(VersionNegotiationResponse {
        negotiated_version: negotiated,
        active_features: active,
        message_size_limits: request.message_size_limits.clone(),
    })
}
