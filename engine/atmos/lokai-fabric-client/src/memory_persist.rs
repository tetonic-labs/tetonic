//! lokai-memory-backed disposition persistence (M5-4).

use chrono::{DateTime, TimeZone, Utc};
use lokai_domain::ids::ResultId;
use lokai_domain::result_integrity::WorkerBehaviorSignals;
use lokai_domain::ResultDisposition;
use lokai_fabric_protocol::ResultDispositionRecord;
use lokai_memory::{DurableDispositionRecord, SharedStore};

use crate::disposition_persist::DispositionPersistence;

pub struct StoreDispositionPersist {
    store: SharedStore,
}

impl StoreDispositionPersist {
    pub fn new(store: SharedStore) -> Self {
        Self { store }
    }
}

#[async_trait::async_trait]
impl DispositionPersistence for StoreDispositionPersist {
    fn record(
        &self,
        result_id: &str,
        idempotency_key: &str,
        disposition: ResultDisposition,
        reason: &str,
        audit_only: bool,
        worker_id: Option<&str>,
        trace_id: Option<&str>,
    ) -> Result<ResultDispositionRecord, String> {
        self.store
            .write_sync({
                let result_id = result_id.to_string();
                let idempotency_key = idempotency_key.to_string();
                let reason = reason.to_string();
                let worker_id = worker_id.map(|s| s.to_string());
                let trace_id = trace_id.map(|s| s.to_string());
                move |db| {
                    let row = db
                        .record_result_disposition(
                            &result_id,
                            &idempotency_key,
                            disposition,
                            &reason,
                            audit_only,
                            worker_id.as_deref(),
                            trace_id.as_deref(),
                        )
                        .map_err(|e| e.to_string())?;
                    Ok(to_protocol(&row))
                }
            })
            .map_err(|e| e.to_string())?
    }

    async fn record_async(
        &self,
        result_id: &str,
        idempotency_key: &str,
        disposition: ResultDisposition,
        reason: &str,
        audit_only: bool,
        worker_id: Option<&str>,
        trace_id: Option<&str>,
    ) -> Result<ResultDispositionRecord, String> {
        self.store
            .write({
                let result_id = result_id.to_string();
                let idempotency_key = idempotency_key.to_string();
                let reason = reason.to_string();
                let worker_id = worker_id.map(String::from);
                let trace_id = trace_id.map(String::from);
                move |db| {
                    let row = db
                        .record_result_disposition(
                            &result_id,
                            &idempotency_key,
                            disposition,
                            &reason,
                            audit_only,
                            worker_id.as_deref(),
                            trace_id.as_deref(),
                        )
                        .map_err(|e| e.to_string())?;
                    Ok(to_protocol(&row))
                }
            })
            .await
            .map_err(|e| e.to_string())?
    }

    fn get_by_result(&self, result_id: &str) -> Result<Option<ResultDispositionRecord>, String> {
        self.store
            .read_sync({
                let result_id = result_id.to_string();
                move |db| {
                    Ok(db
                        .get_result_disposition(&result_id)
                        .map_err(|e| e.to_string())?
                        .map(|r| to_protocol(&r)))
                }
            })
            .map_err(|e| e.to_string())?
    }

    async fn get_by_result_async(
        &self,
        result_id: &str,
    ) -> Result<Option<ResultDispositionRecord>, String> {
        self.store
            .read({
                let result_id = result_id.to_string();
                move |db| {
                    Ok(db
                        .get_result_disposition(&result_id)
                        .map_err(|e| e.to_string())?
                        .map(|r| to_protocol(&r)))
                }
            })
            .await
            .map_err(|e| e.to_string())?
    }

    fn get_by_idempotency(&self, key: &str) -> Result<Option<ResultDispositionRecord>, String> {
        self.store
            .read_sync({
                let key = key.to_string();
                move |db| {
                    Ok(db
                        .get_result_disposition_by_idempotency(&key)
                        .map_err(|e| e.to_string())?
                        .map(|r| to_protocol(&r)))
                }
            })
            .map_err(|e| e.to_string())?
    }

    async fn get_by_idempotency_async(
        &self,
        key: &str,
    ) -> Result<Option<ResultDispositionRecord>, String> {
        self.store
            .read({
                let key = key.to_string();
                move |db| {
                    Ok(db
                        .get_result_disposition_by_idempotency(&key)
                        .map_err(|e| e.to_string())?
                        .map(|r| to_protocol(&r)))
                }
            })
            .await
            .map_err(|e| e.to_string())?
    }

    fn persist_behavior_signals(
        &self,
        worker_id: &str,
        signals: &WorkerBehaviorSignals,
    ) -> Result<(), String> {
        self.store
            .write_sync({
                let worker_id = worker_id.to_string();
                let signals = signals.clone();
                move |db| {
                    db.upsert_worker_behavior_signals(&worker_id, &signals)
                        .map_err(|e| e.to_string())
                }
            })
            .map_err(|e| e.to_string())?
    }

    fn audit_disposition(
        &self,
        session_id: Option<&str>,
        record: &ResultDispositionRecord,
    ) -> Result<(), String> {
        self.store
            .write_sync({
                let session_id = session_id.map(|s| s.to_string());
                let durable = DurableDispositionRecord {
                    result_id: record.result_id.0.clone(),
                    idempotency_key: record.idempotency_key.clone(),
                    disposition: record.disposition.as_str().to_string(),
                    reason: record.reason.clone(),
                    recorded_at: record.recorded_at.to_rfc3339(),
                    audit_only: record.audit_only,
                    worker_id: None,
                    trace_id: None,
                };
                move |db| {
                    db.audit_result_disposition(session_id.as_deref(), &durable)
                        .map_err(|e| e.to_string())
                }
            })
            .map_err(|e| e.to_string())?
    }
}

fn to_protocol(row: &DurableDispositionRecord) -> ResultDispositionRecord {
    ResultDispositionRecord {
        result_id: ResultId::new(&row.result_id),
        idempotency_key: row.idempotency_key.clone(),
        disposition: parse_disp(&row.disposition),
        reason: row.reason.clone(),
        recorded_at: DateTime::parse_from_rfc3339(&row.recorded_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc.timestamp_opt(0, 0).single().unwrap_or_else(Utc::now)),
        audit_only: row.audit_only,
    }
}

fn parse_disp(raw: &str) -> ResultDisposition {
    match raw {
        "accepted" => ResultDisposition::Accepted,
        "quarantined" => ResultDisposition::Quarantined,
        "rejected_invalid_signature" => ResultDisposition::RejectedInvalidSignature,
        "rejected_identity_mismatch" => ResultDisposition::RejectedIdentityMismatch,
        "rejected_revoked_worker" => ResultDisposition::RejectedRevokedWorker,
        "rejected_stale_attempt" => ResultDisposition::RejectedStaleAttempt,
        "rejected_canceled" => ResultDisposition::RejectedCanceled,
        "rejected_digest_mismatch" => ResultDisposition::RejectedDigestMismatch,
        "rejected_schema" => ResultDisposition::RejectedSchema,
        "rejected_policy" => ResultDisposition::RejectedPolicy,
        "rejected_verification" => ResultDisposition::RejectedVerification,
        "rejected_oversized" => ResultDisposition::RejectedOversized,
        "rejected_path_unsafe" => ResultDisposition::RejectedPathUnsafe,
        "rejected_executable_payload" => ResultDisposition::RejectedExecutablePayload,
        "superseded" => ResultDisposition::Superseded,
        _ => ResultDisposition::RejectedSchema,
    }
}
