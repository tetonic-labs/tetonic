//! Checkpoint-backed waiting on the existing managed lifecycle and artifact store.
use super::{lifetime::unix_now, *};
use tetonic_domain::{ArtifactRef, AttemptId, RunCommand, StartAttempt};
use tetonic_memory::RecoverMutex;

const MAX_CHECKPOINT_BYTES: usize = 2 * 1024 * 1024;

fn unavailable() -> ManagedRunError {
    ManagedRunError::InvalidRequest("saved execution cannot be safely continued".into())
}

impl ManagedRunService {
    pub(crate) async fn suspend_at_boundary(
        &self,
        attempt: &AttemptId,
        checkpoint: &tetonic_core::WaitCheckpoint,
        reason: tetonic_domain::SuspensionReason,
    ) -> Result<(), ManagedRunError> {
        let _admission = self.admission_gate.lock().await;
        let _heartbeat = self.heartbeat_gate.lock().await;
        let active = self
            .active
            .lock_recover()
            .get(attempt)
            .cloned()
            .ok_or_else(unavailable)?;
        // The first durable contract covers a scoped root at a human handoff.
        // Team waits need durable delegation, not a resurrected parent lease.
        let snapshot = self.inspect_run(&active.binding.run_id).await?;
        if active.authorization.is_none()
            || active.parent_attempt.is_some()
            || snapshot.tasks.len() != 1
            || reason != tetonic_domain::SuspensionReason::HumanInput
        {
            return Err(unavailable());
        }
        checkpoint.validate().map_err(|_| unavailable())?;
        if checkpoint.pending.attempt_id.as_deref() != Some(attempt.0.as_str())
            || crate::job_input_digest(&checkpoint.invocation.user_input)
                != active.binding.job_spec.input_digest
            || active.work_scope.is_canceled()
            || self.is_canceled(attempt)
        {
            return Err(unavailable());
        }
        if let Some(saved) = active.suspension() {
            let old = self
                .read_wait_checkpoint(&active.binding, &saved.checkpoint)
                .await?;
            if serde_json::to_value(&old).ok() != serde_json::to_value(checkpoint).ok() {
                return Err(unavailable());
            }
            return Ok(());
        }
        if active.deadline_elapsed() {
            return Err(unavailable());
        }
        if let Some(auth) = &active.authorization {
            auth.authority
                .authorize(&auth.scope, &active.identity, &active.binding.job_spec)
                .await
                .map_err(|_| unavailable())?;
        }
        let bytes = serde_json::to_vec(checkpoint).map_err(|_| unavailable())?;
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(unavailable());
        }
        // This closes ordinary tool admission atomically with the quiescence check.
        if !active.work_scope.park() {
            return Err(unavailable());
        }
        let result = async {
            use tetonic_domain::artifact::{ArtifactDeclaration, ArtifactKind, RetentionPolicy};
            let mut writer = self
                .artifacts
                .begin_write(ArtifactDeclaration {
                    kind: ArtifactKind::ContextPack,
                    producer_run_id: active.binding.run_id.clone(),
                    producer_task_id: active.binding.task_id.clone(),
                    producer_attempt_id: attempt.clone(),
                    worker_id: None,
                    workspace_version: None,
                    data_class: tetonic_domain::DataClass::Secret,
                    retention_policy: RetentionPolicy::UntilRunCompletes,
                })
                .await
                .map_err(|_| unavailable())?;
            writer
                .write_chunk(&bytes)
                .await
                .map_err(|_| unavailable())?;
            let meta = writer.seal().await.map_err(|_| unavailable())?;
            tetonic_artifact::verify_stored_content(self.artifacts.as_ref(), &meta.artifact_id)
                .await
                .map_err(|_| unavailable())?;
            let result = self
                .supervisor
                .handle(RunCommand::SuspendAttempt(tetonic_domain::SuspendAttempt {
                    envelope: crate::command_envelope(
                        format!("suspend:{attempt}:{}", checkpoint.pending.call_id),
                        None,
                        "tetonic-runtime",
                    ),
                    run_id: active.binding.run_id.clone(),
                    attempt_id: attempt.clone(),
                    lease_proof: active.lease_proof.clone(),
                    reason,
                    checkpoint: ArtifactRef {
                        artifact_id: meta.artifact_id.0,
                        digest: format!("sha256:{}", meta.content_digest.0),
                    },
                }))
                .await
                .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;
            let suspension = result.snapshot.attempts[attempt]
                .suspension
                .clone()
                .ok_or_else(unavailable)?;
            active.clock.lock_recover().suspension = Some(suspension);
            Ok(())
        }
        .await;
        if result.is_err() {
            active.work_scope.cancel();
        }
        result
    }

    /// Verify a host handoff against the sealed pending call, not caller-supplied
    /// question text. Storage rechecks the same suspension before saving a receipt.
    pub async fn verify_human_handoff(
        &self,
        request: &tetonic_core::SpawnRequest,
    ) -> Result<ArtifactRef, ManagedRunError> {
        let attempt = AttemptId::new(request.attempt_id.as_ref().ok_or_else(unavailable)?);
        let active = self
            .active
            .lock_recover()
            .get(&attempt)
            .cloned()
            .ok_or_else(unavailable)?;
        let saved = active.suspension().ok_or_else(unavailable)?;
        if active.parent_attempt.is_some()
            || active.work_scope.is_canceled()
            || self.is_canceled(&attempt)
        {
            return Err(unavailable());
        }
        let auth = active.authorization.as_ref().ok_or_else(unavailable)?;
        auth.authority
            .authorize(&auth.scope, &active.identity, &active.binding.job_spec)
            .await
            .map_err(|_| unavailable())?;
        let checkpoint = self
            .read_wait_checkpoint(&active.binding, &saved.checkpoint)
            .await?;
        if checkpoint.invocation.discipline.handoff_tool.as_deref() != Some(&request.tool_name)
            || serde_json::to_value(&checkpoint.pending).ok() != serde_json::to_value(request).ok()
        {
            return Err(unavailable());
        }
        Ok(saved.checkpoint)
    }

    pub(crate) async fn read_wait_checkpoint(
        &self,
        binding: &ManagedBinding,
        reference: &ArtifactRef,
    ) -> Result<tetonic_core::WaitCheckpoint, ManagedRunError> {
        let id = tetonic_domain::ArtifactId::new(&reference.artifact_id);
        let meta = self
            .artifacts
            .metadata(&id)
            .await
            .map_err(|_| unavailable())?;
        if meta.producer_run_id != binding.run_id
            || meta.producer_task_id != binding.task_id
            || meta.producer_attempt_id != binding.attempt_id
            || meta.kind != tetonic_domain::artifact::ArtifactKind::ContextPack
            || meta.size_bytes > MAX_CHECKPOINT_BYTES as u64
            || format!("sha256:{}", meta.content_digest.0) != reference.digest
        {
            return Err(unavailable());
        }
        tetonic_artifact::verify_stored_content(self.artifacts.as_ref(), &id)
            .await
            .map_err(|_| unavailable())?;
        let mut reader = self.artifacts.open(&id).await.map_err(|_| unavailable())?;
        let mut bytes = Vec::new();
        let mut chunk = [0; 8192];
        loop {
            let n = reader
                .read_chunk(&mut chunk)
                .await
                .map_err(|_| unavailable())?;
            if n == 0 {
                break;
            }
            if bytes.len() + n > MAX_CHECKPOINT_BYTES {
                return Err(unavailable());
            }
            bytes.extend_from_slice(&chunk[..n]);
        }
        use sha2::Digest;
        if bytes.len() as u64 != meta.size_bytes
            || format!("{:x}", sha2::Sha256::digest(&bytes)) != meta.content_digest.0
        {
            return Err(unavailable());
        }
        let checkpoint: tetonic_core::WaitCheckpoint =
            serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
        checkpoint.validate().map_err(|_| unavailable())?;
        if checkpoint.pending.attempt_id.as_deref() != Some(binding.attempt_id.0.as_str())
            || crate::job_input_digest(&checkpoint.invocation.user_input)
                != binding.job_spec.input_digest
        {
            return Err(unavailable());
        }
        Ok(checkpoint)
    }

    pub(crate) async fn resume_boundary(&self, attempt: &AttemptId) -> Result<(), ManagedRunError> {
        loop {
            let _admission = self.admission_gate.lock().await;
            let _heartbeat = self.heartbeat_gate.lock().await;
            let active = self
                .active
                .lock_recover()
                .get(attempt)
                .cloned()
                .ok_or_else(unavailable)?;
            let saved = active.suspension().ok_or_else(unavailable)?;
            if active.work_scope.is_canceled() || self.is_canceled(attempt) {
                return Err(unavailable());
            }
            if let Some(auth) = &active.authorization {
                auth.authority
                    .authorize(&auth.scope, &active.identity, &active.binding.job_spec)
                    .await
                    .map_err(|_| unavailable())?;
            }
            self.read_wait_checkpoint(&active.binding, &saved.checkpoint)
                .await?;
            let result = self
                .supervisor
                .handle(RunCommand::ResumeAttempt(tetonic_domain::ResumeAttempt {
                    envelope: crate::command_envelope(
                        format!("resume:{attempt}:{}", saved.checkpoint.artifact_id),
                        None,
                        "tetonic-runtime",
                    ),
                    run_id: active.binding.run_id.clone(),
                    attempt_id: attempt.clone(),
                    checkpoint: saved.checkpoint,
                    lease_proof: active.lease_proof.clone(),
                    holder: tetonic_domain::ExecutionTargetId::local(),
                }))
                .await;
            let result = match result {
                Err(
                    tetonic_domain::RunSupervisorError::ExecutionCapacityExceeded
                    | tetonic_domain::RunSupervisorError::OrganizationCapacityExceeded
                    | tetonic_domain::RunSupervisorError::TeamCapacityExceeded
                    | tetonic_domain::RunSupervisorError::PrincipalCapacityExceeded,
                ) => {
                    drop(_heartbeat);
                    drop(_admission);
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    continue;
                }
                result => result.map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?,
            };
            let lease = result.snapshot.attempts[attempt]
                .lease
                .clone()
                .ok_or_else(unavailable)?;
            let proof = tetonic_domain::LeaseProof {
                lease_id: lease.lease_id,
                lease_epoch: lease.lease_epoch,
                holder: lease.holder,
            };
            // Install the new fence before claim, so cancellation/failure cleanup
            // can acknowledge quiescence even if the claim response is lost.
            if let Some(current) = self.active.lock_recover().get_mut(attempt) {
                current.lease_proof = proof.clone();
                current.heartbeat_sequence = 0;
            }
            self.supervisor
                .handle(RunCommand::ClaimExecution(StartAttempt {
                    envelope: crate::command_envelope(
                        format!("resume-execute:{attempt}:{}", proof.lease_epoch),
                        None,
                        "tetonic-runtime",
                    ),
                    run_id: active.binding.run_id.clone(),
                    attempt_id: attempt.clone(),
                    lease_proof: proof,
                }))
                .await
                .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;
            {
                let mut clock = active.clock.lock_recover();
                clock.deadline = result.snapshot.tasks[&active.binding.task_id]
                    .binding
                    .deadline;
                clock.instant = clock.deadline.map(|deadline| {
                    tokio::time::Instant::now()
                        + std::time::Duration::from_secs(deadline.saturating_sub(unix_now()))
                });
                clock.suspension = None;
            }
            if !active.work_scope.unpark() {
                return Err(unavailable());
            }
            return Ok(());
        }
    }
}
