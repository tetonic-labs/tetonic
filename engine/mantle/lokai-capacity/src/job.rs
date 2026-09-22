//! Capacity optimize job types.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OptimizeDepth {
    Quick,
    Full,
}

impl OptimizeDepth {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "full" => Self::Full,
            _ => Self::Quick,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizeProgress {
    pub job_id: String,
    pub phase: String,
    pub percent: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizeOptions {
    pub depth: OptimizeDepth,
    pub auto_apply: bool,
    #[serde(default)]
    pub base_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizeOutcome {
    pub job_id: String,
    pub state: JobState,
    pub profile_ids: Vec<String>,
    pub applied_profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub const BENCH_MODEL_TAG: &str = "lokai-bench-opt";
