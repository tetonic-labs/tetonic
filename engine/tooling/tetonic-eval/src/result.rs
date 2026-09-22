use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationResult {
    pub scenario_id: String,
    pub run_status: RunStatus,
    pub timing: TimingBreakdown,
    pub resource_usage: ResourceUsage,

    // Artifacts & Output
    pub patch_digest: Option<String>,
    pub output_digest: Option<String>,
    pub trace_correlation_id: String,

    // Statistical Summaries
    pub statistics: Option<StatisticalSummary>,

    // Test outcome
    pub passed: bool,
    pub failure_classification: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RunStatus {
    Completed,
    Incomplete,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingBreakdown {
    pub wall_clock_duration_ms: u64,
    pub model_inference_duration_ms: u64,
    pub tool_execution_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUsage {
    pub tool_call_count: u32,
    pub model_call_count: u32,
    pub token_usage_prompt: u32,
    pub token_usage_completion: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatisticalSummary {
    /// Ordered individual trial evidence; index + 1 is the trial number.
    /// Empty for legacy reports that did not retain auditable trial records.
    #[serde(default)]
    pub trials: Vec<EvaluationResult>,
    pub sample_count: u32,
    pub pass_rate: f64,
    pub mean_duration_ms: f64,
    pub median_duration_ms: f64,
    pub standard_deviation_ms: f64,
    pub confidence_interval_95_lower: f64,
    pub confidence_interval_95_upper: f64,
    pub baseline_comparison: Option<String>,
    pub is_sample_size_sufficient: bool,
}
