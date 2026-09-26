//! Durable retry identity is carried by the existing root task and run journal.
use super::contracts::*;
use sha2::{Digest, Sha256};
use tetonic_domain::{ActivationBinding, ExecutionScope, RunId, RunSnapshot, TaskId};

pub fn activation_run_id(
    scope: &ExecutionScope,
    request_id: &str,
) -> Result<RunId, ManagedRunError> {
    if request_id.is_empty()
        || request_id.len() > 128
        || !request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return Err(ManagedRunError::InvalidRequest(
            "invalid activation request ID".into(),
        ));
    }
    // Length-delimited serialization avoids concatenation ambiguity. Context is
    // in the fingerprint, not the key: reusing a key in another context conflicts.
    let bytes = serde_json::to_vec(&(
        "tetonic.activation.v1",
        &scope.organization_id,
        &scope.principal_id,
        request_id,
    ))
    .map_err(|e| ManagedRunError::InternalViolation(e.to_string()))?;
    Ok(RunId::new(format!(
        "run_activation_{:x}",
        Sha256::digest(bytes)
    )))
}

pub(super) fn validate_activation(
    context: &AdmissionContext,
    job: &AdmitJob,
) -> Result<(), ManagedRunError> {
    if let Some(activation) = &context.activation {
        let authorization = context.authorization.as_ref().ok_or_else(|| {
            ManagedRunError::InvalidRequest("activation requires verified scope".into())
        })?;
        if job.parent_attempt.is_some()
            || context.session_id.is_some()
            || context.task_id.is_some()
            || context.speculation.is_some()
        {
            return Err(ManagedRunError::InvalidRequest(
                "activation must be a scoped root job".into(),
            ));
        }
        activation_run_id(&authorization.scope, &activation.request_id)?;
        if !activation
            .request_digest
            .strip_prefix("sha256:")
            .is_some_and(|digest| {
                digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit())
            })
            || activation.audit_session_id.trim().is_empty()
            || activation.audit_session_id.len() > 256
            || activation.audit_session_id.contains('\0')
        {
            return Err(ManagedRunError::InvalidRequest(
                "invalid activation binding".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn receipt_from_snapshot(
    snapshot: &RunSnapshot,
    authorization: &AuthorizedExecution,
    activation: &ActivationBinding,
    job: &tetonic_domain::AgentJobSpec,
    role: Option<&str>,
) -> Result<ActivationReceipt, ManagedRunError> {
    let run_id = activation_run_id(&authorization.scope, &activation.request_id)?;
    let task_id = TaskId::new(format!("task_root_{run_id}"));
    let task = snapshot.tasks.get(&task_id).ok_or_else(|| {
        ManagedRunError::InvalidRequest("activation request conflicts with existing run".into())
    })?;
    let Some(stored) = &task.binding.activation else {
        return Err(ManagedRunError::InvalidRequest(
            "activation request conflicts with existing run".into(),
        ));
    };
    if snapshot.run_id != run_id
        || task.binding.execution_scope.as_ref() != Some(&authorization.scope)
        || task.binding.execution_grant_id != authorization.grant_id
        || task.binding.job_spec.as_ref() != Some(job)
        || task.binding.job_role.as_deref() != role
        || stored.request_id != activation.request_id
        || stored.request_digest != activation.request_digest
    {
        return Err(ManagedRunError::InvalidRequest(
            "activation request conflicts with existing run".into(),
        ));
    }
    Ok(ActivationReceipt {
        run_id,
        task_id,
        audit_session_id: stored.audit_session_id.clone(),
    })
}

impl super::service::ManagedRunService {
    /// Rechecks current authority even for retries of terminal/interrupted jobs.
    /// An existing receipt never resumes or claims execution.
    pub async fn lookup_activation(
        &self,
        authorization: &AuthorizedExecution,
        activation: &ActivationBinding,
        identity: &tetonic_domain::AgentIdentity,
        job: &tetonic_domain::AgentJobSpec,
        role: Option<&str>,
    ) -> Result<Option<ActivationReceipt>, ManagedRunError> {
        authorization
            .authority
            .authorize(&authorization.scope, identity, job)
            .await
            .map_err(|_| {
                ManagedRunError::InvalidRequest("execution authorization denied".into())
            })?;
        let store = self.store.as_ref().ok_or_else(|| {
            ManagedRunError::InvalidRequest("activation requires durable storage".into())
        })?;
        let run_id = activation_run_id(&authorization.scope, &activation.request_id)?;
        let snapshot = store
            .read(move |db| db.load_run_snapshot(&run_id.0))
            .await
            .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?
            .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;
        let receipt = snapshot
            .as_ref()
            .map(|snapshot| receipt_from_snapshot(snapshot, authorization, activation, job, role))
            .transpose()?;
        authorization
            .authority
            .authorize(&authorization.scope, identity, job)
            .await
            .map_err(|_| {
                ManagedRunError::InvalidRequest("execution authorization denied".into())
            })?;
        Ok(receipt)
    }
}
