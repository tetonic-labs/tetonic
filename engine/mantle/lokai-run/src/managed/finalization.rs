//! Managed Attempt finalization, artifact sealing, and terminal state resolution.

use super::attestation::{encode_candidate_bytes, seal_output_set};
use super::contracts::*;
use super::lifetime::unix_now;
use crate::command_envelope;
use lokai_domain::{
    AcceptArtifact, ArtifactRef, CandidateOutcome, ClaimFinalization, CompleteAttempt, FailAttempt,
    FailureClass, FinishRun, RecordSideEffectCommit, RunCommand, RunFinishOutcome,
};
use lokai_memory::RecoverMutex;
use std::sync::atomic::Ordering;

impl super::service::ManagedRunService {
    pub async fn finalize(&self, job: FinalizeJob) -> Result<CandidateOutcome, ManagedRunError> {
        let active = self
            .active
            .lock_recover()
            .get(&job.attempt)
            .cloned()
            .ok_or_else(|| ManagedRunError::InvalidRequest("no active attempt found".into()))?;

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

        if job.finish_run && active.parent_attempt.is_some() {
            return Err(ManagedRunError::InvalidRequest(
                "child finalizer cannot finish the parent run".into(),
            ));
        }
        let run_id = active.binding.run_id.clone();
        let task_id = active.binding.task_id.clone();
        let attempt_id = active.binding.attempt_id.clone();
        let seq = self.current_sequence(&run_id).await;

        let mut seq = seq;
        if matches!(job.outcome, CandidateOutcome::Completed { .. }) {
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
                    self.deliver_terminal(&active, outcome.clone());
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
                                    self.deliver_terminal(&active, outcome.clone());
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

                    // Attestation & sealing
                    let bytes = encode_candidate_bytes(&job.outcome)?;
                    let sealed =
                        seal_output_set(&self.artifacts, &run_id, &task_id, &attempt_id, bytes)
                            .await?;

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
                        .handle(RunCommand::CancelRun(lokai_domain::CancelRun {
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
        self.deliver_terminal(&active, final_outcome.clone());
        Ok(final_outcome)
    }

    fn deliver_terminal(&self, active: &super::lifetime::ActiveAttempt, outcome: CandidateOutcome) {
        active.heartbeat_cancel.store(true, Ordering::Relaxed);
        self.active
            .lock_recover()
            .remove(&active.binding.attempt_id);
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
                failure_class,
                reason: reason.to_string(),
                lease_proof: Some(active.lease_proof.clone()),
                timeout_kind: None,
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
