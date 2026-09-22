//! State transition validation and projection updates (M3-1 / M3-2).

use lokai_domain::{
    AcceptArtifact, AddDependency, AddTask, AttemptLease, AttemptRecord, AttemptState, CancelRun,
    CancelTask, ClaimFinalization, CompleteAttempt, CreateAttempt, CreateRun, ExpireLease,
    FailAttempt, FailureClass, FinishRun, LeaseAttempt, MarkTaskReady, RecordHeartbeat,
    RecordSideEffectCommit, RejectArtifact, RunCommand, RunFinishOutcome, RunSnapshot, RunState,
    RunSupervisorError, StartAttempt, StartRun, TaskDependency, TaskRecord, TaskState,
};

use crate::acceptance::{apply_winner_selection, try_accept_completion, try_claim_finalization};
use crate::dag::{
    dependencies_satisfied, recompute_blocked_ready, task_is_executable, task_is_locked,
    would_create_cycle,
};
use crate::idempotency::{binding_input_digest, register_delivery_key};
use crate::lease::{can_renew_lease, renew_lease, validate_lease_proof};
use crate::retry::{
    apply_failure_with_retry, failure_class_for_timeout, map_attempt_state_for_failure,
};
use crate::side_effect::record_side_effect_commit;

pub fn apply_command(
    snapshot: &RunSnapshot,
    command: &RunCommand,
) -> Result<RunSnapshot, RunSupervisorError> {
    match command {
        RunCommand::CreateRun(c) => apply_create_run(snapshot, c),
        RunCommand::StartRun(c) => apply_start_run(snapshot, c),
        RunCommand::AddTask(c) => apply_add_task(snapshot, c),
        RunCommand::AddDependency(c) => apply_add_dependency(snapshot, c),
        RunCommand::MarkTaskReady(c) => apply_mark_task_ready(snapshot, c),
        RunCommand::CreateAttempt(c) => apply_create_attempt(snapshot, c),
        RunCommand::LeaseAttempt(c) => apply_lease_attempt(snapshot, c),
        RunCommand::StartAttempt(c) => apply_start_attempt(snapshot, c),
        RunCommand::ClaimExecution(c) => apply_claim_execution(snapshot, c),
        RunCommand::RecordHeartbeat(c) => apply_record_heartbeat(snapshot, c),
        RunCommand::ClaimFinalization(c) => apply_claim_finalization(snapshot, c),
        RunCommand::CompleteAttempt(c) => apply_complete_attempt(snapshot, c),
        RunCommand::FailAttempt(c) => apply_fail_attempt(snapshot, c),
        RunCommand::ExpireLease(c) => apply_expire_lease(snapshot, c),
        RunCommand::CancelTask(c) => apply_cancel_task(snapshot, c),
        RunCommand::CancelRun(c) => apply_cancel_run(snapshot, c),
        RunCommand::AcceptArtifact(c) => apply_accept_artifact(snapshot, c),
        RunCommand::RejectArtifact(c) => apply_reject_artifact(snapshot, c),
        RunCommand::RecordSideEffectCommit(c) => apply_record_side_effect_commit(snapshot, c),
        RunCommand::FinishRun(c) => apply_finish_run(snapshot, c),
    }
}

fn new_task(task_id: lokai_domain::TaskId, binding: lokai_domain::TaskInputBinding) -> TaskRecord {
    TaskRecord {
        task_id,
        state: TaskState::Created,
        binding,
        accepted_artifact: None,
        active_attempt: None,
        winning_attempt: None,
        finalization_claim: None,
        completed_version: None,
        retry: Default::default(),
        side_effect_keys: Vec::new(),
    }
}

fn ensure_not_recovery(run: &RunSnapshot) -> Result<(), RunSupervisorError> {
    if run.state == RunState::RecoveryRequired {
        return Err(RunSupervisorError::RecoveryRequired);
    }
    Ok(())
}

fn ensure_run_accepts_mutations(run: &RunSnapshot) -> Result<(), RunSupervisorError> {
    ensure_not_recovery(run)?;
    if run.cancellation.run_canceled {
        return Err(RunSupervisorError::RunNotAccepting(RunState::Canceled));
    }
    match &run.state {
        RunState::Active | RunState::Created => Ok(()),
        RunState::Canceling => Err(RunSupervisorError::RunNotAccepting(run.state.clone())),
        other => Err(RunSupervisorError::RunNotAccepting(other.clone())),
    }
}

fn apply_create_run(
    snapshot: &RunSnapshot,
    cmd: &CreateRun,
) -> Result<RunSnapshot, RunSupervisorError> {
    if !snapshot.tasks.is_empty() || snapshot.sequence > 0 {
        return Err(RunSupervisorError::Conflict(format!(
            "run {} already exists",
            cmd.run_id
        )));
    }
    let mut out = snapshot.clone();
    out.run_id = cmd.run_id.clone();
    out.session_id = cmd.session_id.clone();
    out.state = RunState::Created;
    out.workspace_version = cmd.root_binding.workspace_version.clone();
    out.deadlines.run_deadline = cmd.root_binding.deadline;
    out.tasks.insert(
        cmd.root_task_id.clone(),
        new_task(cmd.root_task_id.clone(), cmd.root_binding.clone()),
    );
    if let Some(spec) = &cmd.speculation {
        out.speculation = spec.clone();
    }
    out.job_spec = cmd.job_spec.clone();
    Ok(out)
}

fn apply_start_run(
    snapshot: &RunSnapshot,
    cmd: &StartRun,
) -> Result<RunSnapshot, RunSupervisorError> {
    if snapshot.run_id != cmd.run_id {
        return Err(RunSupervisorError::RunNotFound(cmd.run_id.to_string()));
    }
    if snapshot.state != RunState::Created {
        return Err(RunSupervisorError::InvalidTransition(format!(
            "cannot start run from {:?}",
            snapshot.state
        )));
    }
    let mut out = snapshot.clone();
    out.state = RunState::Active;
    recompute_blocked_ready(&mut out);
    Ok(out)
}

fn apply_add_task(
    snapshot: &RunSnapshot,
    cmd: &AddTask,
) -> Result<RunSnapshot, RunSupervisorError> {
    ensure_run_accepts_mutations(snapshot)?;
    if snapshot.tasks.contains_key(&cmd.task_id) {
        return Err(RunSupervisorError::Conflict(format!(
            "task {} exists",
            cmd.task_id
        )));
    }
    let mut out = snapshot.clone();
    out.tasks.insert(
        cmd.task_id.clone(),
        new_task(cmd.task_id.clone(), cmd.binding.clone()),
    );
    recompute_blocked_ready(&mut out);
    Ok(out)
}

fn apply_add_dependency(
    snapshot: &RunSnapshot,
    cmd: &AddDependency,
) -> Result<RunSnapshot, RunSupervisorError> {
    ensure_run_accepts_mutations(snapshot)?;
    let task = snapshot
        .tasks
        .get(&cmd.task_id)
        .ok_or_else(|| RunSupervisorError::TaskNotFound(cmd.task_id.to_string()))?;
    if task_is_locked(&task.state) {
        return Err(RunSupervisorError::DependencyLocked);
    }
    if !snapshot.tasks.contains_key(&cmd.depends_on) {
        return Err(RunSupervisorError::TaskNotFound(cmd.depends_on.to_string()));
    }
    if would_create_cycle(&snapshot.dependencies, &cmd.task_id, &cmd.depends_on) {
        return Err(RunSupervisorError::CycleDetected);
    }
    let mut out = snapshot.clone();
    out.dependencies
        .entry(cmd.task_id.clone())
        .or_default()
        .push(TaskDependency {
            depends_on: cmd.depends_on.clone(),
            policy: cmd.policy.clone(),
        });
    recompute_blocked_ready(&mut out);
    Ok(out)
}

fn apply_mark_task_ready(
    snapshot: &RunSnapshot,
    cmd: &MarkTaskReady,
) -> Result<RunSnapshot, RunSupervisorError> {
    ensure_run_accepts_mutations(snapshot)?;
    let task = snapshot
        .tasks
        .get(&cmd.task_id)
        .ok_or_else(|| RunSupervisorError::TaskNotFound(cmd.task_id.to_string()))?;
    if task.state == TaskState::Ready {
        return Ok(snapshot.clone());
    }
    if !matches!(task.state, TaskState::Created | TaskState::Blocked) {
        return Err(RunSupervisorError::InvalidTransition(format!(
            "cannot mark ready from {:?}",
            task.state
        )));
    }
    if let Some(retry_at) = task.retry.next_retry_at {
        if cmd.envelope.timestamp < retry_at {
            return Err(RunSupervisorError::InvalidTransition(
                "retry backoff not elapsed".into(),
            ));
        }
    }
    if !dependencies_satisfied(snapshot, &cmd.task_id) {
        return Err(RunSupervisorError::InvalidTransition(
            "dependencies not satisfied".into(),
        ));
    }
    let mut out = snapshot.clone();
    out.tasks
        .get_mut(&cmd.task_id)
        .ok_or_else(|| RunSupervisorError::TaskNotFound(cmd.task_id.to_string()))?
        .state = TaskState::Ready;
    Ok(out)
}

fn apply_create_attempt(
    snapshot: &RunSnapshot,
    cmd: &CreateAttempt,
) -> Result<RunSnapshot, RunSupervisorError> {
    ensure_run_accepts_mutations(snapshot)?;
    if let Some(key) = &cmd.delivery_key {
        if let Some(existing) = snapshot.delivery_index.get(key) {
            if existing != &cmd.attempt_id {
                return Err(RunSupervisorError::DuplicateDelivery(format!(
                    "delivery key bound to {existing}"
                )));
            }
            return Ok(snapshot.clone());
        }
    }
    let task = snapshot
        .tasks
        .get(&cmd.task_id)
        .ok_or_else(|| RunSupervisorError::TaskNotFound(cmd.task_id.to_string()))?;
    if task.state != TaskState::Ready
        && task.state != TaskState::Leased
        && !(snapshot.speculation.allowed && task.state == TaskState::Running)
    {
        return Err(RunSupervisorError::InvalidTransition(format!(
            "cannot create attempt for task in {:?}",
            task.state
        )));
    }
    if task.retry.attempt_count >= task.binding.retry_policy.max_attempts {
        return Err(RunSupervisorError::RetryLimitExceeded);
    }
    if snapshot.attempts.contains_key(&cmd.attempt_id) {
        return Err(RunSupervisorError::Conflict(format!(
            "attempt {} exists",
            cmd.attempt_id
        )));
    }
    let active_count = snapshot
        .attempts
        .values()
        .filter(|a| {
            a.task_id == cmd.task_id
                && matches!(
                    a.state,
                    AttemptState::Created
                        | AttemptState::Leased
                        | AttemptState::Starting
                        | AttemptState::Running
                )
        })
        .count() as u32;
    if active_count >= snapshot.speculation.max_simultaneous_attempts {
        return Err(RunSupervisorError::Conflict(
            "max simultaneous attempts reached".into(),
        ));
    }
    let mut out = snapshot.clone();
    let binding = out
        .tasks
        .get(&cmd.task_id)
        .ok_or_else(|| RunSupervisorError::TaskNotFound(cmd.task_id.to_string()))?
        .binding
        .clone();
    let attempt_number = out
        .tasks
        .get(&cmd.task_id)
        .ok_or_else(|| RunSupervisorError::TaskNotFound(cmd.task_id.to_string()))?
        .retry
        .attempt_count
        + 1;
    let input_digest = binding
        .job_spec
        .as_ref()
        .or(out.job_spec.as_ref())
        .map(|spec| spec.input_digest.clone())
        .unwrap_or_else(|| binding_input_digest(&binding));
    out.attempts.insert(
        cmd.attempt_id.clone(),
        AttemptRecord {
            execution_claimed: false,
            attempt_id: cmd.attempt_id.clone(),
            task_id: cmd.task_id.clone(),
            state: AttemptState::Created,
            task_version: binding.task_definition_version,
            workspace_version: binding.workspace_version.clone(),
            input_digest,
            result_digest: None,
            delivery_key: cmd.delivery_key.clone(),
            lease: None,
            failure_class: None,
            failure_reason: None,
            attempt_number,
        },
    );
    if let Some(key) = &cmd.delivery_key {
        register_delivery_key(&mut out, key.clone(), cmd.attempt_id.clone());
    }
    if let Some(t) = out.tasks.get_mut(&cmd.task_id) {
        t.active_attempt = Some(cmd.attempt_id.clone());
        if t.state == TaskState::Ready {
            t.state = TaskState::Leased;
        }
    }
    Ok(out)
}

fn apply_lease_attempt(
    snapshot: &RunSnapshot,
    cmd: &LeaseAttempt,
) -> Result<RunSnapshot, RunSupervisorError> {
    ensure_run_accepts_mutations(snapshot)?;
    let attempt = snapshot
        .attempts
        .get(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;
    if attempt.state != AttemptState::Created {
        return Err(RunSupervisorError::InvalidTransition(format!(
            "cannot lease attempt from {:?}",
            attempt.state
        )));
    }
    let mut out = snapshot.clone();
    let epoch = if cmd.lease_epoch > 0 {
        cmd.lease_epoch
    } else {
        out.next_lease_epoch + 1
    };
    out.next_lease_epoch = epoch;
    let lease = AttemptLease {
        lease_id: cmd.lease_id.clone(),
        attempt_id: cmd.attempt_id.clone(),
        lease_epoch: epoch,
        holder: cmd.holder.clone(),
        issued_at: cmd.issued_at,
        expires_at: cmd.expires_at,
        heartbeat_interval_secs: cmd.heartbeat_interval_secs,
        last_heartbeat_sequence: 0,
    };
    let a = out
        .attempts
        .get_mut(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;
    a.state = AttemptState::Leased;
    a.lease = Some(lease);
    Ok(out)
}

fn apply_claim_execution(
    snapshot: &RunSnapshot,
    cmd: &StartAttempt,
) -> Result<RunSnapshot, RunSupervisorError> {
    ensure_run_accepts_mutations(snapshot)?;
    let attempt = snapshot
        .attempts
        .get(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;
    if attempt.state != AttemptState::Running || attempt.execution_claimed {
        return Err(RunSupervisorError::DuplicateDelivery(
            "Attempt already dispatched or not running".into(),
        ));
    }
    validate_lease_proof(attempt, &cmd.lease_proof)?;
    if !crate::lease::is_lease_current(attempt, cmd.envelope.timestamp) {
        return Err(RunSupervisorError::StaleResult(
            "lease expired before dispatch".into(),
        ));
    }
    let mut out = snapshot.clone();
    out.attempts
        .get_mut(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?
        .execution_claimed = true;
    Ok(out)
}

fn apply_start_attempt(
    snapshot: &RunSnapshot,
    cmd: &StartAttempt,
) -> Result<RunSnapshot, RunSupervisorError> {
    ensure_run_accepts_mutations(snapshot)?;
    let attempt = snapshot
        .attempts
        .get(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;
    if attempt.state != AttemptState::Leased {
        return Err(RunSupervisorError::InvalidTransition(format!(
            "cannot start attempt from {:?}",
            attempt.state
        )));
    }
    validate_lease_proof(attempt, &cmd.lease_proof)?;
    let mut out = snapshot.clone();
    let task_id = out
        .attempts
        .get(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?
        .task_id
        .clone();
    out.attempts
        .get_mut(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?
        .state = AttemptState::Running;
    if let Some(t) = out.tasks.get_mut(&task_id) {
        t.state = TaskState::Running;
    }
    Ok(out)
}

fn apply_record_heartbeat(
    snapshot: &RunSnapshot,
    cmd: &RecordHeartbeat,
) -> Result<RunSnapshot, RunSupervisorError> {
    let attempt = snapshot
        .attempts
        .get(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;
    if !matches!(
        attempt.state,
        AttemptState::Leased | AttemptState::Starting | AttemptState::Running
    ) {
        return Err(RunSupervisorError::InvalidTransition(
            "heartbeat on inactive attempt".into(),
        ));
    }
    validate_lease_proof(attempt, &cmd.lease_proof)?;
    if cmd.heartbeat_sequence
        <= attempt
            .lease
            .as_ref()
            .map(|l| l.last_heartbeat_sequence)
            .unwrap_or(0)
    {
        return Err(RunSupervisorError::StaleResult(
            "non-monotonic heartbeat sequence".into(),
        ));
    }
    let mut out = snapshot.clone();
    let now = cmd.envelope.timestamp;
    if !can_renew_lease(&out, attempt, now) {
        return Err(RunSupervisorError::InvalidTransition(
            "cannot renew lease".into(),
        ));
    }
    if let Some(new_exp) = cmd.renewed_expires_at {
        if cmd.report.lease_renewal_requested {
            if let Some(lease) = out
                .attempts
                .get_mut(&cmd.attempt_id)
                .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?
                .lease
                .as_mut()
            {
                renew_lease(lease, cmd.heartbeat_sequence, new_exp);
            }
        }
    } else if let Some(lease) = out
        .attempts
        .get_mut(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?
        .lease
        .as_mut()
    {
        lease.last_heartbeat_sequence = cmd.heartbeat_sequence;
    }
    Ok(out)
}

fn apply_claim_finalization(
    snapshot: &RunSnapshot,
    cmd: &ClaimFinalization,
) -> Result<RunSnapshot, RunSupervisorError> {
    ensure_not_recovery(snapshot)?;
    try_claim_finalization(snapshot, cmd)?;
    let mut out = snapshot.clone();
    let task = out
        .tasks
        .get_mut(&cmd.task_id)
        .ok_or_else(|| RunSupervisorError::TaskNotFound(cmd.task_id.to_string()))?;
    task.finalization_claim = Some(cmd.attempt_id.clone());
    Ok(out)
}

fn apply_complete_attempt(
    snapshot: &RunSnapshot,
    cmd: &CompleteAttempt,
) -> Result<RunSnapshot, RunSupervisorError> {
    ensure_not_recovery(snapshot)?;
    try_accept_completion(snapshot, cmd)?;
    let attempt = snapshot
        .attempts
        .get(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;
    if attempt.state == AttemptState::Succeeded {
        return Ok(snapshot.clone());
    }
    let task_id = attempt.task_id.clone();
    let mut out = snapshot.clone();
    let a = out
        .attempts
        .get_mut(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;
    a.state = AttemptState::Succeeded;
    a.result_digest = Some(cmd.result_digest.clone());
    a.workspace_version = cmd.workspace_version.clone();
    apply_winner_selection(&mut out, &cmd.attempt_id, &task_id, cmd.task_version);
    Ok(out)
}

fn apply_fail_attempt(
    snapshot: &RunSnapshot,
    cmd: &FailAttempt,
) -> Result<RunSnapshot, RunSupervisorError> {
    ensure_not_recovery(snapshot)?;
    let attempt = snapshot
        .attempts
        .get(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;
    if let Some(proof) = &cmd.lease_proof {
        validate_lease_proof(attempt, proof)?;
    }
    if !matches!(
        attempt.state,
        AttemptState::Running | AttemptState::Starting | AttemptState::Leased
    ) {
        return Err(RunSupervisorError::InvalidTransition(format!(
            "cannot fail attempt from {:?}",
            attempt.state
        )));
    }
    let failure_class = if let Some(kind) = &cmd.timeout_kind {
        failure_class_for_timeout(kind)
    } else {
        cmd.failure_class.clone()
    };
    if failure_class.is_nonretryable_by_default()
        && !matches!(failure_class, FailureClass::VerificationFailed)
    {
        // permanent by class
    }
    let task_id = attempt.task_id.clone();
    let mut out = snapshot.clone();
    let state = map_attempt_state_for_failure(&failure_class);
    let a = out
        .attempts
        .get_mut(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;
    a.state = state;
    a.failure_class = Some(failure_class.clone());
    a.failure_reason = Some(cmd.reason.clone());
    apply_failure_with_retry(
        &mut out,
        &task_id,
        failure_class,
        &cmd.reason,
        cmd.envelope.timestamp,
    );
    Ok(out)
}

fn apply_expire_lease(
    snapshot: &RunSnapshot,
    cmd: &ExpireLease,
) -> Result<RunSnapshot, RunSupervisorError> {
    let attempt = snapshot
        .attempts
        .get(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;
    if !matches!(
        attempt.state,
        AttemptState::Leased | AttemptState::Starting | AttemptState::Running
    ) {
        return Ok(snapshot.clone());
    }
    let task_id = attempt.task_id.clone();
    let mut out = snapshot.clone();
    let a = out
        .attempts
        .get_mut(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;
    a.state = AttemptState::LeaseExpired;
    a.failure_class = Some(FailureClass::LeaseExpired);
    a.failure_reason = Some("lease expired".into());
    apply_failure_with_retry(
        &mut out,
        &task_id,
        FailureClass::LeaseExpired,
        "lease expired",
        cmd.expired_at,
    );
    Ok(out)
}

fn apply_cancel_task(
    snapshot: &RunSnapshot,
    cmd: &CancelTask,
) -> Result<RunSnapshot, RunSupervisorError> {
    let task = snapshot
        .tasks
        .get(&cmd.task_id)
        .ok_or_else(|| RunSupervisorError::TaskNotFound(cmd.task_id.to_string()))?;
    if task.state == TaskState::Succeeded {
        return Err(RunSupervisorError::InvalidTransition(
            "cannot cancel succeeded task".into(),
        ));
    }
    let mut out = snapshot.clone();
    out.tasks
        .get_mut(&cmd.task_id)
        .ok_or_else(|| RunSupervisorError::TaskNotFound(cmd.task_id.to_string()))?
        .state = TaskState::Canceled;
    for a in out.attempts.values_mut() {
        if a.task_id == cmd.task_id
            && matches!(
                a.state,
                AttemptState::Created
                    | AttemptState::Leased
                    | AttemptState::Starting
                    | AttemptState::Running
            )
        {
            a.state = AttemptState::Canceled;
            a.failure_class = Some(FailureClass::Canceled);
        }
    }
    Ok(out)
}

fn apply_cancel_run(
    snapshot: &RunSnapshot,
    cmd: &CancelRun,
) -> Result<RunSnapshot, RunSupervisorError> {
    if snapshot.run_id != cmd.run_id {
        return Err(RunSupervisorError::RunNotFound(cmd.run_id.to_string()));
    }
    if matches!(
        snapshot.state,
        RunState::Succeeded | RunState::Failed | RunState::Canceled
    ) {
        return Ok(snapshot.clone());
    }
    // Recovery abandonment must acknowledge the inspected snapshot. Replay uses
    // the same transition, preserving cancellation and effect history in the journal.
    if snapshot.state == RunState::RecoveryRequired
        && cmd.envelope.expected_sequence != Some(snapshot.sequence)
    {
        return Err(RunSupervisorError::RecoveryRequired);
    }
    let mut out = snapshot.clone();
    out.cancellation = lokai_domain::CancellationRecord {
        session_canceled: out.cancellation.session_canceled,
        run_canceled: true,
        canceled_at: Some(cmd.envelope.timestamp),
        reason: Some("run canceled".into()),
    };
    out.state = RunState::Canceling;
    for t in out.tasks.values_mut() {
        if task_is_executable(&t.state) || task_is_locked(&t.state) {
            t.state = TaskState::Canceled;
        }
    }
    for a in out.attempts.values_mut() {
        if matches!(
            a.state,
            AttemptState::Created
                | AttemptState::Leased
                | AttemptState::Starting
                | AttemptState::Running
        ) {
            a.state = AttemptState::Canceled;
            a.failure_class = Some(FailureClass::Canceled);
        }
    }
    out.state = RunState::Canceled;
    Ok(out)
}

fn apply_accept_artifact(
    snapshot: &RunSnapshot,
    cmd: &AcceptArtifact,
) -> Result<RunSnapshot, RunSupervisorError> {
    ensure_not_recovery(snapshot)?;
    let attempt = snapshot
        .attempts
        .get(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;
    if attempt.state != AttemptState::Succeeded {
        return Err(RunSupervisorError::InvalidTransition(
            "attempt must succeed before artifact acceptance".into(),
        ));
    }
    if attempt.task_id != cmd.task_id {
        return Err(RunSupervisorError::Conflict("task/attempt mismatch".into()));
    }
    let mut out = snapshot.clone();
    let task = out
        .tasks
        .get_mut(&cmd.task_id)
        .ok_or_else(|| RunSupervisorError::TaskNotFound(cmd.task_id.to_string()))?;
    task.accepted_artifact = Some(cmd.artifact.clone());
    task.state = TaskState::Succeeded;
    Ok(out)
}

fn apply_reject_artifact(
    snapshot: &RunSnapshot,
    cmd: &RejectArtifact,
) -> Result<RunSnapshot, RunSupervisorError> {
    let attempt = snapshot
        .attempts
        .get(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;
    if attempt.state != AttemptState::Succeeded {
        return Err(RunSupervisorError::InvalidTransition(
            "attempt not succeeded".into(),
        ));
    }
    if attempt.task_id != cmd.task_id {
        return Err(RunSupervisorError::Conflict("task/attempt mismatch".into()));
    }
    let mut out = snapshot.clone();
    let task_id = out
        .attempts
        .get(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?
        .task_id
        .clone();
    out.tasks
        .get_mut(&cmd.task_id)
        .ok_or_else(|| RunSupervisorError::TaskNotFound(cmd.task_id.to_string()))?
        .state = TaskState::Ready;
    out.tasks
        .get_mut(&cmd.task_id)
        .ok_or_else(|| RunSupervisorError::TaskNotFound(cmd.task_id.to_string()))?
        .winning_attempt = None;
    out.tasks
        .get_mut(&cmd.task_id)
        .ok_or_else(|| RunSupervisorError::TaskNotFound(cmd.task_id.to_string()))?
        .completed_version = None;
    let a = out
        .attempts
        .get_mut(&cmd.attempt_id)
        .ok_or_else(|| RunSupervisorError::AttemptNotFound(cmd.attempt_id.to_string()))?;
    a.state = AttemptState::Failed;
    a.failure_class = Some(FailureClass::VerificationFailed);
    a.failure_reason = Some(cmd.reason.clone());
    apply_failure_with_retry(
        &mut out,
        &task_id,
        FailureClass::VerificationFailed,
        &cmd.reason,
        cmd.envelope.timestamp,
    );
    Ok(out)
}

fn apply_record_side_effect_commit(
    snapshot: &RunSnapshot,
    cmd: &RecordSideEffectCommit,
) -> Result<RunSnapshot, RunSupervisorError> {
    let mut out = snapshot.clone();
    record_side_effect_commit(
        &mut out,
        &cmd.task_id,
        &cmd.operation_key,
        cmd.transaction_id.clone(),
        cmd.committed_at,
    )?;
    Ok(out)
}

fn apply_finish_run(
    snapshot: &RunSnapshot,
    cmd: &FinishRun,
) -> Result<RunSnapshot, RunSupervisorError> {
    if snapshot.run_id != cmd.run_id {
        return Err(RunSupervisorError::RunNotFound(cmd.run_id.to_string()));
    }
    if snapshot.state == RunState::Canceled && cmd.outcome == RunFinishOutcome::Canceled {
        return Ok(snapshot.clone());
    }
    if !matches!(snapshot.state, RunState::Active | RunState::Created) {
        return Err(RunSupervisorError::InvalidTransition(format!(
            "cannot finish run from {:?}",
            snapshot.state
        )));
    }
    let mut out = snapshot.clone();
    out.state = match cmd.outcome {
        RunFinishOutcome::Succeeded => RunState::Succeeded,
        RunFinishOutcome::Failed => RunState::Failed,
        RunFinishOutcome::Canceled => RunState::Canceled,
    };
    Ok(out)
}

pub fn event_type_for(command: &RunCommand) -> &'static str {
    match command {
        RunCommand::CreateRun(_) => "run.created",
        RunCommand::StartRun(_) => "run.started",
        RunCommand::AddTask(_) => "task.added",
        RunCommand::AddDependency(_) => "dependency.added",
        RunCommand::MarkTaskReady(_) => "task.ready",
        RunCommand::CreateAttempt(_) => "attempt.created",
        RunCommand::LeaseAttempt(_) => "attempt.leased",
        RunCommand::StartAttempt(_) => "attempt.started",
        RunCommand::ClaimExecution(_) => "attempt.execution_claimed",
        RunCommand::RecordHeartbeat(_) => "attempt.heartbeat",
        RunCommand::ClaimFinalization(_) => "finalization.claimed",
        RunCommand::CompleteAttempt(_) => "attempt.completed",
        RunCommand::FailAttempt(_) => "attempt.failed",
        RunCommand::ExpireLease(_) => "attempt.lease_expired",
        RunCommand::CancelTask(_) => "task.canceled",
        RunCommand::CancelRun(_) => "run.canceled",
        RunCommand::AcceptArtifact(_) => "artifact.accepted",
        RunCommand::RejectArtifact(_) => "artifact.rejected",
        RunCommand::RecordSideEffectCommit(_) => "side_effect.committed",
        RunCommand::FinishRun(_) => "run.finished",
    }
}

pub fn failure_class_for_command(command: &RunCommand) -> Option<FailureClass> {
    match command {
        RunCommand::FailAttempt(c) => Some(c.failure_class.clone()),
        RunCommand::ExpireLease(_) => Some(FailureClass::LeaseExpired),
        RunCommand::RejectArtifact(_) => Some(FailureClass::VerificationFailed),
        _ => None,
    }
}
