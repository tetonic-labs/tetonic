//! Shared daemon helpers (RPC parse and serialization).

use lokai_app::{CapacityDoctorStatus, CapacityStatus, DiagnosisCode};
use lokai_rpc::protocol::{CapacitySummary, ErrorCode, RpcError};
use serde::de::DeserializeOwned;
use serde_json::Value;

pub fn parse<T: DeserializeOwned>(params: Value) -> Result<T, RpcError> {
    serde_json::from_value(params)
        .map_err(|e| RpcError::new(ErrorCode::InvalidParams, e.to_string()))
}

pub fn to_value<T: serde::Serialize>(v: T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

pub fn to_capacity_summary(status: CapacityStatus) -> CapacitySummary {
    CapacitySummary {
        completed: status.completed,
        stale: status.stale,
        doctor: doctor_status_str(status.doctor),
        active_profile_id: status.active_profile_id,
        active_profile_label: status.active_profile_label,
        hardware_summary: status.hardware_summary,
        gates_ok: status.gates_ok,
        last_setup_at: status.last_setup_at.map(|t| t.to_rfc3339()),
    }
}

pub fn doctor_status_str(d: CapacityDoctorStatus) -> String {
    match d {
        CapacityDoctorStatus::Healthy => "healthy".into(),
        CapacityDoctorStatus::Degraded => "degraded".into(),
        CapacityDoctorStatus::Unknown => "unknown".into(),
        CapacityDoctorStatus::NoProfile => "no_profile".into(),
    }
}

pub fn diagnosis_code_str(c: DiagnosisCode) -> String {
    match c {
        DiagnosisCode::NoProfile => "no_profile".into(),
        DiagnosisCode::FingerprintMismatch => "fingerprint_mismatch".into(),
        DiagnosisCode::DegradedPlacement => "degraded_placement".into(),
        DiagnosisCode::SlowRawShort => "slow_raw_short".into(),
        DiagnosisCode::ResidentModelMismatch => "resident_model_mismatch".into(),
        DiagnosisCode::GatesFailed => "gates_failed".into(),
    }
}
