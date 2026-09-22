//! Infer-admission API (WORK-02). Hops are classified compute, not agent jobs.
//! Every hop CreateRun forces `job_spec: None`.

use tetonic_domain::{
    AddTask, AttemptId, CancelTask, CreateAttempt, CreateRun, ExecutionTargetId, FailAttempt,
    FailureClass, LeaseAttempt, LeaseId, RunCommand, RunCommandResult, RunId, RunSnapshot,
    RunState, RunSupervisorError, SessionId, SpeculationConfig, StartRun, TaskId, TaskState,
};

use crate::{command_envelope, RunSupervisor};

fn hop_create_run(run_id: RunId, task_id: TaskId, session_id: Option<SessionId>) -> CreateRun {
    CreateRun {
        envelope: command_envelope(format!("hop_run_create_{}", run_id.0), None, "lokai-run"),
        session_id,
        run_id,
        root_task_id: task_id,
        root_binding: Default::default(),
        speculation: Some(SpeculationConfig {
            allowed: true,
            max_simultaneous_attempts: 2,
            require_result_agreement: false,
        }),
        job_spec: None,
    }
}

pub async fn ensure_hop_run(
    supervisor: &dyn RunSupervisor,
    run_id: RunId,
    task_id: TaskId,
    session_id: Option<SessionId>,
) -> Result<(), RunSupervisorError> {
    let run_exists = match supervisor.snapshot(run_id.clone()).await {
        Ok(snap) => {
            refuse_unclassified_hop(&snap)?;
            true
        }
        Err(RunSupervisorError::RunNotFound(_)) => false,
        Err(e) => return Err(e),
    };

    if !run_exists {
        match supervisor
            .handle(RunCommand::CreateRun(hop_create_run(
                run_id.clone(),
                task_id.clone(),
                session_id,
            )))
            .await
        {
            Ok(_) => {}
            Err(RunSupervisorError::Conflict(_)) => {}
            Err(e) => return Err(e),
        }
    }

    let snap = supervisor.snapshot(run_id.clone()).await?;
    refuse_unclassified_hop(&snap)?;
    if snap.state == RunState::Created {
        start_hop(supervisor, run_id.clone(), snap.sequence).await?;
    }

    let snap = supervisor.snapshot(run_id.clone()).await?;
    if !snap.tasks.contains_key(&task_id) {
        add_hop_task(supervisor, run_id, task_id, snap.sequence).await?;
    }
    Ok(())
}

pub async fn start_hop(
    supervisor: &dyn RunSupervisor,
    run_id: RunId,
    expected_sequence: u64,
) -> Result<RunCommandResult, RunSupervisorError> {
    supervisor
        .handle(RunCommand::StartRun(StartRun {
            envelope: command_envelope(
                format!("hop_run_start_{}", run_id.0),
                Some(expected_sequence),
                "lokai-run",
            ),
            run_id,
        }))
        .await
}

pub async fn add_hop_task(
    supervisor: &dyn RunSupervisor,
    run_id: RunId,
    task_id: TaskId,
    expected_sequence: u64,
) -> Result<RunCommandResult, RunSupervisorError> {
    supervisor
        .handle(RunCommand::AddTask(AddTask {
            envelope: command_envelope(
                format!("hop_task_{}", task_id.0),
                Some(expected_sequence),
                "lokai-run",
            ),
            run_id,
            task_id,
            binding: Default::default(),
        }))
        .await
}

pub async fn fail_hop(
    supervisor: &dyn RunSupervisor,
    run_id: RunId,
    attempt_id: AttemptId,
    expected_sequence: u64,
    reason: String,
) -> Result<u64, RunSupervisorError> {
    let snap = supervisor.snapshot(run_id.clone()).await?;
    if !hop_run_classified(&snap) {
        return Err(RunSupervisorError::InvalidTransition(format!(
            "hop command refused: run {} is not hop-classified",
            snap.run_id.0
        )));
    }
    let result = supervisor
        .handle(RunCommand::FailAttempt(FailAttempt {
            envelope: command_envelope(
                format!("hop_fail_{}", attempt_id.0),
                Some(expected_sequence),
                "lokai-run",
            ),
            run_id,
            attempt_id,
            failure_class: FailureClass::WorkerUnavailable,
            reason,
            lease_proof: None,
            timeout_kind: None,
        }))
        .await?;
    Ok(result.sequence)
}

pub async fn create_hop_attempt(
    supervisor: &dyn RunSupervisor,
    run_id: RunId,
    task_id: TaskId,
    attempt_id: AttemptId,
    expected_sequence: u64,
) -> Result<u64, RunSupervisorError> {
    let snap = supervisor.snapshot(run_id.clone()).await?;
    refuse_unclassified_hop(&snap)?;
    let created = supervisor
        .handle(RunCommand::CreateAttempt(CreateAttempt {
            envelope: command_envelope(
                format!("hop_create_{}", attempt_id.0),
                Some(expected_sequence),
                "lokai-run",
            ),
            run_id: run_id.clone(),
            task_id,
            attempt_id: attempt_id.clone(),
            delivery_key: Some(format!("infer-hop:{}:{}", run_id.0, attempt_id.0)),
        }))
        .await?;
    Ok(created.sequence)
}

pub async fn lease_hop(
    supervisor: &dyn RunSupervisor,
    run_id: RunId,
    attempt_id: AttemptId,
    lease_id: LeaseId,
    holder: ExecutionTargetId,
    issued_at: u64,
    expected_sequence: u64,
) -> Result<RunCommandResult, RunSupervisorError> {
    let snap = supervisor.snapshot(run_id.clone()).await?;
    refuse_unclassified_hop(&snap)?;
    supervisor
        .handle(RunCommand::LeaseAttempt(LeaseAttempt {
            envelope: command_envelope(
                format!("hop_lease_{}", attempt_id.0),
                Some(expected_sequence),
                "lokai-run",
            ),
            run_id,
            attempt_id,
            lease_id,
            lease_epoch: 0,
            holder,
            issued_at,
            expires_at: issued_at.saturating_add(3600),
            heartbeat_interval_secs: 30,
        }))
        .await
}

pub async fn cancel_hop(
    supervisor: &dyn RunSupervisor,
    run_id: RunId,
    task_id: TaskId,
) -> Result<(), RunSupervisorError> {
    let snap = supervisor.snapshot(run_id.clone()).await?;
    refuse_unclassified_hop(&snap)?;
    supervisor
        .handle(RunCommand::CancelTask(CancelTask {
            envelope: command_envelope("broker_cancel", None, "lokai-run"),
            run_id,
            task_id,
        }))
        .await
        .map(|_| ())
}

/// Admission refuses a hop CreateRun that names an agent JobSpec.
pub fn hop_job_spec_must_be_none(spec: Option<&tetonic_domain::AgentJobSpec>) -> bool {
    spec.is_none()
}

/// Production hop class: Infer hops live on Runs with `job_spec: None`.
pub fn hop_run_classified(snapshot: &RunSnapshot) -> bool {
    hop_job_spec_must_be_none(snapshot.job_spec.as_ref())
}

fn refuse_unclassified_hop(snapshot: &RunSnapshot) -> Result<(), RunSupervisorError> {
    if hop_run_classified(snapshot) {
        Ok(())
    } else {
        Err(RunSupervisorError::InvalidTransition(format!(
            "hop command refused: run {} is not hop-classified",
            snapshot.run_id.0
        )))
    }
}

/// Snapshot check used by broker before leasing a hop attempt.
pub fn hop_task_leaseable(state: &TaskState, speculation_allowed: bool) -> bool {
    *state == TaskState::Ready
        || *state == TaskState::Leased
        || (speculation_allowed && *state == TaskState::Running)
}
