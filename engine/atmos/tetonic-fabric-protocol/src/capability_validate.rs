//! Worker capability document validation (M5-2).

use chrono::{DateTime, Utc};

use crate::{
    capability_document::{WorkerCapabilities, MAX_CAPABILITY_DOCUMENT_BYTES},
    FabricError, FabricErrorCode,
};

pub fn validate_capability_document_bytes(bytes: &[u8]) -> Result<(), FabricError> {
    if bytes.len() as u32 > MAX_CAPABILITY_DOCUMENT_BYTES {
        return Err(FabricError {
            code: FabricErrorCode::InputTooLarge,
            message: format!(
                "capability document {} bytes exceeds max {MAX_CAPABILITY_DOCUMENT_BYTES}",
                bytes.len()
            ),
            details: None,
        });
    }
    Ok(())
}

pub fn validate_worker_capabilities(
    caps: &WorkerCapabilities,
    channel_worker_id: &tetonic_domain::ids::WorkerId,
    known_revocation_epoch: u64,
    now: DateTime<Utc>,
) -> Result<(), FabricError> {
    validate_capability_document_fields(caps, channel_worker_id, known_revocation_epoch, now)
}

pub fn validate_capability_document_fields(
    caps: &WorkerCapabilities,
    channel_worker_id: &tetonic_domain::ids::WorkerId,
    known_revocation_epoch: u64,
    now: DateTime<Utc>,
) -> Result<(), FabricError> {
    if caps.worker_id != *channel_worker_id {
        return Err(FabricError {
            code: FabricErrorCode::IdentityMismatch,
            message: "capability worker_id does not match authenticated channel".into(),
            details: None,
        });
    }
    if caps.boot_id.is_empty() {
        return Err(FabricError {
            code: FabricErrorCode::InvalidEnvelope,
            message: "capability boot_id is required".into(),
            details: None,
        });
    }
    if caps.capability_revision == 0 {
        return Err(FabricError {
            code: FabricErrorCode::InvalidEnvelope,
            message: "capability_revision must be positive".into(),
            details: None,
        });
    }
    if caps.valid_until <= caps.generated_at {
        return Err(FabricError {
            code: FabricErrorCode::InvalidEnvelope,
            message: "valid_until must be after generated_at".into(),
            details: None,
        });
    }
    if caps.is_expired(now) {
        return Err(FabricError {
            code: FabricErrorCode::InvalidEnvelope,
            message: "capability advertisement expired".into(),
            details: None,
        });
    }
    if caps.revocation_epoch < known_revocation_epoch {
        return Err(FabricError {
            code: FabricErrorCode::StaleRevocationEpoch,
            message: "capability revocation_epoch is stale".into(),
            details: None,
        });
    }
    for model in &caps.model_inventory {
        if model.local_name.is_empty() {
            return Err(FabricError {
                code: FabricErrorCode::InvalidEnvelope,
                message: "model local_name must not be empty".into(),
                details: None,
            });
        }
    }
    validate_capability_document_content(caps)?;
    Ok(())
}

/// Reject documents that may contain secrets or local file paths.
pub fn validate_capability_document_content(caps: &WorkerCapabilities) -> Result<(), FabricError> {
    scan_field("boot_id", &caps.boot_id)?;
    scan_field("software_version", &caps.software_version.version)?;
    if let Some(build) = &caps.software_version.build {
        scan_field("software_build", build)?;
    }
    for model in &caps.model_inventory {
        scan_field("model.local_name", &model.local_name)?;
        if let Some(d) = &model.model_digest {
            scan_field("model.model_digest", d)?;
        }
        if let Some(q) = &model.quantization {
            scan_field("model.quantization", q)?;
        }
    }
    for gpu in &caps.hardware.gpus {
        scan_field("gpu.name", &gpu.name)?;
    }
    for ev in &caps.evidence {
        scan_field("evidence.claim", &ev.claim)?;
    }
    Ok(())
}

fn scan_field(field: &str, value: &str) -> Result<(), FabricError> {
    let lower = value.to_ascii_lowercase();
    for needle in FORBIDDEN_SECRET_MARKERS {
        if lower.contains(needle) {
            return Err(FabricError {
                code: FabricErrorCode::InvalidEnvelope,
                message: format!("capability field {field} contains forbidden secret marker"),
                details: None,
            });
        }
    }
    for marker in FORBIDDEN_PATH_MARKERS {
        if value.contains(marker) {
            return Err(FabricError {
                code: FabricErrorCode::InvalidEnvelope,
                message: format!("capability field {field} contains local path"),
                details: None,
            });
        }
    }
    Ok(())
}

const FORBIDDEN_SECRET_MARKERS: &[&str] = &[
    "-----begin",
    "api_key",
    "apikey",
    "secret=",
    "password=",
    "bearer ",
    "sk-",
];

const FORBIDDEN_PATH_MARKERS: &[&str] =
    &["\\users\\", "/home/", "/etc/", "c:\\", "file://", "\\\\"];

/// Scheduling may use expired dynamic capacity only after refresh; static doc must be valid.
pub fn scheduling_eligible(
    caps: &WorkerCapabilities,
    now: DateTime<Utc>,
    require_fresh_dynamic: bool,
) -> bool {
    if caps.is_expired(now) || caps.runtime_capacity.draining {
        return false;
    }
    if require_fresh_dynamic && caps.dynamic_capacity_expired(now) {
        return false;
    }
    true
}
