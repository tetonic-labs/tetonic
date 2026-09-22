//! Durable disposition + audit hooks for M5-4.

use lokai_domain::result_integrity::WorkerBehaviorSignals;
use lokai_domain::ResultDisposition;
use lokai_fabric_protocol::ResultDispositionRecord;

/// Coordinator persistence for result dispositions (implemented by lokai-memory).
#[allow(clippy::too_many_arguments)]
#[async_trait::async_trait]
pub trait DispositionPersistence: Send + Sync {
    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        result_id: &str,
        idempotency_key: &str,
        disposition: ResultDisposition,
        reason: &str,
        audit_only: bool,
        worker_id: Option<&str>,
        trace_id: Option<&str>,
    ) -> Result<ResultDispositionRecord, String>;

    fn get_by_result(&self, result_id: &str) -> Result<Option<ResultDispositionRecord>, String>;

    fn get_by_idempotency(&self, key: &str) -> Result<Option<ResultDispositionRecord>, String>;

    fn persist_behavior_signals(
        &self,
        worker_id: &str,
        signals: &WorkerBehaviorSignals,
    ) -> Result<(), String>;

    fn audit_disposition(
        &self,
        session_id: Option<&str>,
        record: &ResultDispositionRecord,
    ) -> Result<(), String>;

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
        self.record(
            result_id,
            idempotency_key,
            disposition,
            reason,
            audit_only,
            worker_id,
            trace_id,
        )
    }

    async fn get_by_result_async(
        &self,
        result_id: &str,
    ) -> Result<Option<ResultDispositionRecord>, String> {
        self.get_by_result(result_id)
    }

    async fn get_by_idempotency_async(
        &self,
        key: &str,
    ) -> Result<Option<ResultDispositionRecord>, String> {
        self.get_by_idempotency(key)
    }

    async fn persist_behavior_signals_async(
        &self,
        worker_id: &str,
        signals: &WorkerBehaviorSignals,
    ) -> Result<(), String> {
        self.persist_behavior_signals(worker_id, signals)
    }
}

/// Convert a protocol disposition record for callers that already have one.
pub fn protocol_record(
    result_id: impl Into<String>,
    idempotency_key: impl Into<String>,
    disposition: ResultDisposition,
    reason: impl Into<String>,
    recorded_at: chrono::DateTime<chrono::Utc>,
    audit_only: bool,
) -> ResultDispositionRecord {
    ResultDispositionRecord {
        result_id: lokai_domain::ids::ResultId::new(result_id),
        idempotency_key: idempotency_key.into(),
        disposition,
        reason: reason.into(),
        recorded_at,
        audit_only,
    }
}
