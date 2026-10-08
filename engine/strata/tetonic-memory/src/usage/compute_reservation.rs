//! Durable compute reservations (M6-1).

use rusqlite::{params, OptionalExtension};

use crate::util::now;
use crate::{Result, Store};

impl Store {
    pub(crate) fn migrate_compute_reservations_v21(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 21 {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS compute_reservations (\n\
                 reservation_id   TEXT PRIMARY KEY,\n\
                 run_id           TEXT NOT NULL,\n\
                 task_id          TEXT NOT NULL,\n\
                 attempt_id       TEXT NOT NULL UNIQUE,\n\
                 target_scope     TEXT NOT NULL,\n\
                 resources_json   TEXT NOT NULL,\n\
                 issued_at        TEXT NOT NULL,\n\
                 expires_at       TEXT NOT NULL,\n\
                 reservation_epoch INTEGER NOT NULL,\n\
                 state            TEXT NOT NULL,\n\
                 speculative      INTEGER NOT NULL DEFAULT 0,\n\
                 updated_at       TEXT NOT NULL\n\
             );\n\
             CREATE INDEX IF NOT EXISTS idx_compute_reservations_state\n\
                 ON compute_reservations(state);\n\
             CREATE INDEX IF NOT EXISTS idx_compute_reservations_attempt\n\
                 ON compute_reservations(attempt_id);",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (21, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_compute_reservation_row(
        &self,
        reservation_id: &str,
        run_id: &str,
        task_id: &str,
        attempt_id: &str,
        target_scope: &str,
        resources_json: &str,
        issued_at: &str,
        expires_at: &str,
        reservation_epoch: i64,
        state: &str,
        speculative: bool,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO compute_reservations(\n\
                 reservation_id, run_id, task_id, attempt_id, target_scope, resources_json,\n\
                 issued_at, expires_at, reservation_epoch, state, speculative, updated_at)\n\
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)\n\
             ON CONFLICT(reservation_id) DO UPDATE SET\n\
                 run_id=excluded.run_id,\n\
                 task_id=excluded.task_id,\n\
                 attempt_id=excluded.attempt_id,\n\
                 target_scope=excluded.target_scope,\n\
                 resources_json=excluded.resources_json,\n\
                 issued_at=excluded.issued_at,\n\
                 expires_at=excluded.expires_at,\n\
                 reservation_epoch=excluded.reservation_epoch,\n\
                 state=excluded.state,\n\
                 speculative=excluded.speculative,\n\
                 updated_at=excluded.updated_at",
            params![
                reservation_id,
                run_id,
                task_id,
                attempt_id,
                target_scope,
                resources_json,
                issued_at,
                expires_at,
                reservation_epoch,
                state,
                if speculative { 1 } else { 0 },
                now(),
            ],
        )?;
        Ok(())
    }

    pub fn get_compute_reservation_row(
        &self,
        reservation_id: &str,
    ) -> Result<Option<ComputeReservationRow>> {
        self.conn
            .query_row(
                "SELECT reservation_id, run_id, task_id, attempt_id, target_scope, resources_json,\n\
                        issued_at, expires_at, reservation_epoch, state, speculative\n\
                 FROM compute_reservations WHERE reservation_id = ?1",
                params![reservation_id],
                row_to_reservation,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_active_compute_reservation_rows(&self) -> Result<Vec<ComputeReservationRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT reservation_id, run_id, task_id, attempt_id, target_scope, resources_json,\n\
                    issued_at, expires_at, reservation_epoch, state, speculative\n\
             FROM compute_reservations\n\
             WHERE state IN ('reserved','dispatched','running')",
        )?;
        let rows = stmt
            .query_map([], row_to_reservation)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn mark_compute_reservation_released_row(
        &self,
        reservation_id: &str,
        state: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE compute_reservations SET state = ?1, updated_at = ?2 WHERE reservation_id = ?3",
            params![state, now(), reservation_id],
        )?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ComputeReservationRow {
    pub reservation_id: String,
    pub run_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub target_scope: String,
    pub resources_json: String,
    pub issued_at: String,
    pub expires_at: String,
    pub reservation_epoch: i64,
    pub state: String,
    pub speculative: bool,
}

fn row_to_reservation(r: &rusqlite::Row<'_>) -> rusqlite::Result<ComputeReservationRow> {
    Ok(ComputeReservationRow {
        reservation_id: r.get(0)?,
        run_id: r.get(1)?,
        task_id: r.get(2)?,
        attempt_id: r.get(3)?,
        target_scope: r.get(4)?,
        resources_json: r.get(5)?,
        issued_at: r.get(6)?,
        expires_at: r.get(7)?,
        reservation_epoch: r.get(8)?,
        state: r.get(9)?,
        speculative: r.get::<_, i64>(10)? != 0,
    })
}
