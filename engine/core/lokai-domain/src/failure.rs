//! Distributed failure semantics (M3-2).

use serde::{Deserialize, Serialize};

use crate::execution::ExecutionTargetId;
use crate::ids::{AttemptId, LeaseId, TransactionId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    TransientTransport,
    WorkerUnavailable,
    LeaseExpired,
    TimedOut,
    ResourceExhausted,
    InvalidInput,
    PolicyDenied,
    VerificationFailed,
    SandboxViolation,
    Canceled,
    PermanentExecutionFailure,
    InternalInvariantViolation,
}

impl FailureClass {
    pub fn is_retryable_by_default(&self) -> bool {
        matches!(
            self,
            FailureClass::TransientTransport
                | FailureClass::WorkerUnavailable
                | FailureClass::TimedOut
                | FailureClass::ResourceExhausted
        )
    }

    pub fn is_nonretryable_by_default(&self) -> bool {
        matches!(
            self,
            FailureClass::InvalidInput
                | FailureClass::PolicyDenied
                | FailureClass::InternalInvariantViolation
                | FailureClass::SandboxViolation
                | FailureClass::PermanentExecutionFailure
                | FailureClass::Canceled
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub retryable_classes: Vec<FailureClass>,
    pub initial_delay_ms: u64,
    pub backoff_factor: f64,
    pub max_delay_ms: u64,
    pub jitter: bool,
    pub require_different_target: bool,
    pub refresh_workspace: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            retryable_classes: vec![
                FailureClass::TransientTransport,
                FailureClass::WorkerUnavailable,
                FailureClass::LeaseExpired,
                FailureClass::TimedOut,
            ],
            initial_delay_ms: 1_000,
            backoff_factor: 2.0,
            max_delay_ms: 60_000,
            jitter: true,
            require_different_target: false,
            refresh_workspace: false,
        }
    }
}

impl Eq for RetryPolicy {}

impl PartialEq for RetryPolicy {
    fn eq(&self, other: &Self) -> bool {
        self.max_attempts == other.max_attempts
            && self.retryable_classes == other.retryable_classes
            && self.initial_delay_ms == other.initial_delay_ms
            && self.backoff_factor.to_bits() == other.backoff_factor.to_bits()
            && self.max_delay_ms == other.max_delay_ms
            && self.jitter == other.jitter
            && self.require_different_target == other.require_different_target
            && self.refresh_workspace == other.refresh_workspace
    }
}

impl RetryPolicy {
    pub fn should_retry(&self, class: &FailureClass, attempt_count: u32) -> bool {
        if class.is_nonretryable_by_default() {
            return false;
        }
        if attempt_count >= self.max_attempts {
            return false;
        }
        self.retryable_classes.contains(class) || class.is_retryable_by_default()
    }

    pub fn next_delay_ms(&self, attempt_count: u32) -> u64 {
        let exp = self.initial_delay_ms as f64 * self.backoff_factor.powi(attempt_count as i32 - 1);
        let capped = exp.min(self.max_delay_ms as f64) as u64;
        if self.jitter {
            capped / 2 + (capped / 2) * (attempt_count as u64 % 2)
        } else {
            capped
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptLease {
    pub lease_id: LeaseId,
    pub attempt_id: AttemptId,
    pub lease_epoch: u64,
    pub holder: ExecutionTargetId,
    pub issued_at: u64,
    pub expires_at: u64,
    pub heartbeat_interval_secs: u64,
    pub last_heartbeat_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseProof {
    pub lease_id: LeaseId,
    pub lease_epoch: u64,
    pub holder: ExecutionTargetId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeartbeatReport {
    pub attempt_state: String,
    pub progress_marker: Option<String>,
    pub resource_usage: Option<ResourceUsage>,
    pub output_size_bytes: Option<u64>,
    pub lease_renewal_requested: bool,
    pub executor_health: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResourceUsage {
    pub cpu_millis: Option<u64>,
    pub memory_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RunDeadlines {
    pub queue_deadline: Option<u64>,
    pub run_deadline: Option<u64>,
    pub lease_duration_secs: Option<u64>,
    pub attempt_execution_timeout_secs: Option<u64>,
    pub process_timeout_secs: Option<u64>,
    pub model_request_timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutKind {
    Queue,
    Lease,
    AttemptExecution,
    Task,
    Run,
    Process,
    ModelRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeculationConfig {
    pub allowed: bool,
    pub max_simultaneous_attempts: u32,
    pub require_result_agreement: bool,
}

impl Default for SpeculationConfig {
    fn default() -> Self {
        Self {
            allowed: false,
            max_simultaneous_attempts: 1,
            require_result_agreement: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CancellationRecord {
    pub session_canceled: bool,
    pub run_canceled: bool,
    pub canceled_at: Option<u64>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SideEffectCommitRecord {
    pub operation_key: String,
    pub committed: bool,
    pub transaction_id: Option<TransactionId>,
    pub committed_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TaskRetryState {
    pub attempt_count: u32,
    pub next_retry_at: Option<u64>,
    pub last_failure_class: Option<FailureClass>,
    pub last_failure_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VerificationRemediation {
    pub retry_with_revised_input: bool,
    pub spawn_remediation_task: bool,
    pub require_different_specialist: bool,
}
