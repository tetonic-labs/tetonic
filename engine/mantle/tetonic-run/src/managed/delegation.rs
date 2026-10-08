//! Live parent authority, inherited execution guards and durable child receipts.
use super::{ActiveAttempt, ManagedBinding, ManagedRunError, ManagedRunService};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use tetonic_domain::{AttemptId, ExecutionScope};
use tetonic_memory::RecoverMutex;

/// Only the managed runtime can mint this handle. A serialized run/attempt ID
/// cannot substitute for a live parent, and keeping a handle cannot keep the
/// runtime alive or restore permission after its attempt has finished.
#[derive(Clone)]
pub struct DelegationParent {
    active: Weak<Mutex<HashMap<AttemptId, ActiveAttempt>>>,
    binding: ManagedBinding,
    // Capture the original credential and executor incarnation. An ID lookup
    // must not lend a replacement worker's authority to an old handle.
    issued: Arc<ActiveAttempt>,
}

impl DelegationParent {
    pub(super) fn belongs_to(&self, service: &ManagedRunService) -> bool {
        self.active.ptr_eq(&Arc::downgrade(&service.active))
    }
    pub fn binding(&self) -> &ManagedBinding {
        &self.binding
    }

    pub(super) fn live_attempt(&self) -> Result<ActiveAttempt, ()> {
        let registry = self.active.upgrade().ok_or(())?;
        let active = registry
            .lock_recover()
            .get(&self.binding.attempt_id)
            .cloned()
            .ok_or(())?;
        if active.binding != self.binding
            || active.lease_proof != self.issued.lease_proof
            || !Arc::ptr_eq(&active.clock, &self.issued.clock)
            || active.suspension().is_some()
            || active.work_scope.is_canceled()
            || active.deadline_elapsed()
            || active.authorization.is_none()
            || active
                .delegation_closed
                .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(());
        }
        Ok(active)
    }

    /// Recheck the parent's original credential and job authority, not merely
    /// the child caller's credential. Child grant attenuation is checked by the
    /// store separately. No registry lock is held across an authority await.
    pub async fn authorize_child_scope(&self, scope: &ExecutionScope) -> Result<(), ()> {
        let active = self.live_attempt()?;
        let authorization = active.authorization.as_ref().ok_or(())?;
        if authorization.scope != *scope {
            return Err(());
        }
        authorization
            .authority
            .authorize(
                &authorization.scope,
                &active.identity,
                &active.binding.job_spec,
            )
            .await?;
        self.live_attempt()?;
        Ok(())
    }

    /// Credential portion of continuation authorization. The caller must first
    /// validate the stored grant chain and use its immutable lifetime. This is
    /// deliberately insufficient to dispatch a child: that requires live_attempt.
    pub async fn authorize_continuation_scope(
        &self,
        scope: &ExecutionScope,
        lifetime: tetonic_memory::DelegationLifetime,
    ) -> Result<(), ()> {
        if lifetime == tetonic_memory::DelegationLifetime::ParentLease {
            return self.authorize_child_scope(scope).await;
        }
        let check_stop = || {
            if self.issued.work_scope.is_canceled()
                || self
                    .issued
                    .delegation_closed
                    .load(std::sync::atomic::Ordering::SeqCst)
            {
                Err(())
            } else {
                Ok(())
            }
        };
        check_stop()?;
        let auth = self.issued.authorization.as_ref().ok_or(())?;
        if auth.scope != *scope {
            return Err(());
        }
        auth.authority
            .authorize(scope, &self.issued.identity, &self.binding.job_spec)
            .await?;
        check_stop()
    }
}

/// The runtime composes this guard itself. A custom host authority cannot skip
/// durable lineage checks or the parent's original credential after admission.
struct DelegatedAuthority {
    store: tetonic_memory::SharedStore,
    parent: DelegationParent,
    grant: String,
    request: String,
    inner: Arc<dyn super::ExecutionAuthority>,
}

#[async_trait::async_trait]
impl super::ExecutionAuthority for DelegatedAuthority {
    async fn authorize(
        &self,
        scope: &ExecutionScope,
        identity: &tetonic_domain::AgentIdentity,
        job: &tetonic_domain::AgentJobSpec,
    ) -> Result<(), ()> {
        self.inner.authorize(scope, identity, job).await?;
        let continuation_scope = scope.clone();
        let (grant, request, scope, job, parent) = (
            self.grant.clone(),
            self.request.clone(),
            scope.clone(),
            job.clone(),
            self.parent.binding.clone(),
        );
        let lineage = self
            .store
            .read(move |db| {
                db.require_delegated_execution_binding(tetonic_memory::DelegatedExecutionBinding {
                    id: &grant,
                    scope: &scope,
                    job: &job,
                    parent_run: &parent.run_id.0,
                    parent_attempt: &parent.attempt_id.0,
                    request: &request,
                    now: chrono::Utc::now().timestamp(),
                })
            })
            .await
            .map_err(|_| ())?
            .map_err(|_| ())?;
        self.parent
            .authorize_continuation_scope(&continuation_scope, lineage.lifetime)
            .await?;
        Ok(())
    }
    async fn revoked_during_execution(
        &self,
        scope: &ExecutionScope,
        identity: &tetonic_domain::AgentIdentity,
        job: &tetonic_domain::AgentJobSpec,
    ) -> bool {
        self.authorize(scope, identity, job).await.is_err()
    }
}

pub(super) fn child_task_id(
    scope: &ExecutionScope,
    request: &str,
) -> Result<tetonic_domain::TaskId, ManagedRunError> {
    let id = super::activation::activation_run_id(scope, request)?;
    Ok(tetonic_domain::TaskId::new(format!("task_child_{}", id.0)))
}

impl ManagedRunService {
    pub(super) fn child_authorization(
        &self,
        parent: &DelegationParent,
        authorization: &super::AuthorizedExecution,
        activation: &tetonic_domain::ActivationBinding,
    ) -> Result<super::AuthorizedExecution, ManagedRunError> {
        let deny = || {
            ManagedRunError::InvalidRequest(
                "delegation requires this runtime's live parent, stored grant and allocation"
                    .into(),
            )
        };
        if !parent.belongs_to(self) {
            return Err(deny());
        }
        parent.live_attempt().map_err(|_| deny())?;
        Ok(super::AuthorizedExecution {
            grant_id: authorization.grant_id.clone(),
            scope: authorization.scope.clone(),
            authority: Arc::new(DelegatedAuthority {
                store: self.store.clone().ok_or_else(deny)?,
                parent: parent.clone(),
                grant: authorization.grant_id.clone().ok_or_else(deny)?,
                request: activation.request_id.clone(),
                inner: authorization.authority.clone(),
            }),
        })
    }

    /// Existing child receipts never claim another worker, including after a
    /// partial admission. The parent's run owns the child task and stop scope.
    pub async fn lookup_child_activation(
        &self,
        parent: &DelegationParent,
        authorization: &super::AuthorizedExecution,
        activation: &tetonic_domain::ActivationBinding,
        identity: &tetonic_domain::AgentIdentity,
        job: &tetonic_domain::AgentJobSpec,
        role: Option<&str>,
    ) -> Result<Option<super::ActivationReceipt>, ManagedRunError> {
        let authorization = self.child_authorization(parent, authorization, activation)?;
        let deny =
            || ManagedRunError::InvalidRequest("delegated execution authorization denied".into());
        authorization
            .authority
            .authorize(&authorization.scope, identity, job)
            .await
            .map_err(|_| deny())?;
        let run = parent.binding.run_id.clone();
        let snapshot = self
            .store
            .as_ref()
            .ok_or_else(deny)?
            .read(move |db| db.load_run_snapshot(&run.0))
            .await
            .map_err(|_| deny())?
            .map_err(|_| deny())?
            .ok_or_else(deny)?;
        let task_id = child_task_id(&authorization.scope, &activation.request_id)?;
        let receipt = if let Some(task) = snapshot.tasks.get(&task_id) {
            let stored = task.binding.delegation.as_ref().ok_or_else(deny)?;
            if stored.parent_attempt != parent.binding.attempt_id
                || stored.activation.request_id != activation.request_id
                || stored.activation.request_digest != activation.request_digest
                || task.binding.job_spec.as_ref() != Some(job)
                || task.binding.execution_scope.as_ref() != Some(&authorization.scope)
                || task.binding.execution_grant_id != authorization.grant_id
                || task.binding.job_role.as_deref() != role
            {
                return Err(ManagedRunError::InvalidRequest(
                    "child activation conflicts with existing task".into(),
                ));
            }
            Some(super::ActivationReceipt {
                run_id: snapshot.run_id,
                task_id,
                audit_session_id: stored.activation.audit_session_id.clone(),
            })
        } else {
            None
        };
        authorization
            .authority
            .authorize(&authorization.scope, identity, job)
            .await
            .map_err(|_| deny())?;
        parent.live_attempt().map_err(|_| deny())?;
        Ok(receipt)
    }
}

impl ManagedRunService {
    pub fn delegation_parent(
        &self,
        attempt: &AttemptId,
    ) -> Result<DelegationParent, ManagedRunError> {
        let binding = self.binding(attempt).ok_or_else(|| {
            ManagedRunError::InvalidRequest("delegation requires a live parent".into())
        })?;
        let parent = DelegationParent {
            active: Arc::downgrade(&self.active),
            issued: Arc::new(
                self.active
                    .lock_recover()
                    .get(attempt)
                    .cloned()
                    .ok_or_else(|| {
                        ManagedRunError::InvalidRequest("delegation requires a live parent".into())
                    })?,
            ),
            binding,
        };
        parent.live_attempt().map_err(|_| {
            ManagedRunError::InvalidRequest("delegation requires an authorized live parent".into())
        })?;
        Ok(parent)
    }
}
