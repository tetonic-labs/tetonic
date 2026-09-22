//! Worker-side job ingress dedup (M5-1).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lokai_fabric_protocol::{
    DeliveryKey, FabricError, FabricErrorCode, IngressDecision, IngressRecord, IngressState,
    JobEnvelope, TerminalOutcome,
};
use lokai_memory::WorkerStore;
use tokio::sync::Mutex;

pub struct JobIngressManager {
    inner: Mutex<IngressState>,
    db_path: PathBuf,
}

impl JobIngressManager {
    pub fn load(db_path: PathBuf) -> Arc<Self> {
        let records = WorkerStore::open(&db_path)
            .ok()
            .and_then(|store| store.load_fabric_ingress_records().ok())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|json| serde_json::from_str::<IngressRecord>(&json).ok())
            .collect::<Vec<_>>();
        Arc::new(Self {
            inner: Mutex::new(IngressState::from_records(records)),
            db_path,
        })
    }

    pub async fn accept(&self, job: &JobEnvelope) -> Result<IngressDecision, FabricError> {
        if job.job_kind != lokai_fabric_protocol::JobKind::Infer {
            return Err(FabricError {
                code: lokai_fabric_protocol::FabricErrorCode::UnsupportedJobKind,
                message: format!("worker does not implement {:?}", job.job_kind),
                details: None,
            });
        }
        let (decision, persisted) = {
            let mut state = self.inner.lock().await;
            let decision = state.accept_delivery(job)?;
            let persisted = if matches!(decision, IngressDecision::AcceptNew) {
                record_for_job(&state, job)
            } else {
                None
            };
            (decision, persisted)
        };
        if let Some((key, json)) = persisted {
            if let Err(e) = persist_record(&self.db_path, key, json).await {
                {
                    let mut state = self.inner.lock().await;
                    let _ =
                        state.mark_terminal(job, TerminalOutcome::Failed(e.message.clone()), None);
                }
                return Err(e);
            }
        }
        Ok(decision)
    }

    pub async fn mark_terminal(
        &self,
        job: &JobEnvelope,
        outcome: TerminalOutcome,
        cached_response_json: Option<String>,
    ) -> Result<(), FabricError> {
        let persisted = {
            let mut state = self.inner.lock().await;
            state.mark_terminal(job, outcome, cached_response_json)?;
            record_for_job(&state, job)
        };
        if let Some((key, json)) = persisted {
            persist_record(&self.db_path, key, json).await?;
        }
        Ok(())
    }
}

fn record_for_job(state: &IngressState, job: &JobEnvelope) -> Option<(String, String)> {
    let digest = DeliveryKey::from_job(job).digest();
    state
        .records()
        .into_iter()
        .find(|r| r.delivery_key.digest() == digest)
        .map(|r| {
            (
                r.delivery_key.digest(),
                serde_json::to_string(&r).unwrap_or_default(),
            )
        })
}

async fn persist_record(db_path: &Path, key: String, json: String) -> Result<(), FabricError> {
    let db_path = db_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        WorkerStore::open(&db_path)
            .and_then(|store| store.upsert_fabric_ingress_record(&key, &json))
            .map_err(|e| FabricError {
                code: FabricErrorCode::InternalFailure,
                message: format!("ingress persist failed: {e}"),
                details: None,
            })
    })
    .await
    .unwrap_or_else(|e| {
        Err(FabricError {
            code: FabricErrorCode::InternalFailure,
            message: format!("ingress persist join: {e}"),
            details: None,
        })
    })
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use lokai_domain::ids::{AttemptId, JobId, LeaseId, RunId, TaskId};
    use lokai_domain::workspace::ContentDigest;
    use lokai_fabric_protocol::{
        IdempotencyKey, JobDeadlines, JobEnvelope, JobKind, JobRequirements, OutputLimits,
        ResourceLimits, VerificationPolicyReference, VersionedJobPayload,
    };
    use lokai_inference::DataClass;

    use super::*;

    fn sample_job(attempt: &str) -> JobEnvelope {
        JobEnvelope {
            job_id: JobId::new("job_1"),
            run_id: RunId::new("run_1"),
            task_id: TaskId::new("task_1"),
            task_version: 1,
            attempt_id: AttemptId::new(attempt),
            idempotency_key: IdempotencyKey(format!("{attempt}:job_1")),
            lease_id: LeaseId::new(attempt),
            lease_epoch: 1,
            job_kind: JobKind::Infer,
            input_digest: ContentDigest::new("digest_1"),
            workspace_version: None,
            input_artifacts: vec![],
            data_class: DataClass::RepositorySource,
            required_capabilities: JobRequirements::default(),
            resource_limits: ResourceLimits::default(),
            deadlines: JobDeadlines::from_execution(Utc::now() + Duration::hours(1), 3600),
            output_limits: OutputLimits {
                max_bytes: 1024,
                max_artifacts: 1,
                max_artifact_size: 512,
            },
            verification_policy: VerificationPolicyReference::default(),
            payload: VersionedJobPayload::V1Infer(serde_json::json!({"model": "qwen"})),
        }
    }

    #[tokio::test]
    async fn ingress_persists_on_accept_and_survives_reload() {
        let dir = std::env::temp_dir().join(format!(
            "lokai_ingress_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("worker.db");

        let job = sample_job("att_persist");
        let mgr = JobIngressManager::load(db_path.clone());
        assert_eq!(mgr.accept(&job).await.unwrap(), IngressDecision::AcceptNew);

        let reloaded = JobIngressManager::load(db_path.clone());
        assert_eq!(
            reloaded.accept(&job).await.unwrap(),
            IngressDecision::DuplicateInFlight
        );

        reloaded
            .mark_terminal(
                &job,
                TerminalOutcome::Completed,
                Some(r#"{"ok":true}"#.into()),
            )
            .await
            .unwrap();

        let after_terminal = JobIngressManager::load(db_path);
        assert!(matches!(
            after_terminal.accept(&job).await.unwrap(),
            IngressDecision::ReplayExisting(_)
        ));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn ingress_refuses_unimplemented_job_kind() {
        let dir = std::env::temp_dir().join(format!(
            "lokai_ingress_jobkind_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("worker.db");
        let mut job = sample_job("att_embed");
        job.job_kind = JobKind::Embed;
        let mgr = JobIngressManager::load(db_path);
        let err = mgr.accept(&job).await.expect_err("embed refused");
        assert_eq!(
            err.code,
            lokai_fabric_protocol::FabricErrorCode::UnsupportedJobKind
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn reject_after_accept_is_not_duplicate_in_flight() {
        let dir = std::env::temp_dir().join(format!(
            "lokai_ingress_reject_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("worker.db");
        let job = sample_job("att_reject");
        let mgr = JobIngressManager::load(db_path);
        assert_eq!(mgr.accept(&job).await.unwrap(), IngressDecision::AcceptNew);
        mgr.mark_terminal(
            &job,
            TerminalOutcome::Rejected("lease_renewal_canceled".into()),
            Some(r#"{"ok":false}"#.into()),
        )
        .await
        .unwrap();
        assert!(
            matches!(
                mgr.accept(&job).await.unwrap(),
                IngressDecision::ReplayExisting(_)
            ),
            "terminal reject must not leave DuplicateInFlight"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn persist_err_does_not_report_success() {
        let dir = std::env::temp_dir().join(format!(
            "lokai_ingress_persist_err_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("worker.db");
        std::fs::create_dir_all(&db_path).unwrap();
        let job = sample_job("att_persist_err");
        let mgr = JobIngressManager::load(db_path);
        let err = mgr.accept(&job).await.expect_err("persist must surface");
        assert_eq!(err.code, FabricErrorCode::InternalFailure);
        assert!(
            !matches!(
                mgr.accept(&job).await.unwrap(),
                IngressDecision::DuplicateInFlight
            ),
            "failed persist after admit must not leave DuplicateInFlight"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
