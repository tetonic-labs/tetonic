//! Estate persistence — `owner_identity` + `worker_enrollments` (memory-store-v2 § estate).

use rusqlite::OptionalExtension;

use crate::{new_id, now, Result, Store};

#[derive(Debug, Clone)]
pub struct OwnerIdentityRow {
    pub id: String,
    pub label: String,
    pub operator_pubkey: Vec<u8>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct WorkerEnrollmentRow {
    pub id: String,
    pub estate_id: String,
    pub label: String,
    pub host: String,
    pub ip: String,
    pub worker_pubkey: Vec<u8>,
    pub audit_pubkey: Vec<u8>,
    pub fabric_port: u16,
    pub coordinator_pubkeys_json: String,
    pub enrolled_at: String,
    pub last_seen: Option<String>,
    pub fabric_tls_cert: Option<Vec<u8>>,
    /// Coordinator-assigned trust tier (M5-3); persisted after v19 migration.
    pub worker_trust: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorkerTrustAuditRow {
    pub worker_id: String,
    pub trust: String,
    pub policy_epoch: u64,
    pub recorded_at: String,
    pub source: String,
}

impl Store {
    pub fn migrate_estate_v4(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 4 {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS owner_identity (\n\
                 id               TEXT PRIMARY KEY,\n\
                 label            TEXT NOT NULL,\n\
                 operator_pubkey  BLOB NOT NULL,\n\
                 created_at       TEXT NOT NULL\n\
             );\n\
             CREATE TABLE IF NOT EXISTS worker_enrollments (\n\
                 id                     TEXT PRIMARY KEY,\n\
                 estate_id              TEXT NOT NULL REFERENCES owner_identity(id),\n\
                 label                  TEXT NOT NULL,\n\
                 host                   TEXT NOT NULL,\n\
                 ip                     TEXT NOT NULL,\n\
                 worker_pubkey          BLOB NOT NULL,\n\
                 audit_pubkey           BLOB NOT NULL,\n\
                 fabric_port            INTEGER NOT NULL,\n\
                 coordinator_pubkeys_json TEXT NOT NULL,\n\
                 enrolled_at            TEXT NOT NULL,\n\
                 last_seen              TEXT\n\
             );\n\
             CREATE INDEX IF NOT EXISTS idx_worker_enrollments_estate\n\
                 ON worker_enrollments(estate_id);",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (4, ?1)",
            rusqlite::params![now()],
        )?;
        Ok(())
    }

    pub fn ensure_owner_identity(
        &self,
        operator_pubkey: &[u8],
        label: &str,
    ) -> Result<OwnerIdentityRow> {
        let existing: Option<OwnerIdentityRow> = self
            .conn
            .query_row(
                "SELECT id, label, operator_pubkey, created_at FROM owner_identity LIMIT 1",
                [],
                |r| {
                    Ok(OwnerIdentityRow {
                        id: r.get(0)?,
                        label: r.get(1)?,
                        operator_pubkey: r.get(2)?,
                        created_at: r.get(3)?,
                    })
                },
            )
            .optional()?;
        if let Some(row) = existing {
            return Ok(row);
        }
        let id = new_id("estate");
        let created = now();
        self.conn.execute(
            "INSERT INTO owner_identity(id, label, operator_pubkey, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, label, operator_pubkey, created],
        )?;
        Ok(OwnerIdentityRow {
            id,
            label: label.to_string(),
            operator_pubkey: operator_pubkey.to_vec(),
            created_at: created,
        })
    }

    pub fn upsert_worker_enrollment(&self, row: &WorkerEnrollmentRow) -> Result<()> {
        let trust = row
            .worker_trust
            .as_deref()
            .unwrap_or("owner_controlled_estate");
        self.conn.execute(
            "INSERT INTO worker_enrollments(\n\
                 id, estate_id, label, host, ip, worker_pubkey, audit_pubkey,\n\
                 fabric_port, coordinator_pubkeys_json, enrolled_at, last_seen, fabric_tls_cert,\n\
                 worker_trust\n\
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)\n\
             ON CONFLICT(id) DO UPDATE SET\n\
                 label=excluded.label, host=excluded.host, ip=excluded.ip,\n\
                 worker_pubkey=excluded.worker_pubkey, audit_pubkey=excluded.audit_pubkey,\n\
                 fabric_port=excluded.fabric_port,\n\
                 coordinator_pubkeys_json=excluded.coordinator_pubkeys_json,\n\
                 last_seen=excluded.last_seen,\n\
                 fabric_tls_cert=excluded.fabric_tls_cert,\n\
                 worker_trust=COALESCE(excluded.worker_trust, worker_enrollments.worker_trust)",
            rusqlite::params![
                row.id,
                row.estate_id,
                row.label,
                row.host,
                row.ip,
                row.worker_pubkey,
                row.audit_pubkey,
                row.fabric_port,
                row.coordinator_pubkeys_json,
                row.enrolled_at,
                row.last_seen,
                row.fabric_tls_cert,
                trust,
            ],
        )?;
        Ok(())
    }

    pub fn list_worker_enrollments(&self) -> Result<Vec<WorkerEnrollmentRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, estate_id, label, host, ip, worker_pubkey, audit_pubkey,\n\
                    fabric_port, coordinator_pubkeys_json, enrolled_at, last_seen,\n\
                    fabric_tls_cert, worker_trust\n\
             FROM worker_enrollments ORDER BY enrolled_at",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(WorkerEnrollmentRow {
                    id: r.get(0)?,
                    estate_id: r.get(1)?,
                    label: r.get(2)?,
                    host: r.get(3)?,
                    ip: r.get(4)?,
                    worker_pubkey: r.get(5)?,
                    audit_pubkey: r.get(6)?,
                    fabric_port: r.get(7)?,
                    coordinator_pubkeys_json: r.get(8)?,
                    enrolled_at: r.get(9)?,
                    last_seen: r.get(10)?,
                    fabric_tls_cert: r.get(11)?,
                    worker_trust: r.get(12).ok(),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn migrate_estate_v19(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 19 {
            return Ok(());
        }
        let _ = self.conn.execute(
            "ALTER TABLE worker_enrollments ADD COLUMN worker_trust TEXT NOT NULL DEFAULT 'owner_controlled_estate'",
            [],
        );
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS worker_trust_audit (\n\
                 id           INTEGER PRIMARY KEY,\n\
                 worker_id    TEXT NOT NULL,\n\
                 trust        TEXT NOT NULL,\n\
                 policy_epoch INTEGER NOT NULL,\n\
                 recorded_at  TEXT NOT NULL,\n\
                 source       TEXT NOT NULL\n\
             );\n\
             CREATE INDEX IF NOT EXISTS idx_worker_trust_audit_worker\n\
                 ON worker_trust_audit(worker_id);",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (19, ?1)",
            rusqlite::params![now()],
        )?;
        Ok(())
    }

    pub fn set_worker_trust(
        &self,
        worker_id: &str,
        trust: &str,
        policy_epoch: u64,
        source: &str,
    ) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE worker_enrollments SET worker_trust = ?1 WHERE id = ?2 OR label = ?2",
            rusqlite::params![trust, worker_id],
        )?;
        if n == 0 {
            return Ok(false);
        }
        self.conn.execute(
            "INSERT INTO worker_trust_audit(worker_id, trust, policy_epoch, recorded_at, source)\n\
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![worker_id, trust, policy_epoch as i64, now(), source],
        )?;
        Ok(true)
    }

    pub fn worker_trust(&self, worker_id: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT worker_trust FROM worker_enrollments WHERE id = ?1 OR label = ?1 LIMIT 1",
                rusqlite::params![worker_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Highest recorded trust-policy epoch across all workers (M5-3 daemon bootstrap).
    pub fn max_worker_trust_policy_epoch(&self) -> Result<u64> {
        let epoch: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(policy_epoch), 0) FROM worker_trust_audit",
            [],
            |r| r.get(0),
        )?;
        Ok(epoch.max(0) as u64)
    }

    pub fn list_worker_trust_audit(&self, worker_id: &str) -> Result<Vec<WorkerTrustAuditRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT worker_id, trust, policy_epoch, recorded_at, source\n\
             FROM worker_trust_audit WHERE worker_id = ?1 ORDER BY id DESC",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![worker_id], |r| {
                Ok(WorkerTrustAuditRow {
                    worker_id: r.get(0)?,
                    trust: r.get(1)?,
                    policy_epoch: r.get::<_, i64>(2)? as u64,
                    recorded_at: r.get(3)?,
                    source: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn migrate_estate_v5(&self) -> Result<()> {
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;
        if applied >= 5 {
            return Ok(());
        }
        let _ = self.conn.execute(
            "ALTER TABLE worker_enrollments ADD COLUMN fabric_tls_cert BLOB",
            [],
        );
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (5, ?1)",
            rusqlite::params![now()],
        )?;
        Ok(())
    }

    pub fn find_worker_enrollment(&self, id_or_label: &str) -> Result<Option<WorkerEnrollmentRow>> {
        let map = |r: &rusqlite::Row<'_>| {
            Ok(WorkerEnrollmentRow {
                id: r.get(0)?,
                estate_id: r.get(1)?,
                label: r.get(2)?,
                host: r.get(3)?,
                ip: r.get(4)?,
                worker_pubkey: r.get(5)?,
                audit_pubkey: r.get(6)?,
                fabric_port: r.get(7)?,
                coordinator_pubkeys_json: r.get(8)?,
                enrolled_at: r.get(9)?,
                last_seen: r.get(10)?,
                fabric_tls_cert: r.get(11)?,
                worker_trust: r.get(12).ok(),
            })
        };
        self.conn
            .query_row(
                "SELECT id, estate_id, label, host, ip, worker_pubkey, audit_pubkey,
                        fabric_port, coordinator_pubkeys_json, enrolled_at, last_seen,
                        fabric_tls_cert, worker_trust
                 FROM worker_enrollments
                 WHERE id = ?1 OR label = ?1
                 ORDER BY CASE WHEN id = ?1 THEN 0 ELSE 1 END
                 LIMIT 1",
                rusqlite::params![id_or_label],
                map,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn delete_worker_enrollment(&self, id: &str) -> Result<bool> {
        let n = self.conn.execute(
            "DELETE FROM worker_enrollments WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(n > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_enrollment(estate_id: &str, id: &str, label: &str) -> WorkerEnrollmentRow {
        WorkerEnrollmentRow {
            id: id.into(),
            estate_id: estate_id.into(),
            label: label.into(),
            host: "worker.local".into(),
            ip: "10.0.0.2".into(),
            worker_pubkey: vec![1, 2, 3],
            audit_pubkey: vec![4, 5, 6],
            fabric_port: 9443,
            coordinator_pubkeys_json: "[]".into(),
            enrolled_at: now(),
            last_seen: None,
            fabric_tls_cert: None,
            worker_trust: None,
        }
    }

    #[test]
    fn worker_trust_persisted_and_audited() {
        let store = Store::open(":memory:").unwrap();
        let owner = store.ensure_owner_identity(&[9, 9, 9], "home").unwrap();
        let row = sample_enrollment(&owner.id, "worker_a", "gpu-box");
        store.upsert_worker_enrollment(&row).unwrap();
        assert!(store
            .set_worker_trust("worker_a", "external_untrusted", 3, "rpc")
            .unwrap());
        assert_eq!(
            store.worker_trust("worker_a").unwrap().as_deref(),
            Some("external_untrusted")
        );
        let audit = store.list_worker_trust_audit("worker_a").unwrap();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].trust, "external_untrusted");
        assert_eq!(audit[0].policy_epoch, 3);
    }

    #[test]
    fn max_worker_trust_policy_epoch_tracks_audit() {
        let store = Store::open(":memory:").unwrap();
        let owner = store.ensure_owner_identity(&[9, 9, 9], "home").unwrap();
        let row = sample_enrollment(&owner.id, "worker_a", "gpu-box");
        store.upsert_worker_enrollment(&row).unwrap();
        assert_eq!(store.max_worker_trust_policy_epoch().unwrap(), 0);
        store
            .set_worker_trust("worker_a", "external_untrusted", 7, "cli")
            .unwrap();
        assert_eq!(store.max_worker_trust_policy_epoch().unwrap(), 7);
    }

    #[test]
    fn enrollment_crud_roundtrip() {
        let store = Store::open(":memory:").unwrap();
        let owner = store.ensure_owner_identity(&[9, 9, 9], "home").unwrap();
        let row = sample_enrollment(&owner.id, "worker_a", "gpu-box");
        store.upsert_worker_enrollment(&row).unwrap();

        let by_id = store.find_worker_enrollment("worker_a").unwrap().unwrap();
        assert_eq!(by_id.label, "gpu-box");

        let by_label = store.find_worker_enrollment("gpu-box").unwrap().unwrap();
        assert_eq!(by_label.id, "worker_a");

        assert_eq!(store.list_worker_enrollments().unwrap().len(), 1);
        assert!(store.delete_worker_enrollment("worker_a").unwrap());
        assert!(store.find_worker_enrollment("worker_a").unwrap().is_none());
    }
}
