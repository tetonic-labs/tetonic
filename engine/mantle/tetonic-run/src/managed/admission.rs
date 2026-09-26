//! Managed job admission, binding, and cancellation-safe registration.

use super::contracts::*;
use super::lifetime::{unix_now, ActiveAttempt, DispatchEntry};
use crate::command_envelope;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tetonic_domain::{
    AttemptId, CreateAttempt, CreateRun, ExecutionTargetId, LeaseAttempt, LeaseId, LeaseProof,
    RunCommand, RunId, StartAttempt, StartRun, TaskId, TaskInputBinding,
};
use tetonic_memory::RecoverMutex;

impl super::service::ManagedRunService {
    pub async fn admit(
        &self,
        id: &DispatchId,
        job: AdmitJob,
    ) -> Result<ManagedBinding, ManagedRunError> {
        self.admit_with_context(id, job, AdmissionContext::default())
            .await
    }

    pub async fn admit_with_context(
        &self,
        id: &DispatchId,
        job: AdmitJob,
        context: AdmissionContext,
    ) -> Result<ManagedBinding, ManagedRunError> {
        match self.admit_submission(id, job, context).await? {
            ManagedAdmission::Admitted(binding) => Ok(binding),
            ManagedAdmission::Existing(_) => Err(ManagedRunError::InvalidRequest(
                "activation already exists; use submission receipts".into(),
            )),
        }
    }

    pub async fn admit_submission(
        &self,
        id: &DispatchId,
        job: AdmitJob,
        context: AdmissionContext,
    ) -> Result<ManagedAdmission, ManagedRunError> {
        let this = self.clone();
        let id = id.clone();
        // This worker survives caller cancellation and completes durable registration.
        tokio::spawn(async move {
            let _gate = this.admission_gate.lock().await;
            let result = this.admit_owned(&id, job, context).await;
            if matches!(&result, Ok(ManagedAdmission::Existing(_))) {
                // A replay has no new effect owner. Legacy admission errors
                // leave ticket cleanup to their caller, as before.
                let _ = this.release_dispatch(&id).await;
            }
            result
        })
        .await
        .map_err(|e| ManagedRunError::InternalViolation(e.to_string()))?
    }

    async fn admit_owned(
        &self,
        id: &DispatchId,
        job: AdmitJob,
        context: AdmissionContext,
    ) -> Result<ManagedAdmission, ManagedRunError> {
        if job.identity.id != job.job_spec.identity_id
            || job.identity.bound_definition_digest != job.job_spec.definition_digest
        {
            return Err(ManagedRunError::InvalidRequest(
                "identity and job binding mismatch".into(),
            ));
        }
        super::activation::validate_activation(&context, &job)?;
        if let (Some(activation), Some(authorization)) =
            (&context.activation, &context.authorization)
        {
            if let Some(receipt) = self
                .lookup_activation(
                    authorization,
                    activation,
                    &job.identity,
                    &job.job_spec,
                    job.role.as_deref(),
                )
                .await?
            {
                return Ok(ManagedAdmission::Existing(receipt));
            }
        }
        let parent = job
            .parent_attempt
            .as_ref()
            .and_then(|parent| self.active.lock_recover().get(parent).cloned());
        let parent_deadline = parent.as_ref().and_then(|active| active.deadline);
        let deadline = match (context.deadline, parent_deadline) {
            (Some(requested), Some(parent)) => Some(requested.min(parent)),
            (requested, parent) => requested.or(parent),
        };
        let mut deadline_instant = deadline
            .map(|deadline| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| {
                        ManagedRunError::InvalidRequest("system clock precedes Unix epoch".into())
                    })?;
                let remaining = std::time::Duration::from_secs(deadline)
                    .checked_sub(now)
                    .filter(|remaining| !remaining.is_zero())
                    .ok_or_else(|| {
                        ManagedRunError::InvalidRequest("execution deadline has elapsed".into())
                    })?;
                tokio::time::Instant::now()
                    .checked_add(remaining)
                    .ok_or_else(|| {
                        ManagedRunError::InvalidRequest("execution deadline is out of range".into())
                    })
            })
            .transpose()?;
        if let Some(parent_instant) = parent.and_then(|active| active.deadline_instant) {
            deadline_instant = Some(
                deadline_instant.map_or(parent_instant, |instant| instant.min(parent_instant)),
            );
        }
        // Do not let a child silently drop or introduce authority. Governed
        // delegation requires its own inherited grant/budget contract.
        if let Some(parent) = &job.parent_attempt {
            let parent_scoped = self
                .active
                .lock_recover()
                .get(parent)
                .is_some_and(|active| active.authorization.is_some());
            if parent_scoped || context.authorization.is_some() {
                return Err(ManagedRunError::InvalidRequest(
                    "governed delegation is not configured".into(),
                ));
            }
        }
        if context.authorization.is_some() && context.session_id.is_some() {
            return Err(ManagedRunError::InvalidRequest(
                "governed session activation is not configured".into(),
            ));
        }
        if let Some(authorization) = &context.authorization {
            if authorization
                .grant_id
                .as_ref()
                .is_some_and(|id| id.trim().is_empty() || id.len() > 256 || id.contains('\0'))
            {
                return Err(ManagedRunError::InvalidRequest(
                    "execution authorization denied".into(),
                ));
            }
            let scope = &authorization.scope;
            if self.store.is_none()
                || [
                    &scope.principal_id,
                    &scope.organization_id,
                    &scope.information_context_id,
                ]
                .iter()
                .any(|id| id.trim().is_empty() || id.len() > 256 || id.contains('\0'))
            {
                return Err(ManagedRunError::InvalidRequest(
                    "execution authorization denied".into(),
                ));
            }
            authorization
                .authority
                .authorize(scope, &job.identity, &job.job_spec)
                .await
                .map_err(|_| {
                    ManagedRunError::InvalidRequest("execution authorization denied".into())
                })?;
        }
        // This admission path has no authenticated employee execution scope.
        // A caller-supplied correlation ID must not activate a scoped discussion.
        if let (Some(store), Some(session)) = (&self.store, &context.session_id) {
            let session = session.0.clone();
            store
                .read(move |db| db.require_legacy_session(&session))
                .await
                .map_err(|_| {
                    ManagedRunError::InvalidRequest(
                        "session is not available for legacy execution".into(),
                    )
                })?
                .map_err(|_| {
                    ManagedRunError::InvalidRequest(
                        "session is not available for legacy execution".into(),
                    )
                })?;
        }
        if let Some(store) = &self.store {
            let identity = job.identity.clone();
            store
                .write(move |db| crate::put_identity(db, &identity))
                .await
                .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?
                .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;
        }
        // 1. Check cancellation before durable work
        {
            let guard = self.dispatches.lock_recover();
            let entry = guard.get(id).ok_or_else(|| {
                ManagedRunError::InvalidRequest("dispatch ticket not found".into())
            })?;
            match entry {
                DispatchEntry::Pending { canceled: true, .. }
                | DispatchEntry::Admitted { canceled: true, .. } => {
                    return Err(ManagedRunError::InvalidRequest(
                        "dispatch canceled before admission".into(),
                    ));
                }
                DispatchEntry::Pending {
                    canceled: false, ..
                } => {}
                _ => {
                    return Err(ManagedRunError::InvalidRequest(
                        "dispatch already admitted or terminal".into(),
                    ))
                }
            }
        }

        // Optional pre-admission test barrier
        let pre_barrier = self.pre_admission_barrier.lock_recover().clone();
        if let Some(barrier) = pre_barrier {
            barrier.wait().await;
        }
        if deadline.is_some_and(|deadline| unix_now() >= deadline)
            || deadline_instant.is_some_and(|instant| tokio::time::Instant::now() >= instant)
        {
            return Err(ManagedRunError::InvalidRequest(
                "execution deadline has elapsed".into(),
            ));
        }

        let (run_id, task_id, attempt_id, seq, lease_proof) = if let Some(parent_attempt) =
            &job.parent_attempt
        {
            let parent_active = self
                .active
                .lock_recover()
                .get(parent_attempt)
                .cloned()
                .ok_or_else(|| {
                    ManagedRunError::InvalidRequest("parent attempt is not active".into())
                })?;
            let run_id = parent_active.binding.run_id;
            let current_seq = self.current_sequence(&run_id).await;
            let task_id = context
                .task_id
                .clone()
                .unwrap_or_else(|| TaskId::new(format!("task_spawn_{}", uuid::Uuid::new_v4())));
            let snapshot = self.inspect_run(&run_id).await?;
            if let Some(task) = snapshot.tasks.get(&task_id) {
                if task.binding.job_spec.as_ref() != Some(&job.job_spec)
                    || task.binding.job_role != job.role
                    || task.binding.deadline != deadline
                {
                    return Err(ManagedRunError::InvalidRequest(
                        "child task already bound to a different job or role".into(),
                    ));
                }
                let existing = {
                    self.active
                        .lock_recover()
                        .values()
                        .find(|a| a.binding.run_id == run_id && a.binding.task_id == task_id)
                        .map(|a| a.binding.clone())
                };
                if let Some(binding) = existing {
                    self.dispatches.lock_recover().remove(id);
                    return Ok(ManagedAdmission::Admitted(binding));
                }
                return Err(ManagedRunError::InvalidRequest(
                    "child delivery already admitted; retry requires a new authorized task".into(),
                ));
            }
            let attempt_id = AttemptId::new(format!("att_{}", uuid::Uuid::new_v4()));

            let added = self
                .supervisor
                .handle(RunCommand::AddTask(tetonic_domain::AddTask {
                    envelope: command_envelope(
                        format!("spawn_task_{attempt_id}"),
                        Some(current_seq),
                        "lokai-manager",
                    ),
                    run_id: run_id.clone(),
                    task_id: task_id.clone(),
                    binding: TaskInputBinding {
                        deadline,
                        execution_scope: context.authorization.as_ref().map(|a| a.scope.clone()),
                        execution_grant_id: context
                            .authorization
                            .as_ref()
                            .and_then(|a| a.grant_id.clone()),
                        job_spec: Some(job.job_spec.clone()),
                        job_role: job.role.clone(),
                        ..TaskInputBinding::default()
                    },
                }))
                .await
                .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;

            let delivery_key = format!("turn:{}:{}", run_id, attempt_id);
            let created = self
                .supervisor
                .handle(RunCommand::CreateAttempt(CreateAttempt {
                    envelope: command_envelope(
                        format!("child_attempt:{attempt_id}"),
                        Some(added.sequence),
                        "lokai-manager",
                    ),
                    run_id: run_id.clone(),
                    task_id: task_id.clone(),
                    attempt_id: attempt_id.clone(),
                    delivery_key: Some(delivery_key),
                }))
                .await
                .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;

            let issued_at = unix_now();
            let leased = self
                .supervisor
                .handle(RunCommand::LeaseAttempt(LeaseAttempt {
                    envelope: command_envelope(
                        format!("child_lease:{attempt_id}"),
                        Some(created.sequence),
                        "lokai-manager",
                    ),
                    run_id: run_id.clone(),
                    attempt_id: attempt_id.clone(),
                    lease_id: LeaseId::new(format!("lease_{attempt_id}")),
                    lease_epoch: 0,
                    holder: ExecutionTargetId::local(),
                    issued_at,
                    expires_at: issued_at.saturating_add(300),
                    heartbeat_interval_secs: 30,
                }))
                .await
                .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;

            let snap = &leased.snapshot;
            let attempt_rec = snap.attempts.get(&attempt_id).ok_or_else(|| {
                ManagedRunError::InvalidRequest("attempt missing after lease".into())
            })?;
            let lease = attempt_rec.lease.as_ref().ok_or_else(|| {
                ManagedRunError::InvalidRequest("lease missing after lease command".into())
            })?;
            let lease_proof = LeaseProof {
                lease_id: lease.lease_id.clone(),
                lease_epoch: lease.lease_epoch,
                holder: lease.holder.clone(),
            };

            let started = self
                .supervisor
                .handle(RunCommand::StartAttempt(StartAttempt {
                    envelope: command_envelope(
                        format!("child_start:{attempt_id}"),
                        Some(leased.sequence),
                        "lokai-manager",
                    ),
                    run_id: run_id.clone(),
                    attempt_id: attempt_id.clone(),
                    lease_proof: lease_proof.clone(),
                }))
                .await
                .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;

            (run_id, task_id, attempt_id, started.sequence, lease_proof)
        } else {
            let run_id = match (&context.activation, &context.authorization) {
                (Some(activation), Some(authorization)) => super::activation::activation_run_id(
                    &authorization.scope,
                    &activation.request_id,
                )?,
                _ => RunId::new(format!("run_{}", uuid::Uuid::new_v4())),
            };
            let task_id = TaskId::new(format!("task_root_{}", run_id));
            let attempt_id = AttemptId::new(format!("att_{}", uuid::Uuid::new_v4()));

            let created_run = self
                .supervisor
                .handle(RunCommand::CreateRun(CreateRun {
                    envelope: command_envelope("turn_create", None, "lokai-manager"),
                    session_id: context.session_id.clone(),
                    run_id: run_id.clone(),
                    root_task_id: task_id.clone(),
                    root_binding: TaskInputBinding {
                        activation: context.activation.clone(),
                        deadline,
                        execution_scope: context.authorization.as_ref().map(|a| a.scope.clone()),
                        execution_grant_id: context
                            .authorization
                            .as_ref()
                            .and_then(|a| a.grant_id.clone()),
                        job_spec: Some(job.job_spec.clone()),
                        job_role: job.role.clone(),
                        ..TaskInputBinding::default()
                    },
                    speculation: context.speculation.clone(),
                    job_spec: Some(job.job_spec.clone()),
                }))
                .await;
            // Two managers may race on the same database. The existing event
            // transaction makes creation exclusive; a replay never starts an
            // attempt, even if the first owner stopped midway through admission.
            let created_run = match created_run {
                Ok(created) if !created.idempotent_replay => created,
                other => {
                    if let (Some(activation), Some(authorization)) =
                        (&context.activation, &context.authorization)
                    {
                        if let Some(receipt) = self
                            .lookup_activation(
                                authorization,
                                activation,
                                &job.identity,
                                &job.job_spec,
                                job.role.as_deref(),
                            )
                            .await?
                        {
                            return Ok(ManagedAdmission::Existing(receipt));
                        }
                    }
                    if matches!(
                        other,
                        Err(tetonic_domain::RunSupervisorError::OrganizationCapacityExceeded)
                    ) {
                        return Err(ManagedRunError::OrganizationCapacityExceeded);
                    }
                    if matches!(
                        other,
                        Err(tetonic_domain::RunSupervisorError::TeamCapacityExceeded)
                    ) {
                        return Err(ManagedRunError::TeamCapacityExceeded);
                    }
                    if matches!(
                        other,
                        Err(tetonic_domain::RunSupervisorError::PrincipalCapacityExceeded)
                    ) {
                        return Err(ManagedRunError::PrincipalCapacityExceeded);
                    }
                    if matches!(
                        other,
                        Err(tetonic_domain::RunSupervisorError::ExecutionCapacityExceeded)
                    ) {
                        return Err(ManagedRunError::ExecutionCapacityExceeded);
                    }
                    return Err(ManagedRunError::PersistenceFailed(match other {
                        Err(error) => error.to_string(),
                        Ok(_) => "replayed activation is missing its durable binding".into(),
                    }));
                }
            };

            let started_run = self
                .supervisor
                .handle(RunCommand::StartRun(StartRun {
                    envelope: command_envelope(
                        "turn_start",
                        Some(created_run.sequence),
                        "lokai-manager",
                    ),
                    run_id: run_id.clone(),
                }))
                .await
                .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;

            let delivery_key = format!("turn:{}:{}", run_id, attempt_id);
            let created_att = self
                .supervisor
                .handle(RunCommand::CreateAttempt(CreateAttempt {
                    envelope: command_envelope(
                        "turn_attempt",
                        Some(started_run.sequence),
                        "lokai-manager",
                    ),
                    run_id: run_id.clone(),
                    task_id: task_id.clone(),
                    attempt_id: attempt_id.clone(),
                    delivery_key: Some(delivery_key),
                }))
                .await
                .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;

            let issued_at = unix_now();
            let leased = self
                .supervisor
                .handle(RunCommand::LeaseAttempt(LeaseAttempt {
                    envelope: command_envelope(
                        "turn_lease",
                        Some(created_att.sequence),
                        "lokai-manager",
                    ),
                    run_id: run_id.clone(),
                    attempt_id: attempt_id.clone(),
                    lease_id: LeaseId::new(format!("lease_{attempt_id}")),
                    lease_epoch: 0,
                    holder: ExecutionTargetId::local(),
                    issued_at,
                    expires_at: issued_at.saturating_add(300),
                    heartbeat_interval_secs: 30,
                }))
                .await
                .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;

            let snap = &leased.snapshot;
            let attempt_rec = snap.attempts.get(&attempt_id).ok_or_else(|| {
                ManagedRunError::InvalidRequest("attempt missing after lease".into())
            })?;
            let lease = attempt_rec.lease.as_ref().ok_or_else(|| {
                ManagedRunError::InvalidRequest("lease missing after lease command".into())
            })?;
            let lease_proof = LeaseProof {
                lease_id: lease.lease_id.clone(),
                lease_epoch: lease.lease_epoch,
                holder: lease.holder.clone(),
            };

            let started = self
                .supervisor
                .handle(RunCommand::StartAttempt(StartAttempt {
                    envelope: command_envelope(
                        "turn_running",
                        Some(leased.sequence),
                        "lokai-manager",
                    ),
                    run_id: run_id.clone(),
                    attempt_id: attempt_id.clone(),
                    lease_proof: lease_proof.clone(),
                }))
                .await
                .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;

            (run_id, task_id, attempt_id, started.sequence, lease_proof)
        };

        // Optional post-admission test barrier (proves cancel-during-admission handling)
        let post_barrier = self.post_admission_barrier.lock_recover().clone();
        if let Some(barrier) = post_barrier {
            barrier.wait().await;
        }

        let pause = self.post_admission_pause.lock_recover().clone();
        if let Some(notify) = pause {
            notify.notify_one();
        }
        let resume = self.post_admission_resume.lock_recover().clone();
        if let Some(notify) = resume {
            notify.notified().await;
        }

        let binding = ManagedBinding {
            execution_scope: context.authorization.as_ref().map(|a| a.scope.clone()),
            session_id: context.session_id.clone(),
            run_id: run_id.clone(),
            task_id: task_id.clone(),
            attempt_id: attempt_id.clone(),
            job_spec: job.job_spec,
        };
        let heartbeat_cancel = Arc::new(AtomicBool::new(false));
        let was_canceled = {
            // Cancellation and promotion share one critical section.
            let mut dispatches = self.dispatches.lock_recover();
            let (task, canceled) = match dispatches.get(id) {
                Some(DispatchEntry::Pending { task, canceled }) => (task.clone(), *canceled),
                _ => (None, true),
            };
            self.active.lock_recover().insert(
                attempt_id.clone(),
                ActiveAttempt {
                    deadline,
                    deadline_instant,
                    work_scope: Default::default(),
                    binding: binding.clone(),
                    identity: job.identity,
                    execution_policy: self.execution_policy.clone(),
                    authorization: context.authorization.clone(),
                    role: job.role,
                    parent_attempt: job.parent_attempt,
                    task_handle: task.clone(),
                    heartbeat_cancel: heartbeat_cancel.clone(),
                    heartbeat_sequence: 0,
                    lease_proof,
                    sequence: seq,
                },
            );
            self.attempt_dispatches
                .lock_recover()
                .insert(attempt_id.clone(), id.clone());
            dispatches.insert(
                id.clone(),
                DispatchEntry::Admitted {
                    attempt_id: attempt_id.clone(),
                    binding: binding.clone(),
                    task,
                    canceled,
                },
            );
            canceled
        };
        if was_canceled {
            let finish_run = self
                .active
                .lock_recover()
                .get(&attempt_id)
                .is_some_and(|a| a.parent_attempt.is_none());
            self.finalize(FinalizeJob {
                attempt: attempt_id.clone(),
                outcome: tetonic_domain::CandidateOutcome::Canceled {
                    reason: "canceled during admission".into(),
                },
                policy: None,
                finish_run,
            })
            .await?;
            return Err(ManagedRunError::InvalidRequest(
                "dispatch canceled during admission".into(),
            ));
        }
        self.notify_hooks(|hooks| hooks.started(&binding));

        // Spawn heartbeat driver
        self.spawn_heartbeat_driver(attempt_id.clone(), heartbeat_cancel);

        Ok(ManagedAdmission::Admitted(binding))
    }

    pub(crate) async fn current_sequence(&self, run_id: &RunId) -> u64 {
        self.supervisor
            .snapshot(run_id.clone())
            .await
            .map(|s| s.sequence)
            .unwrap_or(0)
    }
}

impl super::service::ManagedRunService {
    /// Create an unstarted durable job; callers supply product policy and labels.
    pub async fn create_run(
        &self,
        session_id: Option<tetonic_domain::SessionId>,
        task_id: Option<TaskId>,
        identity: Option<tetonic_domain::AgentIdentity>,
        job_spec: Option<tetonic_domain::AgentJobSpec>,
    ) -> Result<RunId, ManagedRunError> {
        if let (Some(identity), Some(spec)) = (&identity, &job_spec) {
            if identity.id != spec.identity_id
                || identity.bound_definition_digest != spec.definition_digest
            {
                return Err(ManagedRunError::InvalidRequest(
                    "identity/job binding mismatch".into(),
                ));
            }
        }
        if let (Some(store), Some(identity)) = (&self.store, identity) {
            store
                .write(move |db| crate::put_identity(db, &identity))
                .await
                .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?
                .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;
        }
        let run_id = RunId::new(format!("run_{}", uuid::Uuid::new_v4()));
        self.supervisor
            .handle(RunCommand::CreateRun(CreateRun {
                envelope: command_envelope(format!("create:{run_id}"), None, "lokai-manager"),
                root_task_id: task_id.unwrap_or_else(|| TaskId::new(format!("task_root_{run_id}"))),
                run_id: run_id.clone(),
                session_id,
                root_binding: TaskInputBinding::default(),
                speculation: None,
                job_spec,
            }))
            .await
            .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;
        Ok(run_id)
    }
}
