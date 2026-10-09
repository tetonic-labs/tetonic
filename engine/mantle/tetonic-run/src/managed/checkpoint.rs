//! Protected checkpoint artifacts; saving a boundary does not suspend or resume it.
use super::{DelegationParent, ManagedBinding, ManagedRunError, ManagedRunService};
use tetonic_core::WaitCheckpoint;
use tetonic_domain::{
    artifact::{ArtifactDeclaration, ArtifactKind, RetentionPolicy},
    ArtifactRef,
};

pub(super) const MAX_CHECKPOINT_BYTES: usize = 2 * 1024 * 1024;

fn unavailable() -> ManagedRunError {
    ManagedRunError::InvalidRequest("coordinator checkpoint could not be verified".into())
}

impl ManagedRunService {
    /// Save the general harness's exact pre-dispatch boundary under its original
    /// live parent. The caller must bind the reference in a fenced transaction
    /// before admitting children. This never claims that the subtree is quiescent.
    pub async fn save_dispatch_checkpoint(
        &self,
        parent: &DelegationParent,
        checkpoint: &WaitCheckpoint,
    ) -> Result<ArtifactRef, ManagedRunError> {
        self.authorize_dispatch_checkpoint(parent).await?;
        validate_dispatch(parent.binding(), checkpoint)?;
        let reference = self
            .write_wait_checkpoint(parent.binding(), checkpoint)
            .await?;
        self.authorize_dispatch_checkpoint(parent).await?;
        Ok(reference)
    }

    /// Read only for the current live owner. A historical artifact is not an
    /// execution permit and cannot mint a replacement parent handle.
    pub async fn read_dispatch_checkpoint(
        &self,
        parent: &DelegationParent,
        reference: &ArtifactRef,
    ) -> Result<WaitCheckpoint, ManagedRunError> {
        self.authorize_dispatch_checkpoint(parent).await?;
        let checkpoint = self
            .read_wait_checkpoint(parent.binding(), reference)
            .await?;
        validate_dispatch(parent.binding(), &checkpoint)?;
        self.authorize_dispatch_checkpoint(parent).await?;
        Ok(checkpoint)
    }

    async fn authorize_dispatch_checkpoint(
        &self,
        parent: &DelegationParent,
    ) -> Result<(), ManagedRunError> {
        if !parent.belongs_to(self) {
            return Err(unavailable());
        }
        parent
            .authorize_dispatch()
            .await
            .map_err(|_| unavailable())?;
        self.authorize_executor(&parent.live_attempt().map_err(|_| unavailable())?)
            .await
            .map_err(|_| unavailable())
    }

    pub(super) async fn write_wait_checkpoint(
        &self,
        binding: &ManagedBinding,
        checkpoint: &WaitCheckpoint,
    ) -> Result<ArtifactRef, ManagedRunError> {
        checkpoint.validate().map_err(|_| unavailable())?;
        let bytes = serde_json::to_vec(checkpoint).map_err(|_| unavailable())?;
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(unavailable());
        }
        let mut writer = self
            .artifacts
            .begin_write(ArtifactDeclaration {
                kind: ArtifactKind::ExecutionCheckpoint,
                producer_run_id: binding.run_id.clone(),
                producer_task_id: binding.task_id.clone(),
                producer_attempt_id: binding.attempt_id.clone(),
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
        Ok(ArtifactRef {
            artifact_id: meta.artifact_id.0,
            digest: format!("sha256:{}", meta.content_digest.0),
        })
    }
}

fn validate_dispatch(
    binding: &ManagedBinding,
    checkpoint: &WaitCheckpoint,
) -> Result<(), ManagedRunError> {
    checkpoint
        .received_host_calls()
        .map_err(|_| unavailable())?;
    if checkpoint.pending.attempt_id.as_deref() != Some(binding.attempt_id.0.as_str())
        || checkpoint.pending.parent_agent_id != binding.job_spec.identity_id.0
        || crate::job_input_digest(&checkpoint.invocation.user_input)
            != binding.job_spec.input_digest
        || checkpoint.invocation.discipline.spawn_tool.as_deref()
            != Some(&checkpoint.pending.tool_name)
    {
        return Err(unavailable());
    }
    Ok(())
}
