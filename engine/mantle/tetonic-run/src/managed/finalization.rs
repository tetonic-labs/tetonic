//! Managed Attempt finalization, artifact sealing, and terminal state resolution.

use super::attestation::{encode_candidate_bytes, seal_output_set};
use super::contracts::*;
use super::lifetime::unix_now;
use crate::command_envelope;
use std::sync::atomic::Ordering;
use tetonic_domain::{
    AcceptArtifact, ArtifactRef, CandidateOutcome, ClaimFinalization, CompleteAttempt, FailAttempt,
    FailureClass, FinishRun, RecordSideEffectCommit, RunCommand, RunFinishOutcome,
};
use tetonic_memory::RecoverMutex;

impl super::service::ManagedRunService {
    pub async fn finalize(&self, job: FinalizeJob) -> Result<CandidateOutcome, ManagedRunError> {
        let active = self
            .active
            .lock_recover()
            .get(&job.attempt)
            .cloned()
            .ok_or_else(|| ManagedRunError::InvalidRequest("no active attempt found".into()))?;

        if job.finish_run && active.parent_attempt.is_some() {
            return Err(ManagedRunError::InvalidRequest(
                "child finalizer cannot finish the parent run".into(),
            ));
        }
        // Keep the effect owner alive at the deadline. Cancellation closes work
        // admission and reaches cooperative workers; it is not quiescence.
        let finish_run = job.finish_run;
        let mut result = {
            let finalizing = self.finalize_owned(job, active.clone());
            tokio::pin!(finalizing);
            tokio::select! {
                biased;
                result = &mut finalizing => result,
                _ = active.wait_for_deadline() => {
                    active.work_scope.cancel();
                    self.notify_hooks(|hooks| hooks.fail_approval_waits(&active.binding.attempt_id));
                    finalizing.await
                }
            }
        };
        if result.is_err()
            && active.deadline_elapsed()
            && self.binding(&active.binding.attempt_id).is_some()
        {
            // The finalizer has dropped its own lease and joined its workers.
            // Preserve already-completed results and publication recovery errors.
            let snapshot = self.inspect_run(&active.binding.run_id).await?;
            if snapshot
                .attempts
                .get(&active.binding.attempt_id)
                .is_some_and(|attempt| attempt.state != tetonic_domain::AttemptState::Succeeded)
                && !snapshot.cancellation.run_canceled
            {
                while !active.work_scope.is_quiescent() {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                result = self.finish_deadline_exceeded(&active, finish_run).await;
            }
        }
        if let Ok(outcome) = &result {
            // finalize_owned has returned and dropped its finalization lease.
            // Record completion of actual workers before returning admission
            // capacity or notifying consumers of terminal completion.
            self.record_quiescence(&active).await?;
            self.deliver_terminal(&active, outcome.clone());
        }
        result
    }

    async fn finalize_owned(
        &self,
        job: FinalizeJob,
        active: super::lifetime::ActiveAttempt,
    ) -> Result<CandidateOutcome, ManagedRunError> {
        if active.deadline_elapsed() {
            active.work_scope.cancel();
            while !active.work_scope.is_quiescent() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            return self.finish_deadline_exceeded(&active, job.finish_run).await;
        }

        // Own the finalization lifetime as well as each blocking worker. If the
        // caller is dropped, the worker lease still prevents premature release.
        let finalization_lease = active.work_scope.try_enter();
        if finalization_lease.is_none() {
            while !active.work_scope.is_quiescent() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            if !matches!(job.outcome, CandidateOutcome::Canceled { .. }) {
                return Err(ManagedRunError::InvalidRequest(
                    "attempt canceled before finalization".into(),
                ));
            }
        }

        let run_id = active.binding.run_id.clone();
        let task_id = active.binding.task_id.clone();
        let attempt_id = active.binding.attempt_id.clone();
        let seq = self.current_sequence(&run_id).await;

        let mut seq = seq;
        if matches!(job.outcome, CandidateOutcome::Completed { .. }) {
            if let Some(denied) = self
                .deny_revoked_finalization(&active, job.finish_run)
                .await?
            {
                return Ok(denied);
            }

            self.heartbeat(&attempt_id).await?;
            let claim = self
                .supervisor
                .handle(RunCommand::ClaimFinalization(ClaimFinalization {
                    envelope: command_envelope(
                        format!("claim_finalization:{attempt_id}"),
                        None,
                        "lokai-manager",
                    ),
                    run_id: run_id.clone(),
                    attempt_id: attempt_id.clone(),
                    task_id: task_id.clone(),
                    task_version: 1,
                    input_digest: active.binding.job_spec.input_digest.clone(),
                    lease_proof: active.lease_proof.clone(),
                }))
                .await;
            let claim = match claim {
                Ok(claim) => claim,
                Err(error) => {
                    if active.deadline_elapsed() {
                        return self.finish_deadline_exceeded(&active, job.finish_run).await;
                    }
                    // A different winner retains the run. Only this losing
                    // attempt is failed, and no effect driver is invoked.
                    let message = format!("lease lost or competing claim: {error}");
                    self.fail_and_finish(
                        &active,
                        self.current_sequence(&run_id).await,
                        FailureClass::PolicyDenied,
                        &message,
                        false,
                    )
                    .await?;
                    let outcome = CandidateOutcome::Failed { message };
                    return Ok(outcome);
                }
            };
            if claim.idempotent_replay {
                return Err(ManagedRunError::InvalidRequest(
                    "finalization already claimed".into(),
                ));
            }
            seq = claim.sequence;
        }

        let final_outcome = match &job.outcome {
            CandidateOutcome::Completed { .. } => {
                // If verify command configured, run it
                let mut verify_error = None;
                if let Some(policy) = &job.policy {
                    if let (Some(driver), Some(cmd)) = (&policy.effect_driver, &policy.verify_cmd) {
                        driver
                            .bind_effect_identity(&task_id, &attempt_id)
                            .map_err(ManagedRunError::InvalidRequest)?;
                        let driver = driver.clone();
                        let cmd = cmd.clone();
                        let lease = active.work_scope.try_enter().ok_or_else(|| {
                            ManagedRunError::InvalidRequest(
                                "attempt canceled before verification".into(),
                            )
                        })?;
                        let scope = active.work_scope.clone();
                        if let Some(denied) = self
                            .deny_revoked_finalization(&active, job.finish_run)
                            .await?
                        {
                            return Ok(denied);
                        }
                        let verified = tokio::task::spawn_blocking(move || {
                            let _lease = lease;
                            if scope.is_canceled() {
                                return Err((
                                    "attempt canceled before verification started".into(),
                                    None,
                                ));
                            }
                            driver.run_verify(&cmd, &scope.cancellation_signal())
                        })
                        .await
                        .map_err(|e| ManagedRunError::InternalViolation(e.to_string()))?;
                        if active.work_scope.is_canceled() {
                            return Err(ManagedRunError::InvalidRequest(
                                "attempt canceled during verification".into(),
                            ));
                        }
                        if let Err((err, out)) = verified {
                            verify_error = Some(format!(
                                "verification command failed: {err} {}",
                                out.unwrap_or_default()
                            ));
                        }
                    }
                }

                if let Some(err_msg) = verify_error {
                    self.fail_and_finish(
                        &active,
                        seq,
                        FailureClass::VerificationFailed,
                        &err_msg,
                        job.finish_run,
                    )
                    .await?;
                    CandidateOutcome::Failed { message: err_msg }
                } else {
                    // Commit workspace effects if configured
                    if let Some(policy) = &job.policy {
                        if let Some(driver) = &policy.effect_driver {
                            if policy.verify_cmd.is_none() {
                                driver
                                    .bind_effect_identity(&task_id, &attempt_id)
                                    .map_err(ManagedRunError::InvalidRequest)?;
                            }
                            self.heartbeat(&attempt_id).await?;
                            let snapshot = self.inspect_run(&run_id).await?;
                            if snapshot.cancellation.run_canceled || self.is_canceled(&attempt_id) {
                                return Err(ManagedRunError::InvalidRequest(
                                    "attempt canceled before effects".into(),
                                ));
                            }
                            let driver = driver.clone();
                            let lease = active.work_scope.try_enter().ok_or_else(|| {
                                ManagedRunError::InvalidRequest(
                                    "attempt canceled before commit".into(),
                                )
                            })?;
                            let scope = active.work_scope.clone();
                            if let Some(denied) = self
                                .deny_revoked_finalization(&active, job.finish_run)
                                .await?
                            {
                                return Ok(denied);
                            }
                            let committed = tokio::task::spawn_blocking(move || {
                                let _lease = lease;
                                if scope.is_canceled() {
                                    return Err("attempt canceled before commit started".into());
                                }
                                driver.commit_workspace()
                            })
                            .await
                            .map_err(|e| e.to_string())
                            .and_then(|r| r);
                            let committed = match committed {
                                Ok(result) => result,
                                Err(error) => {
                                    self.fail_and_finish(
                                        &active,
                                        self.current_sequence(&run_id).await,
                                        FailureClass::PermanentExecutionFailure,
                                        &error,
                                        job.finish_run,
                                    )
                                    .await?;
                                    let outcome = CandidateOutcome::Failed { message: error };
                                    return Ok(outcome);
                                }
                            };
                            if let Some(commit) = committed {
                                let _recorded = self
                                    .supervisor
                                    .handle(RunCommand::RecordSideEffectCommit(
                                        RecordSideEffectCommit {
                                            envelope: command_envelope(
                                                format!("side_effect:{}", attempt_id),
                                                None,
                                                "lokai-manager",
                                            ),
                                            run_id: run_id.clone(),
                                            task_id: task_id.clone(),
                                            operation_key: format!("txn:{}", commit.transaction_id),
                                            transaction_id: Some(commit.transaction_id.clone()),
                                            committed_at: unix_now(),
                                        },
                                    ))
                                    .await
                                    .map_err(|e| {
                                        ManagedRunError::PersistenceFailed(e.to_string())
                                    })?;
                            }
                        }
                    }

                    if let Some(denied) = self
                        .deny_revoked_finalization(&active, job.finish_run)
                        .await?
                    {
                        return Ok(denied);
                    }
                    // Attestation & sealing
                    let bytes = encode_candidate_bytes(&job.outcome)?;
                    let sealed =
                        seal_output_set(&self.artifacts, &run_id, &task_id, &attempt_id, bytes)
                            .await?;

                    if let Some(denied) = self
                        .deny_revoked_finalization(&active, job.finish_run)
                        .await?
                    {
                        return Ok(denied);
                    }
                    if let Some(authorization) = &active.authorization {
                        let scope = authorization.scope.clone();
                        let artifact_id = sealed.artifact_id.0.clone();
                        let store = self.store.as_ref().ok_or_else(|| {
                            ManagedRunError::InternalViolation(
                                "scoped output requires durable storage".into(),
                            )
                        })?;
                        let bound = store
                            .write(move |db| {
                                db.bind_new_context_artifact(
                                    &scope.principal_id,
                                    &scope.information_context_id,
                                    &artifact_id,
                                )
                            })
                            .await;
                        if !matches!(bound, Ok(Ok(()))) {
                            let message = "output context binding failed".to_string();
                            self.fail_and_finish(
                                &active,
                                seq,
                                FailureClass::PolicyDenied,
                                &message,
                                job.finish_run,
                            )
                            .await?;
                            let outcome = CandidateOutcome::Failed { message };
                            return Ok(outcome);
                        }
                    }
                    let _complete_res = self
                        .supervisor
                        .handle(RunCommand::CompleteAttempt(CompleteAttempt {
                            envelope: command_envelope(
                                format!("complete_att:{}", attempt_id),
                                None,
                                "lokai-manager",
                            ),
                            run_id: run_id.clone(),
                            attempt_id: attempt_id.clone(),
                            task_version: 1,
                            workspace_version: None,
                            input_digest: active.binding.job_spec.input_digest.clone(),
                            result_digest: sealed.digest.clone(),
                            lease_proof: active.lease_proof.clone(),
                        }))
                        .await
                        .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;

                    // Publish the artifact's accepted metadata before recording
                    // the run receipt. A failed receipt can leave a retained
                    // orphan; a receipt must never outrun required publication.
                    self.artifacts
                        .mark_accepted(&sealed.artifact_id)
                        .await
                        .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;

                    let _ = self
                        .supervisor
                        .handle(RunCommand::AcceptArtifact(AcceptArtifact {
                            envelope: command_envelope(
                                format!("accept_art:{}", attempt_id),
                                None,
                                "lokai-manager",
                            ),
                            run_id: run_id.clone(),
                            task_id: task_id.clone(),
                            attempt_id: attempt_id.clone(),
                            artifact: ArtifactRef {
                                artifact_id: sealed.artifact_id.to_string(),
                                digest: sealed.digest,
                            },
                        }))
                        .await
                        .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;

                    if job.finish_run {
                        let _ = self
                            .supervisor
                            .handle(RunCommand::FinishRun(FinishRun {
                                envelope: command_envelope(
                                    format!("finish_run:{}", run_id),
                                    None,
                                    "lokai-manager",
                                ),
                                run_id: run_id.clone(),
                                outcome: RunFinishOutcome::Succeeded,
                            }))
                            .await
                            .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;
                    }

                    job.outcome
                }
            }
            CandidateOutcome::Canceled { reason: _ } => {
                if !job.finish_run {
                    self.fail_and_finish(
                        &active,
                        self.current_sequence(&run_id).await,
                        FailureClass::PermanentExecutionFailure,
                        "child canceled",
                        false,
                    )
                    .await?;
                } else {
                    self.supervisor
                        .handle(RunCommand::CancelRun(tetonic_domain::CancelRun {
                            envelope: command_envelope(
                                format!("cancel_run:{run_id}"),
                                None,
                                "lokai-manager",
                            ),
                            run_id: run_id.clone(),
                        }))
                        .await
                        .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;
                }
                job.outcome
            }
            CandidateOutcome::Failed { message } => {
                self.fail_and_finish(
                    &active,
                    seq,
                    FailureClass::PermanentExecutionFailure,
                    message,
                    job.finish_run,
                )
                .await?;
                job.outcome
            }
            CandidateOutcome::Limited { message, .. } => {
                self.fail_and_finish(
                    &active,
                    seq,
                    FailureClass::PermanentExecutionFailure,
                    message,
                    job.finish_run,
                )
                .await?;
                job.outcome
            }
        };

        drop(finalization_lease);
        Ok(final_outcome)
    }

    pub(crate) async fn record_quiescence(
        &self,
        active: &super::lifetime::ActiveAttempt,
    ) -> Result<(), ManagedRunError> {
        // Closing first prevents a late tool from entering after the drain.
        active.work_scope.cancel();
        while !active.work_scope.is_quiescent() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        if active.authorization.is_none() {
            return Ok(());
        }
        let snapshot = self.inspect_run(&active.binding.run_id).await?;
        if !snapshot
            .tasks
            .values()
            .any(|t| t.binding.activation.is_some())
        {
            return Ok(());
        }
        self.supervisor
            .handle(RunCommand::RecordAttemptQuiescence(
                tetonic_domain::StartAttempt {
                    envelope: command_envelope(
                        format!("quiesced:{}", active.binding.attempt_id),
                        None,
                        "tetonic-manager",
                    ),
                    run_id: active.binding.run_id.clone(),
                    attempt_id: active.binding.attempt_id.clone(),
                    lease_proof: active.lease_proof.clone(),
                },
            ))
            .await
            .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;
        Ok(())
    }

    fn deliver_terminal(&self, active: &super::lifetime::ActiveAttempt, outcome: CandidateOutcome) {
        active.heartbeat_cancel.store(true, Ordering::Relaxed);
        if self
            .active
            .lock_recover()
            .remove(&active.binding.attempt_id)
            .is_none()
        {
            return;
        }
        let dispatch = self
            .attempt_dispatches
            .lock_recover()
            .remove(&active.binding.attempt_id);
        if let Some(dispatch) = dispatch {
            self.dispatches.lock_recover().remove(&dispatch);
        }
        let result = StartIdentityJobResult {
            run_id: active.binding.run_id.clone(),
            task_id: active.binding.task_id.clone(),
            attempt_id: active.binding.attempt_id.clone(),
            outcome,
        };
        self.notify_hooks(|hooks| {
            hooks.fail_approval_waits(&result.attempt_id);
            hooks.terminal(&result);
        });
        self.complete_attempt_join(result);
    }

    /// Revocation denies new finalization work, but terminal failure recording
    /// remains permitted so the attempt cannot be stranded as running.
    async fn deny_revoked_finalization(
        &self,
        active: &super::lifetime::ActiveAttempt,
        finish_run: bool,
    ) -> Result<Option<CandidateOutcome>, ManagedRunError> {
        if active.deadline_elapsed() {
            return self
                .finish_deadline_exceeded(active, finish_run)
                .await
                .map(Some);
        }
        let snapshot = self.inspect_run(&active.binding.run_id).await?;
        let task = snapshot
            .tasks
            .get(&active.binding.task_id)
            .ok_or_else(|| ManagedRunError::InternalViolation("durable task missing".into()))?;
        let binding_matches = task.binding.execution_scope.as_ref()
            == active.authorization.as_ref().map(|a| &a.scope)
            && task.binding.execution_grant_id.as_ref()
                == active
                    .authorization
                    .as_ref()
                    .and_then(|a| a.grant_id.as_ref())
            && task.binding.deadline == active.deadline;
        let allowed = if binding_matches {
            match &active.authorization {
                Some(auth) => auth
                    .authority
                    .authorize(&auth.scope, &active.identity, &active.binding.job_spec)
                    .await
                    .is_ok(),
                None => true,
            }
        } else {
            false
        };
        if active.deadline_elapsed() {
            return self
                .finish_deadline_exceeded(active, finish_run)
                .await
                .map(Some);
        }
        if allowed {
            return Ok(None);
        }
        let message = "execution authorization denied during finalization".to_string();
        self.fail_and_finish(active, 0, FailureClass::PolicyDenied, &message, finish_run)
            .await?;
        let outcome = CandidateOutcome::Failed { message };
        Ok(Some(outcome))
    }

    async fn finish_deadline_exceeded(
        &self,
        active: &super::lifetime::ActiveAttempt,
        finish_run: bool,
    ) -> Result<CandidateOutcome, ManagedRunError> {
        active.work_scope.cancel();
        let message = "execution deadline exceeded".to_string();
        self.fail_and_finish(active, 0, FailureClass::TimedOut, &message, finish_run)
            .await?;
        let outcome = CandidateOutcome::Failed { message };
        Ok(outcome)
    }

    async fn fail_and_finish(
        &self,
        active: &super::lifetime::ActiveAttempt,
        _seq: u64,
        failure_class: FailureClass,
        reason: &str,
        finish_run: bool,
    ) -> Result<(), ManagedRunError> {
        let _ = self
            .supervisor
            .handle(RunCommand::FailAttempt(FailAttempt {
                envelope: command_envelope(
                    format!("fail_att:{}", active.binding.attempt_id),
                    None,
                    "lokai-manager",
                ),
                run_id: active.binding.run_id.clone(),
                attempt_id: active.binding.attempt_id.clone(),
                timeout_kind: (failure_class == FailureClass::TimedOut)
                    .then_some(tetonic_domain::TimeoutKind::Task),
                failure_class,
                reason: reason.to_string(),
                lease_proof: Some(active.lease_proof.clone()),
            }))
            .await
            .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;

        if finish_run {
            let _ = self
                .supervisor
                .handle(RunCommand::FinishRun(FinishRun {
                    envelope: command_envelope(
                        format!("finish_run:{}", active.binding.run_id),
                        None,
                        "lokai-manager",
                    ),
                    run_id: active.binding.run_id.clone(),
                    outcome: RunFinishOutcome::Failed,
                }))
                .await
                .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;
        }
        Ok(())
    }
}
