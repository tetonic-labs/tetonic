//! Managed Attempt execution and identity job runners.

use super::contracts::*;
use tetonic_domain::{AgentAttemptExecutor, CandidateOutcome};
use tetonic_memory::RecoverMutex;

impl super::service::ManagedRunService {
    pub async fn execute_attempt(
        &self,
        attempt: tetonic_domain::AttemptId,
        agent: &mut tetonic_core::Agent,
        conversation: &mut tetonic_core::Conversation,
        invocation: tetonic_domain::AgentInvocation,
        on_step: &mut (dyn FnMut(tetonic_core::Step) + Send),
    ) -> CandidateOutcome {
        let fail = |message: String| CandidateOutcome::Failed { message };
        let Some(active) = self.active.lock_recover().get(&attempt).cloned() else {
            return fail("attempt has no active binding".into());
        };
        let binding = active.binding.clone();
        let snapshot = match self.inspect_run(&binding.run_id).await {
            Ok(snapshot) => snapshot,
            Err(error) => return fail(error.to_string()),
        };
        let Some(task) = snapshot.tasks.get(&binding.task_id) else {
            return fail("durable task missing".into());
        };
        if task.binding.job_spec.as_ref() != Some(&binding.job_spec)
            || binding.job_spec.input_digest != crate::job_input_digest(&invocation.user_input)
            || active.identity.id != binding.job_spec.identity_id
            || active.identity.bound_definition_digest != binding.job_spec.definition_digest
        {
            return fail("identity/job/invocation does not match durable binding".into());
        }
        if agent.bound_attempt_id().is_some_and(|id| id != attempt.0)
            || active
                .role
                .as_deref()
                .is_some_and(|role| Some(role) != agent.execution_role())
        {
            return fail("agent attempt or role binding mismatch".into());
        }
        if invocation.max_steps > agent.execution_step_limit() {
            return fail("invocation exceeds configured execution step limit".into());
        }
        if let Some(store) = &self.store {
            let id = binding.job_spec.identity_id.clone();
            let digest = binding.job_spec.definition_digest.clone();
            match store
                .read(move |db| crate::get_identity_revision(db, &id, &digest))
                .await
            {
                Ok(Ok(Some(identity))) if identity == active.identity => {}
                _ => return fail("durable identity missing or changed".into()),
            }
        }
        let advertised = agent.advertised_tool_names();
        if let Err(error) = (active.execution_policy)(
            Some(&active.identity),
            &binding.job_spec,
            active.role.as_deref(),
            &advertised,
            &invocation,
        ) {
            return fail(error);
        }
        if binding
            .job_spec
            .capability_bindings
            .iter()
            .any(|cap| !advertised.contains(cap) && !active.identity.context_bindings.contains(cap))
            || binding.job_spec.artifact_bindings.iter().any(|artifact| {
                !task
                    .binding
                    .input_artifacts
                    .iter()
                    .any(|a| &a.artifact_id == artifact)
            })
        {
            return fail("unresolved capability or artifact binding".into());
        }
        if snapshot.cancellation.run_canceled || self.is_canceled(&attempt) {
            return CandidateOutcome::Canceled {
                reason: "attempt canceled before execution".into(),
            };
        }
        let claimed = self
            .supervisor
            .handle(tetonic_domain::RunCommand::ClaimExecution(
                tetonic_domain::StartAttempt {
                    envelope: crate::command_envelope(
                        format!("execute:{attempt}"),
                        None,
                        "lokai-manager",
                    ),
                    run_id: binding.run_id.clone(),
                    attempt_id: attempt.clone(),
                    lease_proof: active.lease_proof,
                },
            ))
            .await;
        if let Err(error) = claimed {
            return fail(error.to_string());
        }
        if let Err(error) = agent.bind_work_scope(active.work_scope.clone()) {
            return fail(error.to_string());
        }
        agent.stamp_managed_run(&binding.run_id.0, &binding.task_id.0, &attempt.0);
        let loop_cancel = conversation.cancel_handle();
        let mut step_fn = |step: tetonic_core::Step| {
            self.notify_hooks(|hooks| hooks.step(&binding, &step));
            on_step(step);
        };

        let mut executor =
            tetonic_runtime::LocalAgentAttemptExecutor::new(agent, conversation, &mut step_fn);

        let ctx = tetonic_domain::AttemptExecutionContext {
            attempt_id: binding.attempt_id.clone(),
        };

        let canceled = async {
            loop {
                if active.work_scope.is_canceled()
                    || self.is_canceled(&attempt)
                    || loop_cancel.load(std::sync::atomic::Ordering::SeqCst)
                {
                    active.work_scope.cancel();
                    loop_cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        };
        let outcome = tokio::select! {
            biased;
            _ = canceled => CandidateOutcome::Canceled { reason: "attempt canceled during execution".into() },
            outcome = executor.execute(invocation, ctx) => outcome,
        };

        // Dropping the executor may detach Tokio blocking jobs. Their leases
        // remain in the worker closures until actual completion (including unwind).
        while !active.work_scope.is_quiescent() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        if active.work_scope.is_canceled() || self.is_canceled(&attempt) {
            CandidateOutcome::Canceled {
                reason: "attempt canceled during execution".into(),
            }
        } else {
            outcome
        }
    }

    pub async fn start_identity_job(
        &self,
        cmd: StartIdentityJobCommand,
        agent: &mut tetonic_core::Agent,
    ) -> Result<StartIdentityJobResult, ManagedRunError> {
        self.validate_start(&cmd, agent)?;
        let ticket = self.reserve_dispatch();
        let admit_job = AdmitJob {
            identity: cmd.identity,
            job_spec: cmd.job_spec,
            role: agent.execution_role().map(str::to_owned),
            parent_attempt: None,
        };
        let binding = self.admit(&ticket.id, admit_job).await?;
        let mut conversation = tetonic_core::Conversation::new();
        let outcome = self
            .execute_attempt(
                binding.attempt_id.clone(),
                agent,
                &mut conversation,
                cmd.invocation,
                &mut |_| {},
            )
            .await;

        let final_outcome = if self.binding(&binding.attempt_id).is_none()
            && self
                .inspect_run(&binding.run_id)
                .await?
                .cancellation
                .run_canceled
        {
            CandidateOutcome::Canceled {
                reason: "run canceled".into(),
            }
        } else {
            self.finalize(FinalizeJob {
                attempt: binding.attempt_id.clone(),
                outcome,
                policy: None,
                finish_run: true,
            })
            .await
            .unwrap_or_else(|error| {
                let outcome = CandidateOutcome::Failed {
                    message: format!("managed finalization failed: {error}"),
                };
                let result = StartIdentityJobResult {
                    run_id: binding.run_id.clone(),
                    task_id: binding.task_id.clone(),
                    attempt_id: binding.attempt_id.clone(),
                    outcome: outcome.clone(),
                };
                self.notify_hooks(|hooks| hooks.terminal(&result));
                outcome
            })
        };

        let _ = self.release_dispatch(&ticket.id).await;

        Ok(StartIdentityJobResult {
            run_id: binding.run_id,
            task_id: binding.task_id,
            attempt_id: binding.attempt_id,
            outcome: final_outcome,
        })
    }

    pub async fn submit_identity_job(
        &self,
        cmd: StartIdentityJobCommand,
        mut agent: tetonic_core::Agent,
    ) -> Result<
        (
            tetonic_domain::AttemptId,
            tokio::sync::oneshot::Receiver<StartIdentityJobResult>,
        ),
        ManagedRunError,
    > {
        self.validate_start(&cmd, &agent)?;
        // Verify executor context before creating any durable records.
        let probe = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tokio::task::spawn_local(async {})
        }))
        .map_err(|_| {
            ManagedRunError::InvalidRequest("submit_identity_job requires a Tokio LocalSet".into())
        })?;
        probe.abort();
        let ticket = self.reserve_dispatch();
        let admit_job = AdmitJob {
            identity: cmd.identity,
            job_spec: cmd.job_spec,
            role: agent.execution_role().map(str::to_owned),
            parent_attempt: None,
        };
        let binding = self.admit(&ticket.id, admit_job).await?;
        let attempt_id = binding.attempt_id.clone();
        let rx = self.arm_attempt_join(&attempt_id);

        let this = self.clone();
        let att = attempt_id.clone();
        let t_id = ticket.id.clone();
        let fut = async move {
            let mut conversation = tetonic_core::Conversation::new();
            let outcome = this
                .execute_attempt(
                    att.clone(),
                    &mut agent,
                    &mut conversation,
                    cmd.invocation,
                    &mut |_| {},
                )
                .await;
            let finalized = this
                .finalize(FinalizeJob {
                    attempt: att,
                    outcome,
                    policy: None,
                    finish_run: true,
                })
                .await;
            if let Err(error) = finalized {
                // Deliver an explicit failure, retaining durable recovery state.
                let result = StartIdentityJobResult {
                    run_id: binding.run_id,
                    task_id: binding.task_id,
                    attempt_id: binding.attempt_id,
                    outcome: CandidateOutcome::Failed {
                        message: error.to_string(),
                    },
                };
                this.notify_hooks(|hooks| hooks.terminal(&result));
                this.complete_attempt_join(result);
            }
            let _ = this.release_dispatch(&t_id).await;
        };

        let task = tokio::task::spawn_local(fut);
        self.attach_task(&ticket.id, task.abort_handle())?;
        Ok((attempt_id, rx))
    }
}

impl super::service::ManagedRunService {
    fn validate_start(
        &self,
        cmd: &StartIdentityJobCommand,
        agent: &tetonic_core::Agent,
    ) -> Result<(), ManagedRunError> {
        if cmd.identity.id != cmd.job_spec.identity_id
            || cmd.identity.bound_definition_digest != cmd.job_spec.definition_digest
            || cmd.job_spec.input_digest != crate::job_input_digest(&cmd.invocation.user_input)
            || agent.bound_attempt_id().is_some()
        {
            return Err(ManagedRunError::InvalidRequest(
                "identity/definition/input/Attempt binding mismatch".into(),
            ));
        }
        let advertised = agent.advertised_tool_names();
        if cmd
            .job_spec
            .capability_bindings
            .iter()
            .any(|cap| !advertised.contains(cap) && !cmd.identity.context_bindings.contains(cap))
            || !cmd.job_spec.artifact_bindings.is_empty()
        {
            return Err(ManagedRunError::InvalidRequest(
                "unresolved capability or artifact binding".into(),
            ));
        }
        Ok(())
    }
}
