use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EvaluationMode {
    Deterministic,
    Statistical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationManifest {
    pub scenario_id: String,
    pub scenario_version: String,
    pub mode: EvaluationMode,

    // Repository Context
    pub repository_snapshot_id: Option<String>,
    pub git_commit: Option<String>,
    pub is_dirty_fixture: bool,
    pub source_and_license_metadata: Option<String>,
    pub setup_procedure: Option<String>,
    pub cleanup_procedure: Option<String>,

    // Task & Prompts
    pub task_prompt: String,
    pub task_prompt_digest: String,
    pub system_prompt_version: String,
    pub system_prompt_digest: String,
    pub tool_schema_version: String,
    pub tool_schema_digest: String,

    // Model & Runtime Config
    pub model_name: String,
    pub model_digest: Option<String>,
    pub quantization: Option<String>,
    pub tokenizer_digest: Option<String>,
    pub sampling_parameters: SamplingParameters,

    // Feature Flags & Environment
    pub feature_flags: Vec<String>,
    pub orchestration_mode: String,
    pub limits: EvaluationLimits,
    pub environment: EnvironmentMetadata,

    // Evaluation
    pub expected_behavioral_outcome: Option<String>,
    pub expected_pass_threshold: Option<f64>,
    pub security_classification: Option<String>,
    pub expected_redactions: Option<Vec<String>>,
    pub required_capabilities: Option<Vec<String>>,
    pub known_nondeterministic_elements: Option<Vec<String>>,
    pub patch_correctness: Option<PatchCorrectness>,
    pub graders: Vec<GraderType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchCorrectness {
    pub files_may_change: Option<Vec<String>>,
    pub files_must_not_change: Option<Vec<String>>,
    pub required_functional_behavior: Option<String>,
    pub required_tests: Option<String>,
    pub formatting_constraints: Option<String>,
    pub prohibited_shortcuts: Option<Vec<String>>,
    pub expected_handling_pre_existing_failures: Option<String>,
    pub partial_credit_allowed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum GraderType {
    ExactMatch {
        expected_patch_digest: String,
    },
    TestExecution {
        command: String,
        require_success: bool,
        fail_if_skipped: bool,
        expected_pass_count: Option<u32>,
    },
    /// An exit-status check, explicitly not evidence that tests ran.
    CommandExecution {
        command: String,
    },
    /// Run submitted tests against pinned original code and a trusted mutant in
    /// separate temporary workspaces. Only declared files are copied.
    MutationTest {
        command: String,
        input_files: Vec<String>,
        source_path: String,
        mutant_source: String,
    },
    /// Trusted manifest pins for grading scripts/configuration. Checked before
    /// and after command graders; values are `sha256:` plus 64 lowercase hex digits.
    ProtectedFiles {
        sha256: std::collections::BTreeMap<String, String>,
    },
    FileBoundary {
        /// Literal workspace-relative files; no suffix/glob matching.
        allowed_paths: Vec<String>,
        /// Component-aligned suffixes, so a basename also denies nested copies.
        prohibited_paths: Vec<String>,
    },
    SecurityCheck {
        require_no_secrets: bool,
    },
    ManualReview {
        rubric: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingParameters {
    pub temperature: f64,
    pub seed: Option<u64>,
    pub context_window_size: usize,
    pub context_reserve: usize,
    pub compaction_settings: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationLimits {
    pub max_turns: u32,
    pub max_attempts: u32,
    pub max_tokens: u32,
    pub max_wall_clock_duration_secs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentMetadata {
    pub os: String,
    pub cpu: String,
    pub ram_gb: u32,
    pub gpu: Option<String>,
    pub vram_gb: Option<u32>,
    pub lokai_commit: String,
    pub build_profile: String,
}

impl EvaluationManifest {
    /// Validates that the manifest parameters are strictly deterministic if the mode is Deterministic.
    pub fn validate_deterministic(&self) -> Result<(), anyhow::Error> {
        if self.mode == EvaluationMode::Deterministic {
            if self.sampling_parameters.temperature > 0.0 && self.sampling_parameters.seed.is_none()
            {
                return Err(anyhow::anyhow!(
                    "Deterministic mode requires temperature=0.0 or a fixed seed."
                ));
            }
            if self.model_digest.is_none() {
                return Err(anyhow::anyhow!(
                    "Deterministic mode requires an explicit immutable model_digest."
                ));
            }
        }
        Ok(())
    }
}
