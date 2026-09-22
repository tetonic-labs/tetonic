//! Stable domain value objects and execution-contract types (Architecture Consolidation II).

pub mod artifact;
pub mod canonical;
pub mod classify;
pub mod code_index;
pub mod context_compiler;
pub mod dispatch;
pub mod execution;
pub mod failure;
pub mod idempotency;
pub mod identity;
pub mod ids;
pub mod invocation;
pub mod key_storage;
pub mod lsp_session;
pub mod placement;
pub mod policy;
pub mod result_integrity;
pub mod run;
pub mod secrets;
pub mod sinks;
pub mod tool_host;
pub mod trust;
pub mod workspace;

pub use canonical::{
    compute_canonical_digest, finalize_parameters, prepare_proposed_action,
    validate_authorized_action, CANONICAL_SCHEMA_VERSION,
};
pub use classify::{
    combine_data_classes, data_class_sensitivity_rank, Classification, ClassificationSource,
    ClassificationSummary, DataClass, DisclosureTier, PolicyVersion, CLASSIFICATION_POLICY_VERSION,
};
pub use code_index::{
    CodeDefinition, CodeIndex, CodeIndexOpen, CodeIndexStatus, CodeOutlineRow, CodeSearchHit,
    TextSkeleton,
};
pub use context_compiler::{
    CompiledContext, CompiledEvidence, ContextCompileRequest, ContextCompiler,
    ContextExpansionRequest,
};
pub use dispatch::{
    DispatchDecision, DispatchDenied, DispatchDestination, DispatchGuard, DispatchRequest,
};
pub use execution::{
    ActionKind, ActionPolicyOutcome, ApprovalRequirement, AuthorizedAction, CapabilityScope,
    ExecutionOutcome, ExecutionTargetId, IssuedCapability, ProposedAction,
};
pub use failure::RetryPolicy;
pub use failure::{
    AttemptLease, CancellationRecord, FailureClass, HeartbeatReport, LeaseProof, ResourceUsage,
    RunDeadlines, SideEffectCommitRecord, SpeculationConfig, TaskRetryState, TimeoutKind,
    VerificationRemediation,
};
pub use idempotency::{AttemptDeliveryKey, TaskIdempotencyKey};
pub use identity::{AgentAttemptExecutor, AgentIdentity, AgentJobSpec, AttemptExecutionContext};
pub use ids::{
    ActionId, AgentId, ApprovalId, ArtifactId, AttemptId, CapabilityId, CoordinatorId, EvidenceId,
    ExecutionId, ExpansionHandleId, IdentityId, JobId, KeyId, LeaseId, ReservationId, ResultId,
    RunId, SessionId, TaskId, TransactionId, TurnId, WorkerId, WorkspaceVersion,
};
pub use invocation::{
    AgentInvocation, CandidateOutcome, CompletionKind, LimitKind, LoopDiscipline,
    LoopDisciplineLimits, LoopNotes,
};
pub use lsp_session::{LspSession, LspSessionOpen};
pub use placement::{
    EligibleTarget, PlacementDecision as TrustPlacementDecision, PlacementExplanation,
    PlacementJobKind, PlacementReason, PlacementRequest, ProjectPlacementPolicy,
    RedactionPlanReference, SandboxRequirements, VerificationPolicyReference,
    VerificationRequirement,
};
pub use policy::PolicyDecision;
pub use result_integrity::{
    ArtifactOrigin, ResultDisposition, ResultVerificationRequirement, WorkerBehaviorSignals,
    WorkerOperationalState,
};
pub use run::{
    AcceptArtifact, AddDependency, AddTask, ArtifactRef, AttemptRecord, AttemptState, CancelRun,
    CancelTask, ClaimFinalization, CommandEnvelope, CompleteAttempt, CreateAttempt, CreateRun,
    DependencyPolicy, EventActor, EventType, ExpireLease, FailAttempt, FinishRun, LeaseAttempt,
    MarkTaskReady, RecordHeartbeat, RecordSideEffectCommit, RejectArtifact, ReplayGap,
    ReplayGapReason, ResourceRequirements, RunCommand, RunCommandResult, RunEventEnvelope,
    RunFinishOutcome, RunSnapshot, RunState, RunSupervisorError, StartAttempt, StartRun,
    StorageLimits, TaskDependency, TaskInputBinding, TaskRecord, TaskState, TraceContext,
    VerificationPolicy, VersionedEventPayload,
};
pub use secrets::{
    OutboundRedaction, OutboundRedactionSink, RedactionRecordReference, ScanOutcome, SecretScanner,
};
pub use sinks::{CapabilityConsumer, CapabilityError, MutationSink, PolicyEvaluator, ProcessSink};
pub use tool_host::{
    ChangeKind, FileChange, ToolAdvertisement, ToolHost, ToolOutcome, ToolProposal,
};
pub use trust::WorkerTrust;
pub use workspace::{
    ActualState, CommitHash, CommitResult, ConflictKind, ContentDigest, ExpectedState,
    PatchApproval, RepositoryId, StagedOperation, StagedOperationKind, TransactionArtifact,
    TransactionPreview, TransactionState, VerificationRecord, WorkspaceBinding, WorkspaceConflict,
    WorkspacePath, WorkspaceVersionScheme,
};

pub mod work_scope;
