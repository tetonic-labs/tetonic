//! Durable result disposition records (M5-4).

use lokai_domain::ids::ResultId;
use lokai_domain::ResultDisposition;
use rusqlite::{params, OptionalExtension};

use crate::util::now;
use crate::{Result, Store};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DurableDispositionRecord {
    pub result_id: String,
    pub idempotency_key: String,
    pub disposition: String,
    pub reason: String,
    pub recorded_at: String,
    pub audit_only: bool,
    pub worker_id: Option<String>,
    pub trace_id: Option<String>,
}

impl Store {
    pub(crate) fn migrate_result_disposition_v20(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 20 {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS result_dispositions (\n\
                 result_id        TEXT PRIMARY KEY,\n\
                 idempotency_key  TEXT NOT NULL UNIQUE,\n\
                 disposition      TEXT NOT NULL,\n\
                 reason           TEXT NOT NULL,\n\
                 recorded_at      TEXT NOT NULL,\n\
                 audit_only       INTEGER NOT NULL DEFAULT 0,\n\
                 worker_id        TEXT,\n\
                 trace_id         TEXT\n\
             );\n\
             CREATE INDEX IF NOT EXISTS idx_result_dispositions_idem\n\
                 ON result_dispositions(idempotency_key);\n\
             CREATE TABLE IF NOT EXISTS worker_behavior_signals (\n\
                 worker_id              TEXT PRIMARY KEY,\n\
                 invalid_signatures     INTEGER NOT NULL DEFAULT 0,\n\
                 digest_mismatches      INTEGER NOT NULL DEFAULT 0,\n\
                 stale_results          INTEGER NOT NULL DEFAULT 0,\n\
                 lease_violations       INTEGER NOT NULL DEFAULT 0,\n\
                 oversized_responses    INTEGER NOT NULL DEFAULT 0,\n\
                 schema_violations      INTEGER NOT NULL DEFAULT 0,\n\
                 verification_failures  INTEGER NOT NULL DEFAULT 0,\n\
                 independence_failures  INTEGER NOT NULL DEFAULT 0,\n\
                 malformed_output       INTEGER NOT NULL DEFAULT 0,\n\
                 updated_at             TEXT NOT NULL\n\
             );",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (20, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    /// Idempotent durable record; returns the existing row on duplicate keys.
    #[allow(clippy::too_many_arguments)]
    pub fn record_result_disposition(
        &self,
        result_id: &str,
        idempotency_key: &str,
        disposition: ResultDisposition,
        reason: &str,
        audit_only: bool,
        worker_id: Option<&str>,
        trace_id: Option<&str>,
    ) -> Result<DurableDispositionRecord> {
        if let Some(existing) = self.get_result_disposition(result_id)? {
            return Ok(existing);
        }
        if let Some(existing) = self.get_result_disposition_by_idempotency(idempotency_key)? {
            return Ok(existing);
        }
        let recorded_at = now();
        let _ = self.conn.execute(
            "INSERT OR IGNORE INTO result_dispositions(\n\
                 result_id, idempotency_key, disposition, reason, recorded_at, audit_only,\n\
                 worker_id, trace_id)\n\
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                result_id,
                idempotency_key,
                disposition.as_str(),
                reason,
                recorded_at,
                if audit_only { 1 } else { 0 },
                worker_id,
                trace_id,
            ],
        )?;
        if let Some(existing) = self.get_result_disposition(result_id)? {
            return Ok(existing);
        }
        Ok(DurableDispositionRecord {
            result_id: result_id.to_string(),
            idempotency_key: idempotency_key.to_string(),
            disposition: disposition.as_str().to_string(),
            reason: reason.to_string(),
            recorded_at,
            audit_only,
            worker_id: worker_id.map(str::to_string),
            trace_id: trace_id.map(str::to_string),
        })
    }

    pub fn get_result_disposition(
        &self,
        result_id: &str,
    ) -> Result<Option<DurableDispositionRecord>> {
        self.conn
            .query_row(
                "SELECT result_id, idempotency_key, disposition, reason, recorded_at, audit_only,\n\
                        worker_id, trace_id\n\
                 FROM result_dispositions WHERE result_id = ?1",
                params![result_id],
                row_to_record,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn get_result_disposition_by_idempotency(
        &self,
        key: &str,
    ) -> Result<Option<DurableDispositionRecord>> {
        self.conn
            .query_row(
                "SELECT result_id, idempotency_key, disposition, reason, recorded_at, audit_only,\n\
                        worker_id, trace_id\n\
                 FROM result_dispositions WHERE idempotency_key = ?1",
                params![key],
                row_to_record,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn upsert_worker_behavior_signals(
        &self,
        worker_id: &str,
        signals: &lokai_domain::WorkerBehaviorSignals,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO worker_behavior_signals(\n\
                 worker_id, invalid_signatures, digest_mismatches, stale_results,\n\
                 lease_violations, oversized_responses, schema_violations,\n\
                 verification_failures, independence_failures, malformed_output, updated_at)\n\
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)\n\
             ON CONFLICT(worker_id) DO UPDATE SET\n\
                 invalid_signatures=excluded.invalid_signatures,\n\
                 digest_mismatches=excluded.digest_mismatches,\n\
                 stale_results=excluded.stale_results,\n\
                 lease_violations=excluded.lease_violations,\n\
                 oversized_responses=excluded.oversized_responses,\n\
                 schema_violations=excluded.schema_violations,\n\
                 verification_failures=excluded.verification_failures,\n\
                 independence_failures=excluded.independence_failures,\n\
                 malformed_output=excluded.malformed_output,\n\
                 updated_at=excluded.updated_at",
            params![
                worker_id,
                signals.invalid_signatures as i64,
                signals.digest_mismatches as i64,
                signals.stale_results as i64,
                signals.lease_violations as i64,
                signals.oversized_responses as i64,
                signals.schema_violations as i64,
                signals.verification_failures as i64,
                signals.independence_failures as i64,
                signals.malformed_output as i64,
                now(),
            ],
        )?;
        Ok(())
    }

    /// Append a coordinator audit event for a result disposition (session-scoped when known).
    pub fn audit_result_disposition(
        &self,
        session_id: Option<&str>,
        record: &DurableDispositionRecord,
    ) -> Result<()> {
        let payload = serde_json::json!({
            "result_id": record.result_id,
            "idempotency_key": record.idempotency_key,
            "disposition": record.disposition,
            "reason": record.reason,
            "recorded_at": record.recorded_at,
            "audit_only": record.audit_only,
            "worker_id": record.worker_id,
            "trace_id": record.trace_id,
        })
        .to_string();
        if let Some(sid) = session_id {
            let _ = self.append_event(sid, "result_disposition", "fabric", &payload)?;
        } else {
            // Disposition row is already durable; ResultId keeps audit identity typed.
            let _ = ResultId::new(&record.result_id);
        }
        Ok(())
    }
}

fn row_to_record(r: &rusqlite::Row<'_>) -> rusqlite::Result<DurableDispositionRecord> {
    Ok(DurableDispositionRecord {
        result_id: r.get(0)?,
        idempotency_key: r.get(1)?,
        disposition: r.get(2)?,
        reason: r.get(3)?,
        recorded_at: r.get(4)?,
        audit_only: r.get::<_, i64>(5)? != 0,
        worker_id: r.get(6)?,
        trace_id: r.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn disposition_round_trip_idempotent() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join("lokai.db")).unwrap();
        let first = store
            .record_result_disposition(
                "res_1",
                "idem_1",
                ResultDisposition::Accepted,
                "ok",
                false,
                Some("w1"),
                Some("trace"),
            )
            .unwrap();
        let second = store
            .record_result_disposition(
                "res_1",
                "idem_1",
                ResultDisposition::RejectedPolicy,
                "should not replace",
                false,
                Some("w1"),
                Some("trace"),
            )
            .unwrap();
        assert_eq!(first.disposition, second.disposition);
        assert_eq!(second.reason, "ok");
    }
}
