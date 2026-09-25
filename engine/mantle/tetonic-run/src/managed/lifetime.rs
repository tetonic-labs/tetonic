use super::contracts::*;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tetonic_domain::{AttemptId, HeartbeatReport, LeaseProof, RecordHeartbeat, RunCommand};
use tetonic_memory::RecoverMutex;
use tokio::task::AbortHandle;

#[derive(Clone)]
pub struct ActiveAttempt {
    pub work_scope: tetonic_domain::work_scope::WorkScope,
    pub binding: ManagedBinding,
    pub identity: tetonic_domain::AgentIdentity,
    pub execution_policy: ExecutionPolicy,
    pub authorization: Option<AuthorizedExecution>,
    pub role: Option<String>,
    pub parent_attempt: Option<AttemptId>,
    pub task_handle: Option<AbortHandle>,
    pub heartbeat_cancel: Arc<AtomicBool>,
    pub heartbeat_sequence: u64,
    pub lease_proof: LeaseProof,
    pub sequence: u64,
}

#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub enum DispatchEntry {
    Pending {
        canceled: bool,
        task: Option<AbortHandle>,
    },
    Admitted {
        attempt_id: AttemptId,
        binding: ManagedBinding,
        task: Option<AbortHandle>,
        canceled: bool,
    },
    Terminal,
}

impl super::service::ManagedRunService {
    pub fn reserve_dispatch(&self) -> DispatchTicket {
        let id = DispatchId::new();
        self.dispatches.lock_recover().insert(
            id.clone(),
            DispatchEntry::Pending {
                canceled: false,
                task: None,
            },
        );
        DispatchTicket { id }
    }

    pub fn attach_task(&self, id: &DispatchId, task: AbortHandle) -> Result<(), ManagedRunError> {
        let mut guard = self.dispatches.lock_recover();
        let entry = guard
            .get_mut(id)
            .ok_or_else(|| ManagedRunError::InvalidRequest("dispatch ticket not found".into()))?;

        match entry {
            DispatchEntry::Pending {
                canceled,
                task: existing,
            } => {
                if *canceled {
                    task.abort();
                    return Err(ManagedRunError::InvalidRequest(
                        "dispatch was canceled".into(),
                    ));
                }
                *existing = Some(task.clone());
            }
            DispatchEntry::Admitted {
                canceled,
                task: existing,
                attempt_id,
                ..
            } => {
                if *canceled {
                    task.abort();
                    return Err(ManagedRunError::InvalidRequest(
                        "dispatch was canceled".into(),
                    ));
                }
                *existing = Some(task.clone());
                if let Some(active) = self.active.lock_recover().get_mut(attempt_id) {
                    active.task_handle = Some(task);
                }
            }
            DispatchEntry::Terminal => {
                task.abort();
                return Err(ManagedRunError::InvalidRequest(
                    "dispatch is terminal".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn cancel_dispatch(&self, id: &DispatchId) -> Result<(), ManagedRunError> {
        let mut dispatches = self.dispatches.lock_recover();
        let entry = dispatches
            .get_mut(id)
            .ok_or_else(|| ManagedRunError::InvalidRequest("dispatch not found".into()))?;
        if let DispatchEntry::Admitted { attempt_id, .. } = &*entry {
            if let Some(active) = self.active.lock_recover().get(attempt_id) {
                active.work_scope.cancel();
            }
        }
        match entry {
            DispatchEntry::Pending { canceled, .. } | DispatchEntry::Admitted { canceled, .. } => {
                // Admission and execution own cleanup. Do not abort the future which
                // must finish registration or persist a scoped terminal result.
                *canceled = true;
            }
            DispatchEntry::Terminal => {}
        }
        Ok(())
    }

    pub fn is_canceled(&self, attempt: &AttemptId) -> bool {
        let id = self.attempt_dispatches.lock_recover().get(attempt).cloned();
        let Some(id) = id else {
            return false;
        };
        matches!(
            self.dispatches.lock_recover().get(&id),
            Some(
                DispatchEntry::Pending { canceled: true, .. }
                    | DispatchEntry::Admitted { canceled: true, .. }
            )
        )
    }

    pub fn binding(&self, attempt: &AttemptId) -> Option<ManagedBinding> {
        self.active
            .lock_recover()
            .get(attempt)
            .map(|a| a.binding.clone())
    }

    pub async fn release_dispatch(&self, id: &DispatchId) -> Result<(), ManagedRunError> {
        let mut guard = self.dispatches.lock_recover();
        let entry = guard.get(id);
        let Some(entry) = entry else {
            return Ok(());
        };
        if let DispatchEntry::Admitted { attempt_id, .. } = entry {
            if self.active.lock_recover().contains_key(attempt_id) {
                return Err(ManagedRunError::InvalidRequest(
                    "cannot release active unfinished attempt".into(),
                ));
            }
        }
        guard.remove(id);
        Ok(())
    }

    pub async fn heartbeat(&self, attempt: &AttemptId) -> Result<(), ManagedRunError> {
        let _gate = self.heartbeat_gate.lock().await;
        let (run_id, lease_proof, next_seq) = {
            let mut guard = self.active.lock_recover();
            let active = guard
                .get_mut(attempt)
                .ok_or_else(|| ManagedRunError::InvalidRequest("attempt not active".into()))?;
            let next_seq = active.heartbeat_sequence.saturating_add(1);
            active.heartbeat_sequence = next_seq;
            (
                active.binding.run_id.clone(),
                active.lease_proof.clone(),
                next_seq,
            )
        };

        let now = unix_now();
        let renewed = now.saturating_add(300);

        let cmd = RecordHeartbeat {
            envelope: crate::command_envelope(
                format!("heartbeat:{}_{}", attempt, next_seq),
                None,
                "lokai-manager",
            ),
            run_id,
            attempt_id: attempt.clone(),
            lease_proof,
            heartbeat_sequence: next_seq,
            report: HeartbeatReport {
                attempt_state: "running".into(),
                progress_marker: None,
                resource_usage: None,
                output_size_bytes: None,
                lease_renewal_requested: true,
                executor_health: None,
            },
            renewed_expires_at: Some(renewed),
        };

        let res = self
            .supervisor
            .handle(RunCommand::RecordHeartbeat(cmd))
            .await
            .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;

        if let Some(active) = self.active.lock_recover().get_mut(attempt) {
            active.sequence = res.sequence;
        }
        Ok(())
    }

    pub fn arm_attempt_join(
        &self,
        attempt_id: &AttemptId,
    ) -> tokio::sync::oneshot::Receiver<StartIdentityJobResult> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.attempt_joins
            .lock_recover()
            .insert(attempt_id.clone(), tx);
        rx
    }

    pub(crate) fn complete_attempt_join(&self, res: StartIdentityJobResult) {
        if let Some(tx) = self.attempt_joins.lock_recover().remove(&res.attempt_id) {
            let _ = tx.send(res);
        }
    }
}

pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl super::service::ManagedRunService {
    pub fn active_bindings(&self, run: &tetonic_domain::RunId) -> Vec<ManagedBinding> {
        self.active
            .lock_recover()
            .values()
            .filter(|a| &a.binding.run_id == run)
            .map(|a| a.binding.clone())
            .collect()
    }
    pub fn dispatch_for_attempt(&self, attempt: &AttemptId) -> Option<DispatchId> {
        self.attempt_dispatches.lock_recover().get(attempt).cloned()
    }
    pub fn spawn_dispatch(
        &self,
        id: &DispatchId,
        future: impl std::future::Future<Output = ()> + 'static,
    ) -> Result<(), ManagedRunError> {
        let task = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tokio::task::spawn_local(future)
        }))
        .map_err(|_| {
            ManagedRunError::InvalidRequest("dispatch requires a Tokio LocalSet".into())
        })?;
        self.attach_task(id, task.abort_handle())
    }
}
