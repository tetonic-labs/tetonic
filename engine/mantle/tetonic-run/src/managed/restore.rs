//! Reattach a quiescent saved root, without creating a run, attempt, or allowance.
//! Team subtree restoration requires a durable delegation contract and is denied here.
use super::lifetime::{unix_now, AttemptClock};
use super::*;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use tetonic_domain::{AttemptState, RunState, TaskState};
use tetonic_memory::RecoverMutex;

impl ManagedRunService {
    /// Trusted composition must rebuild the pinned harness and current authority.
    /// Receipt IDs alone are never execution permission. The existing executor
    /// performs its full conformance checks and claims a new lease only on wake.
    pub async fn restore_suspended_root(
        &self,
        receipt: ActivationReceipt,
        cmd: StartIdentityJobCommand,
        agent: tetonic_core::Agent,
        context: AdmissionContext,
        finalization: Option<FinalizationPolicy>,
    ) -> Result<ManagedSubmission, ManagedRunError> {
        self.validate_start(&cmd, &agent)?;
        let deny = || {
            ManagedRunError::InvalidRequest(
                "saved root execution cannot be restored with this binding".into(),
            )
        };
        if context.session_id.is_some() || context.delegation_parent.is_some() {
            return Err(deny());
        }
        // Fail before any state mutation if this host cannot own the task.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tokio::task::spawn_local(async {})
        }))
        .map_err(|_| deny())?
        .abort();
        let _gate = self.admission_gate.lock().await;
        let authorization = context.authorization.ok_or_else(deny)?;
        let activation = context.activation.ok_or_else(deny)?;
        if self
            .lookup_activation(
                &authorization,
                &activation,
                &cmd.identity,
                &cmd.job_spec,
                agent.execution_role(),
            )
            .await?
            .as_ref()
            != Some(&receipt)
        {
            return Err(deny());
        }
        let snapshot = self.inspect_run(&receipt.run_id).await?;
        let task = snapshot.tasks.get(&receipt.task_id).ok_or_else(deny)?;
        if snapshot.state != RunState::Active
            || snapshot.cancellation.run_canceled
            || snapshot.tasks.len() != 1
            || task.state != TaskState::Parked
            || task.binding.job_spec.as_ref() != Some(&cmd.job_spec)
            || task.binding.execution_scope.as_ref() != Some(&authorization.scope)
            || task.binding.execution_grant_id != authorization.grant_id
            || task.binding.job_role.as_deref() != agent.execution_role()
        {
            return Err(deny());
        }
        let attempt = task
            .active_attempt
            .as_ref()
            .and_then(|id| snapshot.attempts.get(id))
            .ok_or_else(deny)?;
        let suspension = attempt.suspension.clone().ok_or_else(deny)?;
        if attempt.state != AttemptState::Suspended
            || !attempt.execution_quiesced
            || !attempt.execution_claimed
            || suspension.reason != tetonic_domain::SuspensionReason::HumanInput
        {
            return Err(deny());
        }
        if self.binding(&attempt.attempt_id).is_some() {
            return Ok(ManagedSubmission::Existing(receipt));
        }
        let binding = Box::new(ManagedBinding {
            execution_scope: Some(authorization.scope.clone()),
            session_id: None,
            run_id: receipt.run_id,
            task_id: receipt.task_id,
            attempt_id: attempt.attempt_id.clone(),
            job_spec: cmd.job_spec.clone(),
        });
        let checkpoint = self
            .read_wait_checkpoint(&binding, &suspension.checkpoint)
            .await?;
        if checkpoint.invocation != cmd.invocation || !checkpoint.matches_agent(&agent) {
            return Err(deny());
        }
        (self.execution_policy)(
            Some(&cmd.identity),
            &cmd.job_spec,
            agent.execution_role(),
            &agent.advertised_tool_names(),
            &cmd.invocation,
        )
        .map_err(|_| deny())?;
        authorization
            .authority
            .authorize(&authorization.scope, &cmd.identity, &cmd.job_spec)
            .await
            .map_err(|_| deny())?;
        let lease = attempt.lease.as_ref().ok_or_else(deny)?;
        let ticket = self.reserve_dispatch();
        let scope = tetonic_domain::work_scope::WorkScope::default();
        if !scope.park() {
            return Err(deny());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.active.lock_recover().insert(
            attempt.attempt_id.clone(),
            ActiveAttempt {
                delegation_closed: Arc::new(AtomicBool::new(false)),
                clock: Arc::new(Mutex::new(AttemptClock {
                    deadline: task.binding.deadline,
                    instant: task.binding.deadline.map(|deadline| {
                        tokio::time::Instant::now()
                            + std::time::Duration::from_secs(deadline.saturating_sub(unix_now()))
                    }),
                    suspension: Some(suspension),
                })),
                work_scope: scope,
                binding: (*binding).clone(),
                identity: cmd.identity.clone(),
                execution_policy: self.execution_policy.clone(),
                authorization: Some(authorization),
                role: agent.execution_role().map(str::to_owned),
                parent_attempt: None,
                task_handle: None,
                heartbeat_cancel: cancel.clone(),
                heartbeat_sequence: lease.last_heartbeat_sequence,
                lease_proof: tetonic_domain::LeaseProof {
                    lease_id: lease.lease_id.clone(),
                    lease_epoch: lease.lease_epoch,
                    holder: lease.holder.clone(),
                },
                sequence: snapshot.sequence,
            },
        );
        self.attempt_dispatches
            .lock_recover()
            .insert(attempt.attempt_id.clone(), ticket.id.clone());
        self.dispatches.lock_recover().insert(
            ticket.id.clone(),
            DispatchEntry::Admitted {
                attempt_id: attempt.attempt_id.clone(),
                binding: (*binding).clone(),
                task: None,
                canceled: false,
            },
        );
        let completion = self.arm_attempt_join(&attempt.attempt_id);
        let this = self.clone();
        let owned_binding = binding.clone();
        let id = ticket.id.clone();
        self.spawn_dispatch(&ticket.id, async move {
            this.drive_submission(owned_binding, cmd, agent, finalization, true, id)
                .await;
        })?;
        self.spawn_heartbeat_driver(attempt.attempt_id.clone(), cancel);
        Ok(ManagedSubmission::Started {
            binding,
            completion,
        })
    }
}
