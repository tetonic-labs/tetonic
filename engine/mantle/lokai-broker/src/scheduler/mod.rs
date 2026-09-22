//! Deterministic weighted scheduler (M6-2 Option A).

pub mod attempt_lease;
pub mod calibration;
pub mod circuit;
pub mod estimate;
pub mod failover;
pub mod fallback;
pub mod hop_placement;
pub mod infer_schedule;
pub mod persist;
pub mod score;
pub mod speculate;
pub mod stamp;
pub mod types;

pub use calibration::PredictionCalibration;
pub use circuit::{CircuitBreakerRegistry, CircuitState};
pub use estimate::{estimate_finish_ms, CandidateInputs, CompletionEstimate, UncertaintyModel};
pub use failover::PendingWorkerLossContinue;
pub use fallback::{
    classify_inference_error, fallback_action, labels_to_targets, next_fallback,
    parse_target_label, FallbackAction, FallbackFailureClass,
};
pub use hop_placement::{revalidate_hop_placement, HopPlacementOutcome};
pub use infer_schedule::{schedule_infer_chat, ScheduleInferChat};
pub use persist::{InMemorySchedulerStore, MemorySchedulerStore, SchedulerDecisionStore};
pub use score::{decide, DecideInput, SchedulerConfig, SchedulerMode, SCHEDULER_MODEL_VERSION};
pub use speculate::{
    should_speculate_for_tail, speculation_allowed, speculative_race_sessions,
    SpeculationDenyReason,
};
pub use stamp::{
    apply_execution_target, apply_scheduler_decision, stamp_fabric_preferred,
    stamp_fabric_single_target,
};
pub use types::{
    CandidateEstimate, ExecutionTargetId, SchedulerDecision, SchedulerDecisionId, SchedulerReason,
};
