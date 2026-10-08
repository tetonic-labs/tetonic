//! Deterministic interleaving with other activity in the same managed run.
use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
use tetonic_domain::{
    RunCommand, RunCommandResult, RunEventEnvelope, RunId, RunSnapshot, RunSupervisorError,
};
use tetonic_run::{ManagedBinding, RunSupervisor};

struct InterleavingSupervisor {
    inner: DurableRunSupervisor,
    parent: Mutex<Option<ManagedBinding>>,
    injected: AtomicUsize,
    cancel_at: Option<usize>,
}

#[async_trait::async_trait]
impl RunSupervisor for InterleavingSupervisor {
    async fn handle(&self, command: RunCommand) -> Result<RunCommandResult, RunSupervisorError> {
        let parent = self.parent.lock().unwrap().clone();
        if let Some(parent) = parent.filter(|_| {
            matches!(
                &command,
                RunCommand::AddTask(_)
                    | RunCommand::CreateAttempt(_)
                    | RunCommand::LeaseAttempt(_)
                    | RunCommand::StartAttempt(_)
            )
        }) {
            let n = self.injected.fetch_add(1, Ordering::SeqCst);
            let envelope = tetonic_run::command_envelope(format!("interleave-{n}"), None, "test");
            let update = if self.cancel_at == Some(n) {
                RunCommand::CancelRun(tetonic_domain::CancelRun {
                    envelope,
                    run_id: parent.run_id,
                })
            } else {
                let snapshot = self.inner.snapshot(parent.run_id.clone()).await?;
                let lease = snapshot.attempts[&parent.attempt_id]
                    .lease
                    .as_ref()
                    .unwrap();
                RunCommand::RecordHeartbeat(tetonic_domain::RecordHeartbeat {
                    envelope,
                    run_id: parent.run_id,
                    attempt_id: parent.attempt_id,
                    lease_proof: tetonic_domain::LeaseProof {
                        lease_id: lease.lease_id.clone(),
                        lease_epoch: lease.lease_epoch,
                        holder: lease.holder.clone(),
                    },
                    heartbeat_sequence: n as u64 + 1,
                    report: tetonic_domain::HeartbeatReport {
                        attempt_state: "running".into(),
                        progress_marker: None,
                        resource_usage: None,
                        output_size_bytes: None,
                        lease_renewal_requested: false,
                        executor_health: None,
                    },
                    renewed_expires_at: None,
                })
            };
            self.inner.handle(update).await?;
        }
        self.inner.handle(command).await
    }

    async fn snapshot(&self, id: RunId) -> Result<RunSnapshot, RunSupervisorError> {
        self.inner.snapshot(id).await
    }

    async fn resume_from_sequence(
        &self,
        id: RunId,
        after: u64,
        limit: u32,
    ) -> Result<Result<Vec<RunEventEnvelope>, tetonic_domain::ReplayGap>, RunSupervisorError> {
        self.inner.resume_from_sequence(id, after, limit).await
    }
}

async fn interleaved_child(cancel_at: Option<usize>) {
    let dir = tempfile::tempdir().unwrap();
    let store = tetonic_memory::SharedStore::open(dir.path().join("runs.db"), 1).unwrap();
    let supervisor = Arc::new(InterleavingSupervisor {
        inner: DurableRunSupervisor::new(Some(store.clone())),
        parent: Mutex::new(None),
        injected: AtomicUsize::new(0),
        cancel_at,
    });
    let artifacts = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            dir.path().join("artifacts"),
            tetonic_artifact::ScanPolicy::Scan(Arc::new(|_| false)),
        )
        .unwrap(),
    );
    let service = ManagedRunService::new(
        supervisor.clone(),
        Some(store),
        artifacts,
        Arc::new(tetonic_policy::PolicyEngine::new(
            tetonic_policy::PolicyMode::EstateStub,
        )),
    );
    let parent = admit_root(&service).await;
    *supervisor.parent.lock().unwrap() = Some(parent.clone());
    let (identity, job_spec) = test_identity_and_spec();
    let result = service
        .admit(
            &service.reserve_dispatch().id,
            AdmitJob {
                identity,
                job_spec,
                role: None,
                parent_attempt: Some(parent.attempt_id.clone()),
            },
        )
        .await;
    let snapshot = service.inspect_run(&parent.run_id).await.unwrap();
    if let Some(stage) = cancel_at {
        assert!(
            result.is_err(),
            "cancellation must still fence child admission at stage {stage}"
        );
        assert_eq!(supervisor.injected.load(Ordering::SeqCst), stage + 1);
        assert_eq!(snapshot.state, tetonic_domain::RunState::Canceled);
    } else {
        let child = result.expect("unrelated run progress must not reject a new child");
        assert_eq!(supervisor.injected.load(Ordering::SeqCst), 4);
        assert_eq!(child.run_id, parent.run_id);
        assert_eq!(snapshot.tasks.len(), 2);
        assert_eq!(snapshot.attempts.len(), 2);
        assert_eq!(service.binding(&child.attempt_id), Some(child.clone()));
        assert!(
            !snapshot.attempts[&child.attempt_id].execution_claimed,
            "admission must not grant execution outside the execution claim boundary"
        );
    }
}

#[tokio::test]
async fn child_admission_survives_progress_between_every_transition() {
    interleaved_child(None).await;
}

#[tokio::test]
async fn child_admission_still_rejects_cancellation_between_transitions() {
    for stage in 0..4 {
        interleaved_child(Some(stage)).await;
    }
}
