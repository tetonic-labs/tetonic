//! ComputeBroker and hierarchical admission control (M6-1).

pub mod adapters;
pub mod admission;
pub mod broker;
pub mod budget;
pub mod chat_request;
pub mod dispatch;
pub mod job_profile;
pub mod metrics;
pub mod persist;
pub mod priority;
pub mod queue;
pub mod scheduler;
pub mod types;

pub use adapters::{
    redact_outbound, BrokerGatedProcessBroker, BrokerInferenceProvider, InferenceTargetAdapter,
    LocalProcessTargetAdapter,
};
pub use admission::{
    AdmissionController, AdmissionDecision, AdmissionRejection, AdmissionRejectionReason,
    AdmissionRequest, HierarchicalAdmissionController,
};
pub use broker::{ComputeBroker, DefaultComputeBroker};
pub use budget::{
    BudgetLimits, BudgetReject, DurationEstimate, GpuRequirement, HierarchicalBudgetLedger,
    ReservationState, ReservationTarget, ReservedResources, ResourceAmount, ResourceRequest,
    ResourceReservation,
};
pub use job_profile::{profile_for, JobKindProfile};
pub use metrics::{AdmissionMetrics, SchedulerMetrics};
pub use persist::{InMemoryReservationStore, MemoryReservationStore, ReservationStore};
pub use priority::{ComputePriority, FairnessPolicy};
pub use queue::{QueueAdmission, QueueLimits, QueueManager};
pub use scheduler::{
    classify_inference_error, decide, fallback_action, labels_to_targets, next_fallback,
    parse_target_label, revalidate_hop_placement, schedule_infer_chat, should_speculate_for_tail,
    speculation_allowed, speculative_race_sessions, CandidateEstimate, CandidateInputs,
    CircuitBreakerRegistry, CircuitState, CompletionEstimate, DecideInput, ExecutionTargetId,
    FallbackAction, FallbackFailureClass, HopPlacementOutcome, InMemorySchedulerStore,
    MemorySchedulerStore, PredictionCalibration, ScheduleInferChat, SchedulerConfig,
    SchedulerDecision, SchedulerDecisionId, SchedulerDecisionStore, SchedulerMode, SchedulerReason,
    SpeculationDenyReason, UncertaintyModel, SCHEDULER_MODEL_VERSION,
};
pub use types::{
    CancellationReason, ComputeBrokerError, ComputeHandle, ComputeRequest, ComputeStatus,
    DeadlinePolicy, PlacementDecisionReference, RetryPolicyReference,
};
