//! Runtime profile types — immutable capacity bindings.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TierRole {
    Coder,
    Fast,
    Hard,
    Embed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProfileSource {
    Setup,
    Reoptimize,
    Manual,
    Import,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum GpuRole {
    Compute,
    Display,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GpuInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bus_id: Option<String>,
    pub vram_total_mb: u32,
    #[serde(default)]
    pub role: GpuRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HardwareSnapshot {
    pub fingerprint: String,
    pub gpus: Vec<GpuInfo>,
    pub cpu_cores: u32,
    pub ram_mb: u64,
    pub os: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ollama_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EnvHint {
    pub name: String,
    pub value: String,
    /// v1: always false in persisted profiles — document only.
    #[serde(default)]
    pub apply: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InferenceRecipe {
    pub base_model: String,
    pub estate_model: String,
    pub num_ctx: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_gpu: Option<u32>,
    #[serde(default = "default_keep_alive")]
    pub keep_alive: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modelfile_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_hints: Vec<EnvHint>,
    /// Configured speculative draft model (OPT-501).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_model: Option<String>,
    /// Number of speculative tokens to generate per forward pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_count: Option<u32>,
}

fn default_keep_alive() -> String {
    "30m".to_string()
}

/// What we measured on the box when the profile was validated — doctor compares live state to this.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ObservedPlacement {
    /// GPU residency [0, 100] from this runner's `/api/ps` `size_vram`/`size`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_processor_pct: Option<f32>,
    /// Display-only Ollama processor column when present; never parsed for gates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processor_split: Option<String>,
    /// This runner's allocation in MiB, not whole-device usage.
    pub vram_used_mb: u32,
    pub resident_model: String,
    pub load_wall_s: f64,
    pub measured_at: DateTime<Utc>,
}

/// Authoritative GPU share for gates and scoring — structured field only.
pub fn observed_gpu_pct(observed: &ObservedPlacement) -> Option<f32> {
    observed.gpu_processor_pct
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProfileMetrics {
    pub raw_short_wall_s: f64,
    #[serde(default)]
    pub raw_short_tps: f64,
    #[serde(default)]
    pub tool_payload_prefill_tps: f64,
    #[serde(default)]
    pub raw_long_prefill_tps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RuntimeProfile {
    pub schema_version: u32,
    pub id: String,
    pub label: String,
    pub created_at: DateTime<Utc>,
    pub node_id: String,
    pub role: TierRole,
    pub source: ProfileSource,
    pub hardware: HardwareSnapshot,
    pub recipe: InferenceRecipe,
    pub observed: ObservedPlacement,
    pub metrics: ProfileMetrics,
    pub gates_passed: bool,
}

impl RuntimeProfile {
    pub fn validate_schema(&self) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "unsupported schema_version {} (want {SCHEMA_VERSION})",
                self.schema_version
            ));
        }
        if self.id.is_empty() {
            return Err("profile id required".into());
        }
        if self.recipe.estate_model.is_empty() {
            return Err("recipe.estate_model required".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_gpu_pct_reads_structured_field() {
        let obs = ObservedPlacement {
            gpu_processor_pct: Some(85.0),
            processor_split: Some("15%/85%".into()),
            vram_used_mb: 18_000,
            resident_model: "m".into(),
            load_wall_s: 0.0,
            measured_at: Utc::now(),
        };
        assert!((observed_gpu_pct(&obs).unwrap() - 85.0).abs() < f32::EPSILON);
    }

    #[test]
    fn fixture_round_trip() {
        let raw = include_str!("../../../bench/fixtures/profile_floor_p40.example.json");
        let p: RuntimeProfile = serde_json::from_str(raw).expect("fixture parses");
        p.validate_schema().expect("valid");
        let again = serde_json::to_string(&p).unwrap();
        let p2: RuntimeProfile = serde_json::from_str(&again).unwrap();
        assert_eq!(p.id, p2.id);
        assert_eq!(p.recipe.estate_model, p2.recipe.estate_model);
    }
}
