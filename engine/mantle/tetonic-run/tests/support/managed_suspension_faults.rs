use super::*;
use tetonic_run::RunSupervisor;

struct Fixture {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    service: ManagedRunService,
    binding: tetonic_run::ManagedBinding,
    receipt: ActivationReceipt,
    calls: Arc<AtomicUsize>,
    effects: Arc<AtomicUsize>,
    allowed: Arc<AtomicBool>,
    answer: Arc<AtomicBool>,
}
impl Fixture {
    async fn parked() -> Self {
        let (base, dir) = test_service();
        let path = dir.path().join("faults.db");
        let service = waiting_service(&path, base.artifacts().clone());
        let calls = Arc::new(AtomicUsize::new(0));
        let effects = Arc::new(AtomicUsize::new(0));
        let allowed = Arc::new(AtomicBool::new(true));
        let answer = Arc::new(AtomicBool::new(false));
        let local = tokio::task::LocalSet::new();
        let binding = local
            .run_until(async {
                let ManagedSubmission::Started { binding, .. } = service
                    .submit_identity_job_with_context(
                        command(),
                        agent(calls.clone(), effects.clone(), answer.clone(), 100),
                        admission(allowed.clone(), 30),
                        None,
                    )
                    .await
                    .unwrap()
                else {
                    panic!()
                };
                parked(&service, &binding).await;
                *binding
            })
            .await;
        drop(local);
        let receipt = ActivationReceipt {
            run_id: binding.run_id.clone(),
            task_id: binding.task_id.clone(),
            audit_session_id: "original-audit".into(),
        };
        Self {
            _dir: dir,
            path,
            service,
            binding,
            receipt,
            calls,
            effects,
            allowed,
            answer,
        }
    }
    fn reopen(&self) -> ManagedRunService {
        waiting_service(&self.path, self.service.artifacts().clone())
    }
    async fn restore(
        &self,
        service: &ManagedRunService,
    ) -> Result<ManagedSubmission, tetonic_run::ManagedRunError> {
        service
            .restore_suspended_root(
                self.receipt.clone(),
                command(),
                agent(
                    self.calls.clone(),
                    self.effects.clone(),
                    self.answer.clone(),
                    100,
                ),
                admission(self.allowed.clone(), 999),
                None,
            )
            .await
    }
}

#[tokio::test(flavor = "current_thread")]
async fn racing_restorers_execute_once_and_loser_cannot_cancel_winner() {
    let f = Fixture::parked().await;
    let a = f.reopen();
    let b = f.reopen();
    tokio::task::LocalSet::new()
        .run_until(async {
            let ManagedSubmission::Started { completion: ca, .. } = f.restore(&a).await.unwrap()
            else {
                panic!()
            };
            let ManagedSubmission::Started { completion: cb, .. } = f.restore(&b).await.unwrap()
            else {
                panic!()
            };
            // Let both attach to the same checkpoint before exposing the answer.
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            f.answer.store(true, Ordering::SeqCst);
            let (ra, rb) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
                tokio::join!(ca, cb)
            })
            .await
            .unwrap();
            let outcomes = [ra.unwrap().outcome, rb.unwrap().outcome];
            assert_eq!(
                outcomes.iter().filter(|o| o.is_completed()).count(),
                1,
                "{outcomes:?}"
            );
            assert_eq!(f.calls.load(Ordering::SeqCst), 3);
            assert_eq!(f.effects.load(Ordering::SeqCst), 1);
            let snapshot = a.inspect_run(&f.binding.run_id).await.unwrap();
            assert_eq!(snapshot.state, RunState::Succeeded);
            assert!(!snapshot.cancellation.run_canceled);
            // Both new state commands survive authoritative event replay.
            let events = a
                .resume_events(&f.binding.run_id, 0, 100)
                .await
                .unwrap()
                .unwrap();
            let replayed = tetonic_run::replay_from_events(
                &tetonic_run::empty_snapshot(f.binding.run_id.clone(), None),
                &events,
            )
            .unwrap();
            assert_eq!(replayed.state, snapshot.state);
            assert_eq!(replayed.attempts, snapshot.attempts);
            let resume = events
                .iter()
                .find(|e| matches!(&e.event_type, tetonic_domain::EventType::Other(value) if value == "attempt.resumed"))
                .unwrap();
            let command: tetonic_domain::RunCommand =
                serde_json::from_value(resume.payload.clone()).unwrap();
            let supervisor = DurableRunSupervisor::new(a.store().cloned());
            assert!(matches!(
                supervisor.handle(command).await,
                Err(tetonic_domain::RunSupervisorError::DuplicateDelivery(_))
            ));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn canceled_wait_rejects_late_answer_and_restore() {
    let f = Fixture::parked().await;
    let service = f.reopen();
    tokio::task::LocalSet::new()
        .run_until(async {
            let ManagedSubmission::Started { completion, .. } = f.restore(&service).await.unwrap()
            else {
                panic!()
            };
            tokio::time::sleep(std::time::Duration::from_millis(75)).await;
            service.cancel_run(&f.binding.run_id).await.unwrap();
            f.answer.store(true, Ordering::SeqCst);
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), completion)
                .await
                .unwrap()
                .unwrap();
            assert!(matches!(result.outcome, CandidateOutcome::Canceled { .. }));
            assert!(f.restore(&service).await.is_err());
            assert_eq!(f.calls.load(Ordering::SeqCst), 2);
            assert_eq!(
                service.inspect_run(&f.binding.run_id).await.unwrap().state,
                RunState::Canceled
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn revoked_wait_cannot_wake_and_remains_inspectable_without_consuming_capacity() {
    let f = Fixture::parked().await;
    let service = f.reopen();
    tokio::task::LocalSet::new()
        .run_until(async {
            let ManagedSubmission::Started { completion, .. } = f.restore(&service).await.unwrap()
            else {
                panic!()
            };
            tokio::time::sleep(std::time::Duration::from_millis(75)).await;
            f.allowed.store(false, Ordering::SeqCst);
            f.answer.store(true, Ordering::SeqCst);
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), completion)
                .await
                .unwrap()
                .unwrap();
            assert!(!result.outcome.is_completed());
            assert_eq!(f.calls.load(Ordering::SeqCst), 2);
            assert_eq!(f.effects.load(Ordering::SeqCst), 1);
            assert_eq!(
                service
                    .inspect_run(&f.binding.run_id)
                    .await
                    .unwrap()
                    .attempts[&f.binding.attempt_id]
                    .state,
                AttemptState::Suspended
            );
            assert!(f.restore(&service).await.is_err());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn missing_checkpoint_cannot_be_replaced_by_replaying_the_original_input() {
    for corrupt in [false, true] {
        let f = Fixture::parked().await;
        let snapshot = f.service.inspect_run(&f.binding.run_id).await.unwrap();
        let checkpoint = snapshot.attempts[&f.binding.attempt_id]
            .suspension
            .as_ref()
            .unwrap()
            .checkpoint
            .clone();
        if corrupt {
            std::fs::write(
                f._dir.path().join("objects").join(&checkpoint.artifact_id),
                b"corrupt checkpoint",
            )
            .unwrap();
        } else {
            f.service
                .artifacts()
                .delete(&tetonic_domain::ArtifactId::new(checkpoint.artifact_id))
                .await
                .unwrap();
        }
        let service = f.reopen();
        tokio::task::LocalSet::new()
            .run_until(async {
                assert!(f.restore(&service).await.is_err());
                assert_eq!(f.calls.load(Ordering::SeqCst), 2);
                assert!(service.binding(&f.binding.attempt_id).is_none());
            })
            .await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn malformed_suspension_or_live_sibling_still_requires_recovery() {
    let f = Fixture::parked().await;
    let snapshot = f.service.inspect_run(&f.binding.run_id).await.unwrap();
    let now = chrono::Utc::now().timestamp() as u64 + 86400;
    assert!(!tetonic_run::detect_recovery_required(&snapshot, now));
    for variant in 0..4 {
        let mut bad = snapshot.clone();
        let attempt = bad.attempts.get_mut(&f.binding.attempt_id).unwrap();
        match variant {
            0 => attempt.suspension = None,
            1 => attempt.execution_quiesced = false,
            2 => attempt.suspension.as_mut().unwrap().remaining_seconds = 0,
            _ => {
                let mut sibling = attempt.clone();
                sibling.attempt_id = tetonic_domain::AttemptId::new("live-sibling");
                sibling.state = AttemptState::Running;
                sibling.execution_quiesced = false;
                bad.attempts.insert(sibling.attempt_id.clone(), sibling);
            }
        }
        assert!(tetonic_run::detect_recovery_required(&bad, now));
    }
}
