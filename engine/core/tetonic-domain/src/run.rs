//! Durable run, task, and attempt model (M3-1 / M3-2).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::classify::DataClass;
use crate::execution::ExecutionTargetId;
use crate::failure::RetryPolicy;
use crate::failure::{
    AttemptLease, CancellationRecord, FailureClass, HeartbeatReport, LeaseProof, RunDeadlines,
    SideEffectCommitRecord, SpeculationConfig, TaskRetryState, TimeoutKind,
    VerificationRemediation,
};
use crate::identity::AgentJobSpec;
use crate::ids::{
    AttemptId, CommandId, EventId, LeaseId, RunId, SessionId, TaskId, TraceId, TransactionId,
    WorkspaceVersion,
};
use crate::workspace::ContentDigest;

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Created,
    Active,
    Canceling,
    Canceled,
    Failed,
    Succeeded,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Created,
    Blocked,
    Ready,
    Leased,
    Running,
    Succeeded,
    Failed,
    Canceled,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptState {
    Created,
    Leased,
    Starting,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    LeaseExpired,
    Canceled,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyPolicy {
    RequireSuccess,
    AllowPartial,
    ContinueOnFailure,
    FallbackTask(TaskId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDependency {
    pub depends_on: TaskId,
    pub policy: DependencyPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub artifact_id: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResourceRequirements {
    pub cpu_millis: Option<u64>,
    pub memory_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VerificationPolicy {
    pub required: bool,
    pub command: Option<String>,
    #[serde(default)]
    pub remediation: VerificationRemediation,
}

/// Durable attribution for governed execution. These identifiers are not grants;
/// current authority must be checked by the trusted execution host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionScope {
    pub principal_id: String,
    pub organization_id: String,
    pub information_context_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskInputBinding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_scope: Option<ExecutionScope>,
    /// Locator for the selected immutable grant, never authority by itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_grant_id: Option<String>,
    /// Per-task managed job truth. Omitted for legacy snapshots and Infer tasks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_spec: Option<AgentJobSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_role: Option<String>,
    pub task_definition_version: u64,
    pub workspace_version: Option<WorkspaceVersion>,
    pub input_artifacts: Vec<ArtifactRef>,
    pub dependencies: Vec<TaskDependency>,
    pub data_class: DataClass,
    pub required_capabilities: Vec<String>,
    pub resource_requirements: ResourceRequirements,
    pub deadline: Option<u64>,
    pub retry_policy: RetryPolicy,
    pub verification_policy: VerificationPolicy,
}

impl Default for TaskInputBinding {
    fn default() -> Self {
        Self {
            execution_scope: None,
            execution_grant_id: None,
            job_spec: None,
            job_role: None,
            task_definition_version: 1,
            workspace_version: None,
            input_artifacts: Vec::new(),
            dependencies: Vec::new(),
            data_class: DataClass::RepositorySource,
            required_capabilities: Vec::new(),
            resource_requirements: ResourceRequirements::default(),
            deadline: None,
            retry_policy: RetryPolicy::default(),
            verification_policy: VerificationPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    pub run_id: Option<RunId>,
    pub task_id: Option<TaskId>,
    pub attempt_id: Option<AttemptId>,
    /// ComputeBroker / scheduler decision correlation (M6-3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler_decision_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEnvelope {
    pub command_id: String,
    pub expected_sequence: Option<u64>,
    pub trace: TraceContext,
    pub actor: String,
    pub timestamp: u64,
    pub workspace_version: Option<WorkspaceVersion>,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRun {
    pub envelope: CommandEnvelope,
    #[serde(default)]
    pub session_id: Option<SessionId>,
    pub run_id: RunId,
    pub root_task_id: TaskId,
    pub root_binding: TaskInputBinding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speculation: Option<SpeculationConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_spec: Option<AgentJobSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartRun {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddTask {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub binding: TaskInputBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddDependency {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub depends_on: TaskId,
    pub policy: DependencyPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkTaskReady {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub task_id: TaskId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAttempt {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub attempt_id: AttemptId,
    /// M3-2: duplicate dispatch returns the same attempt.
    pub delivery_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseAttempt {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub lease_id: LeaseId,
    pub lease_epoch: u64,
    pub holder: ExecutionTargetId,
    pub issued_at: u64,
    pub expires_at: u64,
    pub heartbeat_interval_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartAttempt {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub lease_proof: LeaseProof,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordHeartbeat {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub lease_proof: LeaseProof,
    pub heartbeat_sequence: u64,
    pub report: HeartbeatReport,
    pub renewed_expires_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompleteAttempt {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub task_version: u64,
    pub workspace_version: Option<WorkspaceVersion>,
    pub input_digest: String,
    pub result_digest: String,
    pub lease_proof: LeaseProof,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimFinalization {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub task_id: TaskId,
    pub task_version: u64,
    pub input_digest: String,
    pub lease_proof: LeaseProof,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailAttempt {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub failure_class: FailureClass,
    pub reason: String,
    pub lease_proof: Option<LeaseProof>,
    pub timeout_kind: Option<TimeoutKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpireLease {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub expired_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordSideEffectCommit {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub operation_key: String,
    pub transaction_id: Option<TransactionId>,
    pub committed_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelTask {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub task_id: TaskId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelRun {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptArtifact {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub attempt_id: AttemptId,
    pub artifact: ArtifactRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectArtifact {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub attempt_id: AttemptId,
    pub artifact_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinishRun {
    pub envelope: CommandEnvelope,
    pub run_id: RunId,
    pub outcome: RunFinishOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunFinishOutcome {
    Succeeded,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum RunCommand {
    CreateRun(CreateRun),
    StartRun(StartRun),
    AddTask(AddTask),
    AddDependency(AddDependency),
    MarkTaskReady(MarkTaskReady),
    CreateAttempt(CreateAttempt),
    LeaseAttempt(LeaseAttempt),
    StartAttempt(StartAttempt),
    /// Durable, non-idempotent permission to enter the executor once.
    ClaimExecution(StartAttempt),
    RecordHeartbeat(RecordHeartbeat),
    ClaimFinalization(ClaimFinalization),
    CompleteAttempt(CompleteAttempt),
    FailAttempt(FailAttempt),
    CancelTask(CancelTask),
    CancelRun(CancelRun),
    AcceptArtifact(AcceptArtifact),
    RejectArtifact(RejectArtifact),
    RecordSideEffectCommit(RecordSideEffectCommit),
    ExpireLease(ExpireLease),
    FinishRun(FinishRun),
}

impl RunCommand {
    pub fn envelope(&self) -> &CommandEnvelope {
        match self {
            RunCommand::CreateRun(c) => &c.envelope,
            RunCommand::StartRun(c) => &c.envelope,
            RunCommand::AddTask(c) => &c.envelope,
            RunCommand::AddDependency(c) => &c.envelope,
            RunCommand::MarkTaskReady(c) => &c.envelope,
            RunCommand::CreateAttempt(c) => &c.envelope,
            RunCommand::LeaseAttempt(c) => &c.envelope,
            RunCommand::StartAttempt(c) => &c.envelope,
            RunCommand::ClaimExecution(c) => &c.envelope,
            RunCommand::RecordHeartbeat(c) => &c.envelope,
            RunCommand::ClaimFinalization(c) => &c.envelope,
            RunCommand::CompleteAttempt(c) => &c.envelope,
            RunCommand::FailAttempt(c) => &c.envelope,
            RunCommand::CancelTask(c) => &c.envelope,
            RunCommand::CancelRun(c) => &c.envelope,
            RunCommand::AcceptArtifact(c) => &c.envelope,
            RunCommand::RejectArtifact(c) => &c.envelope,
            RunCommand::RecordSideEffectCommit(c) => &c.envelope,
            RunCommand::ExpireLease(c) => &c.envelope,
            RunCommand::FinishRun(c) => &c.envelope,
        }
    }

    pub fn run_id(&self) -> Option<&RunId> {
        match self {
            RunCommand::CreateRun(c) => Some(&c.run_id),
            RunCommand::StartRun(c) => Some(&c.run_id),
            RunCommand::AddTask(c) => Some(&c.run_id),
            RunCommand::AddDependency(c) => Some(&c.run_id),
            RunCommand::MarkTaskReady(c) => Some(&c.run_id),
            RunCommand::CreateAttempt(c) => Some(&c.run_id),
            RunCommand::LeaseAttempt(c) => Some(&c.run_id),
            RunCommand::StartAttempt(c) => Some(&c.run_id),
            RunCommand::ClaimExecution(c) => Some(&c.run_id),
            RunCommand::RecordHeartbeat(c) => Some(&c.run_id),
            RunCommand::ClaimFinalization(c) => Some(&c.run_id),
            RunCommand::CompleteAttempt(c) => Some(&c.run_id),
            RunCommand::FailAttempt(c) => Some(&c.run_id),
            RunCommand::CancelTask(c) => Some(&c.run_id),
            RunCommand::CancelRun(c) => Some(&c.run_id),
            RunCommand::AcceptArtifact(c) => Some(&c.run_id),
            RunCommand::RejectArtifact(c) => Some(&c.run_id),
            RunCommand::RecordSideEffectCommit(c) => Some(&c.run_id),
            RunCommand::ExpireLease(c) => Some(&c.run_id),
            RunCommand::FinishRun(c) => Some(&c.run_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRecord {
    pub task_id: TaskId,
    pub state: TaskState,
    pub binding: TaskInputBinding,
    pub accepted_artifact: Option<ArtifactRef>,
    pub active_attempt: Option<AttemptId>,
    #[serde(default)]
    pub winning_attempt: Option<AttemptId>,
    #[serde(default)]
    pub finalization_claim: Option<AttemptId>,
    #[serde(default)]
    pub completed_version: Option<u64>,
    #[serde(default)]
    pub retry: TaskRetryState,
    #[serde(default)]
    pub side_effect_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptRecord {
    #[serde(default, skip_serializing_if = "is_false")]
    pub execution_claimed: bool,
    pub attempt_id: AttemptId,
    pub task_id: TaskId,
    pub state: AttemptState,
    pub task_version: u64,
    pub workspace_version: Option<WorkspaceVersion>,
    pub input_digest: String,
    pub result_digest: Option<String>,
    pub delivery_key: Option<String>,
    pub lease: Option<AttemptLease>,
    pub failure_class: Option<FailureClass>,
    pub failure_reason: Option<String>,
    #[serde(default)]
    pub attempt_number: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    RunCreated,
    TaskAdded,
    DependencyAdded,
    TaskTransitioned,
    AttemptLeased,
    AttemptCompleted,
    CancellationRequested,
    ArtifactAccepted,
    ApprovalRequested,
    ApprovalResolved,
    WorkspaceTransactionCommitted,
    // Additional event types can be added as needed
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventActor {
    pub name: String,
}

pub type Timestamp = chrono::DateTime<chrono::Utc>;
pub type VersionedEventPayload = serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunEventEnvelope {
    pub event_id: EventId,
    pub run_id: RunId,
    pub sequence: u64,
    pub event_type: EventType,
    pub schema_version: u32,
    pub command_id: Option<CommandId>,
    pub causation_id: Option<EventId>,
    pub correlation_id: Option<TraceId>,
    pub actor: EventActor,
    pub occurred_at: Timestamp,
    pub recorded_at: Timestamp,
    pub data_class: DataClass,
    pub payload_digest: ContentDigest,
    pub payload: VersionedEventPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayGapReason {
    OlderThanFloor,
    Corruption,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayGap {
    pub requested_after: u64,
    pub earliest_available: u64,
    pub snapshot_sequence: Option<u64>,
    pub reason: ReplayGapReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageLimits {
    pub global_quota_bytes: u64,
    pub project_quota_bytes: u64,
    pub run_quota_bytes: u64,
    pub artifact_quota_bytes: u64,
    pub trace_quota_bytes: u64,
    pub reserved_recovery_bytes: u64,
}

impl Default for StorageLimits {
    fn default() -> Self {
        Self {
            global_quota_bytes: 10 * 1024 * 1024 * 1024, // 10 GB
            project_quota_bytes: 5 * 1024 * 1024 * 1024, // 5 GB
            run_quota_bytes: 500 * 1024 * 1024,          // 500 MB
            artifact_quota_bytes: 1024 * 1024 * 1024,    // 1 GB
            trace_quota_bytes: 500 * 1024 * 1024,        // 500 MB
            reserved_recovery_bytes: 50 * 1024 * 1024,   // 50 MB
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSnapshot {
    pub run_id: RunId,
    #[serde(default)]
    pub session_id: Option<SessionId>,
    pub state: RunState,
    pub sequence: u64,
    pub workspace_version: Option<WorkspaceVersion>,
    pub tasks: BTreeMap<TaskId, TaskRecord>,
    pub attempts: BTreeMap<AttemptId, AttemptRecord>,
    pub dependencies: BTreeMap<TaskId, Vec<TaskDependency>>,
    pub events: Vec<RunEventEnvelope>,
    #[serde(default)]
    pub delivery_index: BTreeMap<String, AttemptId>,
    #[serde(default)]
    pub side_effect_commits: BTreeMap<String, SideEffectCommitRecord>,
    #[serde(default)]
    pub deadlines: RunDeadlines,
    #[serde(default)]
    pub cancellation: CancellationRecord,
    #[serde(default)]
    pub speculation: SpeculationConfig,
    #[serde(default)]
    pub next_lease_epoch: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_spec: Option<AgentJobSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunCommandResult {
    pub run_id: RunId,
    pub sequence: u64,
    pub idempotent_replay: bool,
    pub snapshot: RunSnapshot,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum RunSupervisorError {
    #[error("run not found: {0}")]
    RunNotFound(String),
    #[error("task not found: {0}")]
    TaskNotFound(String),
    #[error("attempt not found: {0}")]
    AttemptNotFound(String),
    #[error("stale sequence: expected {expected}, actual {actual}")]
    StaleSequence { expected: u64, actual: u64 },
    #[error("invalid transition: {0}")]
    InvalidTransition(String),
    #[error("cycle detected in task graph")]
    CycleDetected,
    #[error("dependency change after lease")]
    DependencyLocked,
    #[error("run not accepting commands: {0:?}")]
    RunNotAccepting(RunState),
    #[error("recovery required")]
    RecoveryRequired,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("stale lease epoch: expected {expected}, actual {actual}")]
    StaleLeaseEpoch { expected: u64, actual: u64 },
    #[error("stale result: {0}")]
    StaleResult(String),
    #[error("duplicate delivery: {0}")]
    DuplicateDelivery(String),
    #[error("retry limit exceeded")]
    RetryLimitExceeded,
    #[error("side effect already committed: {0}")]
    SideEffectAlreadyCommitted(String),
    #[error("deadline exceeded: {0:?}")]
    DeadlineExceeded(TimeoutKind),
    #[error("persistence error: {0}")]
    Persistence(String),
    #[error("corruption detected: {0}")]
    Corruption(String),
    #[error("storage limit exceeded: {0}")]
    StorageLimitExceeded(String),
}
