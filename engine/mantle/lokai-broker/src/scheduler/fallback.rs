//! Fallback actions by failure class (M6-2).

use lokai_domain::ids::WorkerId;
use lokai_inference::{InferenceError, LOCAL_NODE_ID};

use crate::scheduler::types::ExecutionTargetId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackFailureClass {
    RemoteRejectBeforeStart,
    TransientTransport,
    WorkerLoss,
    PermanentPolicy,
    LocalExhaustion,
    CapabilityExpired,
    TrustDowngraded,
}

/// Map a remote Infer failure into a fallback class for `fallback_action`.
pub fn classify_inference_error(err: &InferenceError) -> FallbackFailureClass {
    match err {
        InferenceError::WorkerBusy { .. } => FallbackFailureClass::RemoteRejectBeforeStart,
        InferenceError::Preempted { .. } => FallbackFailureClass::WorkerLoss,
        InferenceError::Egress(_) => FallbackFailureClass::TransientTransport,
        InferenceError::IncompleteStream { .. } | InferenceError::StreamTimeout { .. } => {
            FallbackFailureClass::TransientTransport
        }
        InferenceError::GpuSpillDetected { .. } => FallbackFailureClass::LocalExhaustion,
        InferenceError::SecretScanFailed { .. } | InferenceError::RemoteSecretDenied { .. } => {
            FallbackFailureClass::PermanentPolicy
        }
        InferenceError::Decode(_) => FallbackFailureClass::PermanentPolicy,
        InferenceError::Provider(msg) => {
            let m = msg.to_ascii_lowercase();
            if m.contains("secret")
                || m.contains("denied")
                || m.contains("policy")
                || m.contains("placement denied")
            {
                FallbackFailureClass::PermanentPolicy
            } else if m.contains("trust") || m.contains("revok") {
                FallbackFailureClass::TrustDowngraded
            } else if m.contains("failover hop ineligible")
                || m.contains("expired")
                || m.contains("capability")
            {
                FallbackFailureClass::CapabilityExpired
            } else if m.contains("busy")
                || m.contains("queue full")
                || m.contains("draining")
                || m.contains("reject")
            {
                FallbackFailureClass::RemoteRejectBeforeStart
            } else if m.contains("worker lost")
                || m.contains("worker_lost")
                || m.contains("connection reset")
            {
                FallbackFailureClass::WorkerLoss
            } else {
                FallbackFailureClass::TransientTransport
            }
        }
    }
}

pub fn parse_target_label(label: &str) -> ExecutionTargetId {
    if label == LOCAL_NODE_ID || label.eq_ignore_ascii_case("local") {
        ExecutionTargetId::Local
    } else {
        ExecutionTargetId::Worker {
            worker_id: WorkerId::new(label),
        }
    }
}

pub fn labels_to_targets(labels: &[String]) -> Vec<ExecutionTargetId> {
    labels.iter().map(|l| parse_target_label(l)).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FallbackAction {
    /// Try next eligible remote, else local if admitted.
    NextEligibleOrLocal,
    /// Bounded retry same worker, then next, then local.
    RetryThenNextThenLocal { max_same_worker: u32 },
    /// New attempt; never accept superseded result.
    NewAttemptAfterLeaseExpire,
    /// Do not retry same invalid placement.
    FailExplicit,
    /// Queue locally within deadline or remote if eligible.
    QueueLocalOrRemoteOrReject,
}

pub fn fallback_action(class: FallbackFailureClass) -> FallbackAction {
    match class {
        FallbackFailureClass::RemoteRejectBeforeStart
        | FallbackFailureClass::CapabilityExpired
        | FallbackFailureClass::TrustDowngraded => FallbackAction::NextEligibleOrLocal,
        FallbackFailureClass::TransientTransport => {
            FallbackAction::RetryThenNextThenLocal { max_same_worker: 1 }
        }
        FallbackFailureClass::WorkerLoss => FallbackAction::NewAttemptAfterLeaseExpire,
        FallbackFailureClass::PermanentPolicy => FallbackAction::FailExplicit,
        FallbackFailureClass::LocalExhaustion => FallbackAction::QueueLocalOrRemoteOrReject,
    }
}

/// Pick next target from an ordered fallback list after `failed`.
pub fn next_fallback(
    order: &[ExecutionTargetId],
    failed: &ExecutionTargetId,
) -> Option<ExecutionTargetId> {
    let mut seen = false;
    for t in order {
        if seen {
            return Some(t.clone());
        }
        if t == failed {
            seen = true;
        }
    }
    None
}
