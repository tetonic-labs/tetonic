//! Capacity plane — runtime profiles, bench reports, and gate policy.
//!
//! Immutable **RuntimeProfile** documents describe how inference should run on
//! a node. The daemon loads active bindings and resolves session defaults;
//! doctor compares live Ollama placement to saved `observed` snapshots.
//!
//! Contract: `docs/implementation/contracts/capacity-profile-v1.md`

mod applier;
mod bench;
mod client;
mod defaults;
mod detect;
mod doctor;
mod gates;
mod job;
mod microbench;
mod model_fit;
mod optimizer;
mod paths;
mod probe;
mod profile;
mod report;
mod service;
mod status;
mod store;

pub use applier::{apply_recipe, estate_model_name, render_modelfile};
pub use bench::{BenchReport, BenchSuite, SuiteResult};
pub use client::{ClientError, InferenceClient, OllamaInferenceClient};
pub use defaults::{load_inference_defaults, InferenceDefaults, LOCAL_NODE_ID};
pub use detect::{detect_hardware, hardware_summary};
pub use doctor::{diagnose, is_fingerprint_stale};
pub use gates::{evaluate_gates, GateFailure, GatePolicy, GateSeverity, GateVerdict};
pub use job::{
    JobState, OptimizeDepth, OptimizeOptions, OptimizeOutcome, OptimizeProgress, BENCH_MODEL_TAG,
};
pub use microbench::{run_raw_short, RAW_SHORT_PROMPT};
pub use model_fit::{canonical_model_stem, same_model_family, session_matches_profile};
pub use optimizer::{run_optimize, OptimizeError};
pub use paths::{lokai_data_dir, modelfiles_dir, reports_dir};
pub use probe::{gpu_residency_pct_from_ps, parse_ps_response};
pub use profile::{
    observed_gpu_pct, EnvHint, GpuInfo, GpuRole, HardwareSnapshot, InferenceRecipe,
    ObservedPlacement, ProfileMetrics, ProfileSource, RuntimeProfile, TierRole, SCHEMA_VERSION,
};
pub use report::format_capacity_report;
pub use service::{
    admission_status_from_store, build_capacity_status, capacity_status_for_store,
    enrich_local_node_capacity, status_for_node, status_for_node_id,
};
pub use status::{
    doctor_status_str, CapacityDiagnosis, CapacityDoctorStatus, CapacityStatus, DiagnosisCode,
};
pub use store::{parse_tier_role, tier_role_str, ProfileStore, ProfileStoreError, ProfileSummary};
mod worker;

pub use worker::{
    capacity_status_blocking, capacity_status_for_worker_path, node_capacity_from_wire,
    status_for_worker, worker_capacity_wire, WorkerCapacityWire, WorkerProfileStore,
};
