//! Main ManagedRunService struct and constructor.

use super::contracts::*;
use super::lifetime::{ActiveAttempt, DispatchEntry};
use crate::RunSupervisor;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tetonic_domain::{AttemptId, ReplayGap, RunEventEnvelope, RunId, RunSnapshot};
use tetonic_memory::RecoverMutex;
use tokio::sync::{oneshot, Barrier};
use tokio::task::AbortHandle;

#[derive(Clone)]
pub struct ManagedRunService {
    pub(crate) heartbeat_gate: Arc<tokio::sync::Mutex<()>>,
    pub(crate) admission_gate: Arc<tokio::sync::Mutex<()>>,
    pub(crate) supervisor: Arc<dyn RunSupervisor>,
    pub(crate) store: Option<tetonic_memory::SharedStore>,
    pub(crate) artifacts: Arc<dyn tetonic_domain::artifact::ArtifactStore>,
    pub(crate) policy: Arc<tetonic_policy::PolicyEngine>,
    pub(crate) execution_policy: ExecutionPolicy,
    pub(crate) hooks: Arc<Mutex<Option<Arc<dyn ManagedRunHooks>>>>,
    pub(crate) dispatches: Arc<Mutex<HashMap<DispatchId, DispatchEntry>>>,
    pub(crate) active: Arc<Mutex<HashMap<AttemptId, ActiveAttempt>>>,
    pub(crate) attempt_dispatches: Arc<Mutex<HashMap<AttemptId, DispatchId>>>,
    pub(crate) attempt_joins:
        Arc<Mutex<HashMap<AttemptId, oneshot::Sender<StartIdentityJobResult>>>>,
    pub(crate) pre_admission_barrier: Arc<Mutex<Option<Arc<Barrier>>>>,
    pub(crate) post_admission_barrier: Arc<Mutex<Option<Arc<Barrier>>>>,
    pub(crate) post_admission_pause: Arc<Mutex<Option<Arc<tokio::sync::Notify>>>>,
    pub(crate) post_admission_resume: Arc<Mutex<Option<Arc<tokio::sync::Notify>>>>,
}

impl ManagedRunService {
    pub fn new(
        supervisor: Arc<dyn RunSupervisor>,
        store: Option<tetonic_memory::SharedStore>,
        artifacts: Arc<dyn tetonic_domain::artifact::ArtifactStore>,
        policy: Arc<tetonic_policy::PolicyEngine>,
    ) -> Self {
        Self {
            heartbeat_gate: Arc::new(tokio::sync::Mutex::new(())),
            admission_gate: Arc::new(tokio::sync::Mutex::new(())),
            supervisor,
            store,
            artifacts,
            policy,
            execution_policy: Arc::new(|_, _, _, _, _| Ok(())),
            hooks: Arc::new(Mutex::new(None)),
            dispatches: Arc::new(Mutex::new(HashMap::new())),
            active: Arc::new(Mutex::new(HashMap::new())),
            attempt_dispatches: Arc::new(Mutex::new(HashMap::new())),
            attempt_joins: Arc::new(Mutex::new(HashMap::new())),
            pre_admission_barrier: Arc::new(Mutex::new(None)),
            post_admission_barrier: Arc::new(Mutex::new(None)),
            post_admission_pause: Arc::new(Mutex::new(None)),
            post_admission_resume: Arc::new(Mutex::new(None)),
        }
    }

    pub fn store(&self) -> Option<&tetonic_memory::SharedStore> {
        self.store.as_ref()
    }

    pub fn policy(&self) -> &Arc<tetonic_policy::PolicyEngine> {
        &self.policy
    }

    pub fn artifacts(&self) -> &Arc<dyn tetonic_domain::artifact::ArtifactStore> {
        &self.artifacts
    }

    pub fn with_execution_policy(mut self, policy: ExecutionPolicy) -> Self {
        self.execution_policy = policy;
        self
    }

    pub fn with_hooks(self, hooks: Arc<dyn ManagedRunHooks>) -> Self {
        *self.hooks.lock_recover() = Some(hooks);
        self
    }

    pub fn attach_hooks(&self, hooks: Arc<dyn ManagedRunHooks>) {
        *self.hooks.lock_recover() = Some(hooks);
    }

    pub(crate) fn notify_hooks(&self, notify: impl FnOnce(&dyn ManagedRunHooks)) {
        let hooks = self.hooks.lock_recover().clone();
        if let Some(hooks) = hooks {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| notify(hooks.as_ref())))
                .is_err()
            {
                tracing::error!("managed observer panicked");
            }
        }
    }

    pub fn set_pre_admission_barrier(&self, barrier: Arc<Barrier>) {
        *self.pre_admission_barrier.lock_recover() = Some(barrier);
    }

    pub fn set_post_admission_barrier(&self, barrier: Arc<Barrier>) {
        *self.post_admission_barrier.lock_recover() = Some(barrier);
    }

    pub fn set_post_admission_hook(
        &self,
        pause: Arc<tokio::sync::Notify>,
        resume: Arc<tokio::sync::Notify>,
    ) {
        *self.post_admission_pause.lock_recover() = Some(pause);
        *self.post_admission_resume.lock_recover() = Some(resume);
    }

    /// Abandon an inspected interrupted run without accepting or undoing its effects.
    pub async fn abandon_recovery_run(
        &self,
        run_id: &RunId,
        expected_sequence: u64,
    ) -> Result<(), ManagedRunError> {
        let snapshot = self.inspect_run(run_id).await?;
        if snapshot.state != tetonic_domain::RunState::RecoveryRequired {
            return Err(ManagedRunError::InvalidRequest(
                "run does not require recovery".into(),
            ));
        }
        self.supervisor
            .handle(tetonic_domain::RunCommand::CancelRun(
                tetonic_domain::CancelRun {
                    envelope: crate::command_envelope(
                        format!("abandon-recovery:{run_id}:{expected_sequence}"),
                        Some(expected_sequence),
                        "recovery-operator",
                    ),
                    run_id: run_id.clone(),
                },
            ))
            .await
            .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?;
        Ok(())
    }

    pub async fn inspect_run(&self, run_id: &RunId) -> Result<RunSnapshot, ManagedRunError> {
        self.supervisor
            .snapshot(run_id.clone())
            .await
            .map_err(|e| ManagedRunError::InvalidRequest(e.to_string()))
    }

    pub async fn cancel_run(&self, run_id: &RunId) -> Result<(), ManagedRunError> {
        let cmd = tetonic_domain::CancelRun {
            envelope: crate::command_envelope(
                format!("cancel_run:{}", run_id),
                None,
                "lokai-manager",
            ),
            run_id: run_id.clone(),
        };
        // Stop local work even if persistence is unavailable; never report a
        // successful cancellation until the durable command succeeds.
        let actives: Vec<_> = self
            .active
            .lock_recover()
            .values()
            .filter(|a| &a.binding.run_id == run_id)
            .cloned()
            .collect();
        for active in &actives {
            active.work_scope.cancel();
            if let Some(id) = self.dispatch_for_attempt(&active.binding.attempt_id) {
                self.cancel_dispatch(&id)?;
            }
            self.notify_hooks(|h| h.fail_approval_waits(&active.binding.attempt_id));
        }
        let result = self
            .supervisor
            .handle(tetonic_domain::RunCommand::CancelRun(cmd))
            .await
            .map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()));
        for active in actives {
            // Cancellation is an admission barrier, not evidence of physical
            // completion. Never release ownership while a worker retains a lease.
            if !active.work_scope.is_quiescent() {
                tracing::info!(attempt_id = %active.binding.attempt_id,
                    "cancellation waiting for in-process effect workers");
            }
            while !active.work_scope.is_quiescent() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            // Execution observes the cancellation flag. Let its owner drain,
            // especially while awaiting a blocking finalization effect: aborting
            // the async task would leave that effect running without its owner.
            active
                .heartbeat_cancel
                .store(true, std::sync::atomic::Ordering::SeqCst);
            let outcome = match &result {
                Ok(_) => tetonic_domain::CandidateOutcome::Canceled {
                    reason: "run canceled".into(),
                },
                Err(error) => tetonic_domain::CandidateOutcome::Failed {
                    message: format!("cannot persist cancellation: {error}"),
                },
            };
            let terminal = StartIdentityJobResult {
                run_id: active.binding.run_id,
                task_id: active.binding.task_id,
                attempt_id: active.binding.attempt_id.clone(),
                outcome,
            };
            if result.is_ok()
                && self
                    .active
                    .lock_recover()
                    .remove(&active.binding.attempt_id)
                    .is_none()
            {
                continue; // Finalization already delivered a terminal result.
            }
            if result.is_ok() {
                let dispatch = self
                    .attempt_dispatches
                    .lock_recover()
                    .remove(&active.binding.attempt_id);
                if let Some(dispatch) = dispatch {
                    self.dispatches.lock_recover().remove(&dispatch);
                }
            }
            self.notify_hooks(|h| h.terminal(&terminal));
            self.complete_attempt_join(terminal);
        }
        result?;
        Ok(())
    }

    pub async fn resume_events(
        &self,
        run_id: &RunId,
        after: u64,
        limit: usize,
    ) -> Result<Result<Vec<RunEventEnvelope>, ReplayGap>, ManagedRunError> {
        let lim: u32 = limit.try_into().unwrap_or(u32::MAX);
        self.supervisor
            .resume_from_sequence(run_id.clone(), after, lim)
            .await
            .map_err(|e| ManagedRunError::InvalidRequest(e.to_string()))
    }

    pub(crate) fn spawn_heartbeat_driver(
        &self,
        attempt_id: AttemptId,
        cancel: Arc<AtomicBool>,
    ) -> Option<AbortHandle> {
        let interval_ms = std::env::var("LOKAI_HEARTBEAT_INTERVAL_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(30_000);
        let interval = std::time::Duration::from_millis(interval_ms);
        let this = self.clone();
        let fut = async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await; // first tick is immediate
            while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                ticker.tick().await;
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = this.heartbeat(&attempt_id).await {
                    tracing::warn!("managed heartbeat failed for attempt {}: {}", attempt_id, e);
                    if let Some(id) = this.dispatch_for_attempt(&attempt_id) {
                        let _ = this.cancel_dispatch(&id);
                    }
                    break;
                }
            }
        };

        Some(tokio::spawn(fut).abort_handle())
    }
}
