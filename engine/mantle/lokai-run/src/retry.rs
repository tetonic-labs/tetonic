//! Retry and failure handling (M3-2).

use lokai_domain::{AttemptState, FailureClass, RunSnapshot, TaskState, TimeoutKind};

use crate::dag::{apply_dependency_failure, recompute_blocked_ready};
use crate::side_effect::can_retry_side_effects;

pub fn apply_failure_with_retry(
    out: &mut RunSnapshot,
    task_id: &lokai_domain::TaskId,
    failure_class: FailureClass,
    reason: &str,
    now: u64,
) {
    let policy = out.tasks.get(task_id).unwrap().binding.retry_policy.clone();
    let retry = out.tasks.get_mut(task_id).unwrap().retry.clone();
    let attempt_count = retry.attempt_count + 1;

    out.tasks.get_mut(task_id).unwrap().retry = lokai_domain::TaskRetryState {
        attempt_count,
        last_failure_class: Some(failure_class.clone()),
        last_failure_reason: Some(reason.to_string()),
        next_retry_at: None,
    };

    let side_effect_ok = can_retry_side_effects(out, task_id);
    let verification = out
        .tasks
        .get(task_id)
        .unwrap()
        .binding
        .verification_policy
        .clone();
    let verification_retry = matches!(failure_class, FailureClass::VerificationFailed)
        && verification.remediation.retry_with_revised_input;

    if (policy.should_retry(&failure_class, attempt_count) || verification_retry) && side_effect_ok
    {
        let delay = policy.next_delay_ms(attempt_count);
        let task = out.tasks.get_mut(task_id).unwrap();
        task.state = TaskState::Ready;
        task.active_attempt = None;
        task.retry.next_retry_at = Some(now + delay / 1000);
    } else {
        let task = out.tasks.get_mut(task_id).unwrap();
        task.state = TaskState::Failed;
        task.active_attempt = None;
        apply_dependency_failure(out, task_id);
    }
    recompute_blocked_ready(out);
}

pub fn failure_class_for_timeout(kind: &TimeoutKind) -> FailureClass {
    match kind {
        TimeoutKind::Lease => FailureClass::LeaseExpired,
        _ => FailureClass::TimedOut,
    }
}

pub fn map_attempt_state_for_failure(class: &FailureClass) -> AttemptState {
    match class {
        FailureClass::LeaseExpired => AttemptState::LeaseExpired,
        FailureClass::TimedOut => AttemptState::TimedOut,
        FailureClass::Canceled => AttemptState::Canceled,
        _ => AttemptState::Failed,
    }
}

pub fn verification_remediation(class: &FailureClass) -> lokai_domain::VerificationRemediation {
    if matches!(class, FailureClass::VerificationFailed) {
        lokai_domain::VerificationRemediation {
            retry_with_revised_input: true,
            spawn_remediation_task: true,
            require_different_specialist: false,
        }
    } else {
        lokai_domain::VerificationRemediation::default()
    }
}
