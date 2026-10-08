//! Durable scheduler decisions (M6-2).

use rusqlite::{params, OptionalExtension};

use crate::util::now;
use crate::{Result, Store};

impl Store {
    pub(crate) fn migrate_scheduler_decisions_v22(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 22 {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS scheduler_decisions (\n\
                 decision_id            TEXT PRIMARY KEY,\n\
                 run_id                 TEXT NOT NULL,\n\
                 task_id                TEXT NOT NULL,\n\
                 attempt_id             TEXT NOT NULL,\n\
                 selected_target        TEXT NOT NULL,\n\
                 fallback_order_json    TEXT NOT NULL,\n\
                 reason                 TEXT NOT NULL,\n\
                 model_version          TEXT NOT NULL,\n\
                 expected_speedup       REAL,\n\
                 uncertainty_margin_ms  INTEGER NOT NULL,\n\
                 candidates_json        TEXT NOT NULL,\n\
                 decided_at             TEXT NOT NULL,\n\
                 pending                INTEGER NOT NULL DEFAULT 1\n\
             );\n\
             CREATE INDEX IF NOT EXISTS idx_scheduler_decisions_pending\n\
                 ON scheduler_decisions(pending);\n\
             CREATE INDEX IF NOT EXISTS idx_scheduler_decisions_attempt\n\
                 ON scheduler_decisions(attempt_id);",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (22, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_scheduler_decision_row(
        &self,
        decision_id: &str,
        run_id: &str,
        task_id: &str,
        attempt_id: &str,
        selected_target: &str,
        fallback_order_json: &str,
        reason: &str,
        model_version: &str,
        expected_speedup: Option<f64>,
        uncertainty_margin_ms: i64,
        candidates_json: &str,
        decided_at: &str,
        pending: bool,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO scheduler_decisions(\n\
                 decision_id, run_id, task_id, attempt_id, selected_target, fallback_order_json,\n\
                 reason, model_version, expected_speedup, uncertainty_margin_ms, candidates_json,\n\
                 decided_at, pending)\n\
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)\n\
             ON CONFLICT(decision_id) DO UPDATE SET\n\
                 run_id=excluded.run_id,\n\
                 task_id=excluded.task_id,\n\
                 attempt_id=excluded.attempt_id,\n\
                 selected_target=excluded.selected_target,\n\
                 fallback_order_json=excluded.fallback_order_json,\n\
                 reason=excluded.reason,\n\
                 model_version=excluded.model_version,\n\
                 expected_speedup=excluded.expected_speedup,\n\
                 uncertainty_margin_ms=excluded.uncertainty_margin_ms,\n\
                 candidates_json=excluded.candidates_json,\n\
                 decided_at=excluded.decided_at,\n\
                 pending=excluded.pending",
            params![
                decision_id,
                run_id,
                task_id,
                attempt_id,
                selected_target,
                fallback_order_json,
                reason,
                model_version,
                expected_speedup,
                uncertainty_margin_ms,
                candidates_json,
                decided_at,
                if pending { 1 } else { 0 },
            ],
        )?;
        Ok(())
    }

    pub fn get_scheduler_decision_row(
        &self,
        decision_id: &str,
    ) -> Result<Option<SchedulerDecisionRow>> {
        self.conn
            .query_row(
                "SELECT decision_id, run_id, task_id, attempt_id, selected_target, fallback_order_json,\n\
                        reason, model_version, expected_speedup, uncertainty_margin_ms, candidates_json,\n\
                        decided_at, pending\n\
                 FROM scheduler_decisions WHERE decision_id = ?1",
                params![decision_id],
                row_to_decision,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_pending_scheduler_decision_rows(&self) -> Result<Vec<SchedulerDecisionRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT decision_id, run_id, task_id, attempt_id, selected_target, fallback_order_json,\n\
                    reason, model_version, expected_speedup, uncertainty_margin_ms, candidates_json,\n\
                    decided_at, pending\n\
             FROM scheduler_decisions WHERE pending = 1",
        )?;
        let rows = stmt
            .query_map([], row_to_decision)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

#[derive(Clone, Debug)]
pub struct SchedulerDecisionRow {
    pub decision_id: String,
    pub run_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub selected_target: String,
    pub fallback_order_json: String,
    pub reason: String,
    pub model_version: String,
    pub expected_speedup: Option<f64>,
    pub uncertainty_margin_ms: i64,
    pub candidates_json: String,
    pub decided_at: String,
    pub pending: bool,
}

fn row_to_decision(r: &rusqlite::Row<'_>) -> rusqlite::Result<SchedulerDecisionRow> {
    Ok(SchedulerDecisionRow {
        decision_id: r.get(0)?,
        run_id: r.get(1)?,
        task_id: r.get(2)?,
        attempt_id: r.get(3)?,
        selected_target: r.get(4)?,
        fallback_order_json: r.get(5)?,
        reason: r.get(6)?,
        model_version: r.get(7)?,
        expected_speedup: r.get(8)?,
        uncertainty_margin_ms: r.get(9)?,
        candidates_json: r.get(10)?,
        decided_at: r.get(11)?,
        pending: r.get::<_, i64>(12)? != 0,
    })
}

#[cfg(test)]
mod tests {
    use crate::Store;

    #[test]
    fn scheduler_decision_round_trip_and_pending() {
        let store = Store::open(":memory:").unwrap();
        store
            .upsert_scheduler_decision_row(
                "sched_1",
                "run_1",
                "task_1",
                "att_1",
                "worker:w1",
                "[\"local\"]",
                "remote_faster",
                "weighted_v1",
                Some(1.5),
                40,
                "[]",
                "2026-01-01T00:00:00Z",
                true,
            )
            .unwrap();
        let got = store
            .get_scheduler_decision_row("sched_1")
            .unwrap()
            .unwrap();
        assert_eq!(got.selected_target, "worker:w1");
        assert!(got.pending);
        assert_eq!(
            store.list_pending_scheduler_decision_rows().unwrap().len(),
            1
        );
        store
            .upsert_scheduler_decision_row(
                "sched_1",
                "run_1",
                "task_1",
                "att_1",
                "worker:w1",
                "[\"local\"]",
                "remote_faster",
                "weighted_v1",
                Some(1.5),
                40,
                "[]",
                "2026-01-01T00:00:00Z",
                false,
            )
            .unwrap();
        assert!(store
            .list_pending_scheduler_decision_rows()
            .unwrap()
            .is_empty());
    }
}
