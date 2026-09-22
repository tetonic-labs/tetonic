//! Capacity status and doctor diagnosis (RPC + CLI).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CapacityDoctorStatus {
    Healthy,
    Degraded,
    Unknown,
    NoProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityStatus {
    pub completed: bool,
    pub stale: bool,
    pub doctor: CapacityDoctorStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware_summary: Option<String>,
    pub gates_ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_setup_at: Option<DateTime<Utc>>,
    /// Estate / applied tag from the active profile (`qwen3.6-estate`), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_model: Option<String>,
    /// Base Ollama tag the profile was built from (`qwen3.6:latest`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_base_model: Option<String>,
}

impl Default for CapacityStatus {
    fn default() -> Self {
        Self {
            completed: false,
            stale: false,
            doctor: CapacityDoctorStatus::NoProfile,
            active_profile_id: None,
            active_profile_label: None,
            hardware_summary: None,
            gates_ok: false,
            last_setup_at: None,
            profile_model: None,
            profile_base_model: None,
        }
    }
}

pub fn doctor_status_str(status: CapacityDoctorStatus) -> &'static str {
    match status {
        CapacityDoctorStatus::Healthy => "healthy",
        CapacityDoctorStatus::Degraded => "degraded",
        CapacityDoctorStatus::Unknown => "unknown",
        CapacityDoctorStatus::NoProfile => "no_profile",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosisCode {
    NoProfile,
    FingerprintMismatch,
    DegradedPlacement,
    SlowRawShort,
    ResidentModelMismatch,
    GatesFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CapacityDiagnosis {
    pub status: CapacityDoctorStatus,
    pub codes: Vec<DiagnosisCode>,
    pub summary: String,
    #[serde(default)]
    pub recommendations: Vec<String>,
}
