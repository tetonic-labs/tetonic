//! Durable run supervisor (M3-1 / M3-2).

pub mod acceptance;
pub mod dag;
pub mod idempotency;
pub mod identity;
pub mod infer_admission;
pub mod lease;
pub mod metrics;
pub mod migration;
pub mod payload_digest;
pub mod quotas;
pub mod recovery;
pub mod replay;
pub mod retry;
pub mod service;
pub mod side_effect;
pub mod transition;

pub use acceptance::{apply_winner_selection, try_accept_completion, try_claim_finalization};
pub use dag::{
    apply_dependency_failure, dependencies_satisfied, recompute_blocked_ready, task_is_executable,
    task_is_locked, topo_sort_tasks, would_create_cycle,
};
pub use idempotency::{binding_input_digest, job_input_digest, task_idempotency_digest};
pub use identity::{get_identity, put_identity, IdentityError};
pub use infer_admission::{
    add_hop_task, cancel_hop, create_hop_attempt, ensure_hop_run, fail_hop,
    hop_job_spec_must_be_none, hop_run_classified, hop_task_leaseable, lease_hop, start_hop,
};
pub use lease::{is_lease_current, validate_lease_proof};
pub use recovery::{detect_recovery_required, recover_expired_leases};
pub use replay::{empty_snapshot, replay_from_events};
pub use service::{command_envelope, DurableRunSupervisor, RunEventHook, RunSupervisor};
pub use transition::{apply_command, event_type_for, failure_class_for_command};

pub mod managed;
pub use managed::{
    AdmitJob, DispatchId, DispatchTicket, ExecutionPolicy, FinalizationEffectDriver,
    FinalizationPolicy, FinalizeJob, ManagedBinding, ManagedRunError, ManagedRunHooks,
    ManagedRunService, StartIdentityJobCommand, StartIdentityJobResult,
};
