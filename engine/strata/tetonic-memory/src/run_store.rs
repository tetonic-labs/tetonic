//! Run supervisor persistence — only `lokai-run` may call these APIs (M3-1).

use rusqlite::{params, OptionalExtension};
use tetonic_domain::ids::{CommandId, EventId, RunId, TraceId};
use tetonic_domain::{ContentDigest, RunCommandResult, RunEventEnvelope, RunSnapshot};

use crate::util::now;
use crate::{Result, Store, StoreError};

/// Result of compacting run events at a replay floor (R25).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactReport {
    pub deleted: usize,
    pub freed_payload_bytes: u64,
    pub replay_floor: u64,
}

impl Store {
    /// Atomically append an authoritative run event and persist the projection.
    pub fn commit_run_command(
        &self,
        snapshot: &RunSnapshot,
        event: &RunEventEnvelope,
        idempotency: Option<(&str, &RunCommandResult)>,
    ) -> Result<u64> {
        // Acquire the writer before reading projection metadata. A deferred WAL
        // transaction can otherwise fail its read-to-write upgrade immediately
        // (SQLITE_BUSY_SNAPSHOT) instead of honoring the configured busy timeout.
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let new_seq = snapshot.sequence;
        let existing_floor: i64 = tx
            .query_row(
                "SELECT replay_floor FROM run_projections WHERE run_id = ?1",
                params![snapshot.run_id.0],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let existing_recovery: Option<String> = tx
            .query_row(
                "SELECT recovery_snapshot_json FROM run_projections WHERE run_id = ?1",
                params![snapshot.run_id.0],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        tx.execute(
            "INSERT OR REPLACE INTO run_projections
             (run_id, session_id, sequence, state, projection_json, updated_at, replay_floor, recovery_snapshot_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                snapshot.run_id.0,
                snapshot.session_id.as_ref().map(|s| s.0.as_str()),
                new_seq as i64,
                format!("{:?}", snapshot.state).to_lowercase(),
                {
                    let mut db_snap = snapshot.clone();
                    db_snap.events.clear();
                    serde_json::to_string(&db_snap).map_err(|e| {
                        crate::StoreError::InvalidDataClass(format!("snapshot json: {e}"))
                    })?
                },
                now(),
                existing_floor,
                existing_recovery,
            ],
        )?;
        // Verify the digest against the exact bytes about to be stored (M6,
        // INV-RUN-003). Re-hashing on read cannot catch a digest that never
        // described its payload, because such a row is self-consistent from the
        // moment it is written; the only place to catch it is here.
        //
        // This replaces a guard that rejected an empty digest *string*. That
        // guard could not fire on the value it was aimed at: the pre-M6 fallback
        // hashed empty *bytes*, producing a well-formed sha256:e3b0c442… . It
        // tested for presence where the invariant asks for correspondence.
        let payload_json = serde_json::to_string(&event.payload)
            .map_err(|e| crate::StoreError::InvalidDataClass(format!("payload json: {e}")))?;
        let computed = crate::payload_digest::digest_payload_json(&payload_json);
        if computed.0 != event.payload_digest.0 {
            return Err(crate::StoreError::DigestMismatch(format!(
                "event {} sequence {}: supplied {} does not describe its payload (computed {})",
                event.event_id.0, event.sequence, event.payload_digest.0, computed.0
            )));
        }
        let event_type_json = serde_json::to_string(&event.event_type)
            .map_err(|e| crate::StoreError::InvalidDataClass(format!("event_type json: {e}")))?;
        let actor_json = serde_json::to_string(&event.actor)
            .map_err(|e| crate::StoreError::InvalidDataClass(format!("actor json: {e}")))?;
        let data_class_json = serde_json::to_string(&event.data_class)
            .map_err(|e| crate::StoreError::InvalidDataClass(format!("data_class json: {e}")))?;
        tx.execute(
            "INSERT INTO run_events (event_id, run_id, sequence, event_type, schema_version, command_id, causation_id, correlation_id, actor_json, occurred_at, recorded_at, data_class, payload_digest, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                event.event_id.0,
                event.run_id.0,
                event.sequence as i64,
                event_type_json,
                event.schema_version,
                event.command_id.as_ref().map(|id| id.0.clone()),
                event.causation_id.as_ref().map(|id| id.0.clone()),
                event.correlation_id.as_ref().map(|id| id.0.clone()),
                actor_json,
                event.occurred_at.to_rfc3339(),
                event.recorded_at.to_rfc3339(),
                data_class_json,
                event.payload_digest.0.clone(),
                payload_json,
            ],
        )?;
        if let Some((key, result)) = idempotency {
            tx.execute(
                "INSERT OR REPLACE INTO run_idempotency (run_id, idempotency_key, result_json)
                 VALUES (?1, ?2, ?3)",
                params![
                    snapshot.run_id.0,
                    key,
                    serde_json::to_string(result).map_err(|e| {
                        crate::StoreError::InvalidDataClass(format!("result json: {e}"))
                    })?,
                ],
            )?;
        }
        if let Some(command_id) = &event.command_id {
            let result = tetonic_domain::RunCommandResult {
                run_id: snapshot.run_id.clone(),
                sequence: snapshot.sequence,
                idempotent_replay: false,
                snapshot: snapshot.clone(),
            };
            tx.execute(
                "INSERT INTO run_command_dedup (run_id, command_id, result_json, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![snapshot.run_id.0, command_id.0,
                    serde_json::to_string(&result).map_err(|e| StoreError::InvalidDataClass(e.to_string()))?, now()],
            )?;
        }
        tx.commit()?;
        Ok(new_seq)
    }

    /// Persist the projection alone, appending no event (M6, INV-RUN-003).
    ///
    /// Restart recovery marks state that no command produced. It cannot use
    /// `commit_run_command`, because `run_events` is a **command** log: replay
    /// deserializes every payload into a `RunCommand` and refuses on a sequence
    /// gap, so a non-command event would make the run unreplayable. Recovery
    /// state belongs to the projection, which is what this writes.
    ///
    /// `replay_floor` and `recovery_snapshot_json` are carried forward, matching
    /// `commit_run_command`.
    pub fn persist_run_projection(&self, snapshot: &RunSnapshot) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let existing_floor: i64 = tx
            .query_row(
                "SELECT replay_floor FROM run_projections WHERE run_id = ?1",
                params![snapshot.run_id.0],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let existing_recovery: Option<String> = tx
            .query_row(
                "SELECT recovery_snapshot_json FROM run_projections WHERE run_id = ?1",
                params![snapshot.run_id.0],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let projection_json = {
            let mut db_snap = snapshot.clone();
            db_snap.events.clear();
            serde_json::to_string(&db_snap)
                .map_err(|e| crate::StoreError::InvalidDataClass(format!("snapshot json: {e}")))?
        };
        tx.execute(
            "INSERT OR REPLACE INTO run_projections
             (run_id, session_id, sequence, state, projection_json, updated_at, replay_floor, recovery_snapshot_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                snapshot.run_id.0,
                snapshot.session_id.as_ref().map(|s| s.0.as_str()),
                snapshot.sequence as i64,
                format!("{:?}", snapshot.state).to_lowercase(),
                projection_json,
                now(),
                existing_floor,
                existing_recovery,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// `run_projections.session_id` column. Missing row is an error; SQL NULL is `Ok(None)`.
    pub fn load_run_projection_session_id(&self, run_id: &str) -> Result<Option<String>> {
        let found: Option<Option<String>> = self
            .conn
            .query_row(
                "SELECT session_id FROM run_projections WHERE run_id = ?1",
                params![run_id],
                |r| r.get(0),
            )
            .optional()?;
        found.ok_or_else(|| {
            crate::StoreError::InvalidDataClass(format!("no run_projections row for {run_id}"))
        })
    }

    pub fn load_run_snapshot(&self, run_id: &str) -> Result<Option<RunSnapshot>> {
        let row: Option<String> = self
            .conn
            .query_row(
                "SELECT projection_json FROM run_projections WHERE run_id = ?1",
                params![run_id],
                |r| r.get(0),
            )
            .optional()?;
        row.map(|j| serde_json::from_str(&j))
            .transpose()
            .map_err(|e| crate::StoreError::InvalidDataClass(format!("snapshot decode: {e}")))
    }

    pub fn load_run_idempotency(
        &self,
        run_id: &str,
        key: &str,
    ) -> Result<Option<RunCommandResult>> {
        let row: Option<String> = self
            .conn
            .query_row(
                "SELECT result_json FROM run_idempotency WHERE run_id = ?1 AND idempotency_key = ?2",
                params![run_id, key],
                |r| r.get(0),
            )
            .optional()?;
        row.map(|j| serde_json::from_str(&j))
            .transpose()
            .map_err(|e| crate::StoreError::InvalidDataClass(format!("idempotency decode: {e}")))
    }

    pub fn list_run_events(&self, run_id: &str) -> Result<Vec<RunEventEnvelope>> {
        let mut stmt = self.conn.prepare(
            "SELECT event_id, sequence, event_type, schema_version, command_id, causation_id, correlation_id, actor_json, occurred_at, recorded_at, data_class, payload_digest, payload_json FROM run_events
             WHERE run_id = ?1 ORDER BY sequence ASC",
        )?;
        // Read raw columns inside the mapper, which can only yield rusqlite
        // errors, then decode and verify outside it (M6). Before M6 the decode
        // happened inside the closure behind seven `.unwrap()`s, so a damaged
        // journal panicked instead of refusing.
        type RawEventRow = (
            String,
            i64,
            String,
            u32,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            String,
            String,
            String,
            String,
            String,
        );
        let raw: Vec<RawEventRow> = stmt
            .query_map(params![run_id], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                    r.get(11)?,
                    r.get(12)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut events = Vec::with_capacity(raw.len());
        for (
            event_id,
            sequence,
            event_type,
            schema_version,
            command_id,
            causation_id,
            correlation_id,
            actor_json,
            occurred_at,
            recorded_at,
            data_class,
            payload_digest,
            payload_json,
        ) in raw
        {
            let bad = |what: &str, detail: String| {
                crate::StoreError::InvalidDataClass(format!(
                    "run event {event_id} sequence {sequence}: {what}: {detail}"
                ))
            };

            // Verify before decoding: the digest describes the stored bytes, so
            // a mismatch is answered without trusting their contents.
            let computed = crate::payload_digest::digest_payload_json(&payload_json);
            if computed.0 != payload_digest {
                return Err(crate::StoreError::DigestMismatch(format!(
                    "run event {event_id} sequence {sequence}: stored {payload_digest} does not describe its payload (computed {})",
                    computed.0
                )));
            }

            events.push(RunEventEnvelope {
                event_id: EventId::new(event_id.clone()),
                run_id: RunId::new(run_id.to_string()),
                sequence: sequence as u64,
                event_type: serde_json::from_str(&event_type)
                    .map_err(|e| bad("event_type", e.to_string()))?,
                schema_version,
                command_id: command_id.map(CommandId::new),
                causation_id: causation_id.map(EventId::new),
                correlation_id: correlation_id.map(TraceId::new),
                actor: serde_json::from_str(&actor_json)
                    .map_err(|e| bad("actor", e.to_string()))?,
                occurred_at: chrono::DateTime::parse_from_rfc3339(&occurred_at)
                    .map_err(|e| bad("occurred_at", e.to_string()))?
                    .with_timezone(&chrono::Utc),
                recorded_at: chrono::DateTime::parse_from_rfc3339(&recorded_at)
                    .map_err(|e| bad("recorded_at", e.to_string()))?
                    .with_timezone(&chrono::Utc),
                data_class: serde_json::from_str(&data_class)
                    .map_err(|e| bad("data_class", e.to_string()))?,
                payload_digest: ContentDigest::new(payload_digest),
                payload: serde_json::from_str(&payload_json)
                    .map_err(|e| bad("payload", e.to_string()))?,
            });
        }
        Ok(events)
    }

    pub fn compact_run_events(&self, run_id: &str, up_to_sequence: u64) -> Result<usize> {
        // Prefer compact_run_events_at_floor from lokai-run (R25). This keeps
        // a fail-closed delete for tests that only need retention pruning.
        let tip = self.load_run_snapshot(run_id)?;
        let recovery = tip.unwrap_or_else(|| RunSnapshot {
            run_id: RunId::new(run_id),
            session_id: None,
            state: tetonic_domain::RunState::Created,
            sequence: up_to_sequence.saturating_sub(1),
            workspace_version: None,
            tasks: Default::default(),
            attempts: Default::default(),
            dependencies: Default::default(),
            events: Vec::new(),
            delivery_index: Default::default(),
            side_effect_commits: Default::default(),
            deadlines: Default::default(),
            cancellation: Default::default(),
            speculation: Default::default(),
            next_lease_epoch: 0,
            job_spec: None,
        });
        // Tip is head-state; only valid when used by tests that do not rebuild
        // from floor. Production always supplies a floor-rebuilt snapshot.
        self.compact_run_events_at_floor(run_id, up_to_sequence, &recovery)
            .map(|r| r.deleted)
    }

    /// Persist recovery snapshot at `new_floor`, then delete non-retained events
    /// with `sequence < new_floor`. Secret / sensitive_source rows are kept (R25).
    pub fn compact_run_events_at_floor(
        &self,
        run_id: &str,
        new_floor: u64,
        recovery_snapshot: &RunSnapshot,
    ) -> Result<CompactReport> {
        if new_floor == 0 {
            return Ok(CompactReport {
                deleted: 0,
                freed_payload_bytes: 0,
                replay_floor: 0,
            });
        }
        let tx = self.conn.unchecked_transaction()?;
        let mut recovery = recovery_snapshot.clone();
        recovery.events.clear();
        let recovery_json = serde_json::to_string(&recovery).map_err(|e| {
            crate::StoreError::InvalidDataClass(format!("recovery snapshot json: {e}"))
        })?;
        let updated = tx.execute(
            "UPDATE run_projections
             SET replay_floor = ?1, recovery_snapshot_json = ?2, updated_at = ?3
             WHERE run_id = ?4",
            params![new_floor as i64, recovery_json, now(), run_id],
        )?;
        if updated == 0 {
            return Err(crate::StoreError::InvalidDataClass(format!(
                "compact: missing run_projections for {run_id}"
            )));
        }

        let secret = serde_json::to_string(&tetonic_domain::DataClass::Secret).unwrap();
        let sensitive = serde_json::to_string(&tetonic_domain::DataClass::SensitiveSource).unwrap();

        let mut size_stmt = tx.prepare(
            "SELECT COALESCE(SUM(LENGTH(payload_json)), 0) FROM run_events
             WHERE run_id = ?1 AND sequence < ?2
               AND data_class NOT IN (?3, ?4)",
        )?;
        let freed: i64 = size_stmt
            .query_row(params![run_id, new_floor as i64, secret, sensitive], |r| {
                r.get(0)
            })?;
        drop(size_stmt);

        let deleted = tx.execute(
            "DELETE FROM run_events
             WHERE run_id = ?1 AND sequence < ?2
               AND data_class NOT IN (?3, ?4)",
            params![run_id, new_floor as i64, secret, sensitive],
        )?;
        tx.commit()?;
        Ok(CompactReport {
            deleted,
            freed_payload_bytes: freed.max(0) as u64,
            replay_floor: new_floor,
        })
    }

    /// Stored replay floor and recovery snapshot (R25).
    pub fn load_replay_floor(&self, run_id: &str) -> Result<Option<(u64, Option<RunSnapshot>)>> {
        let row: Option<(i64, Option<String>)> = self
            .conn
            .query_row(
                "SELECT replay_floor, recovery_snapshot_json FROM run_projections WHERE run_id = ?1",
                params![run_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match row {
            None => Ok(None),
            Some((floor, json)) => {
                let snap = match json {
                    Some(j) if !j.is_empty() => Some(serde_json::from_str(&j).map_err(|e| {
                        crate::StoreError::InvalidDataClass(format!("recovery decode: {e}"))
                    })?),
                    _ => None,
                };
                Ok(Some((floor as u64, snap)))
            }
        }
    }

    pub fn get_legacy_events(&self, session_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT payload_json FROM legacy_run_events WHERE run_id = ?1 ORDER BY sequence ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |r| r.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    pub fn list_all_run_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT run_id FROM run_projections")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    pub fn load_run_command_dedup(
        &self,
        run_id: &str,
        command_id: &str,
    ) -> Result<Option<RunCommandResult>> {
        let row: Option<String> = self
            .conn
            .query_row(
                "SELECT result_json FROM run_command_dedup WHERE run_id = ?1 AND command_id = ?2",
                params![run_id, command_id],
                |r| r.get(0),
            )
            .optional()?;
        row.map(|j| serde_json::from_str(&j))
            .transpose()
            .map_err(|e| StoreError::InvalidDataClass(format!("command dedup decode: {e}")))
    }

    pub fn store_run_command_dedup(
        &self,
        run_id: &str,
        command_id: &str,
        result: &RunCommandResult,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO run_command_dedup (run_id, command_id, result_json, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                run_id,
                command_id,
                serde_json::to_string(result).map_err(|e| {
                    StoreError::InvalidDataClass(format!("command dedup encode: {e}"))
                })?,
                now(),
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_domain::classify::DataClass;
    use tetonic_domain::ids::{EventId, TraceId};
    use tetonic_domain::{ContentDigest, EventActor, EventType, RunId, RunState, SessionId};

    /// Before M6 this fabricated `sha256:p{seq}`, a digest that described no
    /// payload, and the store accepted it. Every test built on this fixture
    /// therefore encoded the defect it was meant to exercise, so the fixture had
    /// to become honest before either verification side could land.
    fn sample_event(run_id: &str, seq: u64, class: DataClass) -> RunEventEnvelope {
        let payload = serde_json::json!({"n": seq});
        RunEventEnvelope {
            event_id: EventId::new(format!("ev_{seq}")),
            run_id: RunId::new(run_id),
            sequence: seq,
            event_type: EventType::Other("test".into()),
            schema_version: 1,
            command_id: None,
            causation_id: None,
            correlation_id: Some(TraceId::new("t")),
            actor: EventActor {
                name: "test".into(),
            },
            occurred_at: chrono::Utc::now(),
            recorded_at: chrono::Utc::now(),
            data_class: class,
            payload_digest: crate::payload_digest::digest_event_payload(&payload).unwrap(),
            payload,
        }
    }

    fn active_snapshot(run: &str) -> RunSnapshot {
        RunSnapshot {
            run_id: RunId::new(run),
            session_id: Some(SessionId::new("s")),
            state: RunState::Active,
            sequence: 1,
            workspace_version: None,
            tasks: Default::default(),
            attempts: Default::default(),
            dependencies: Default::default(),
            events: Vec::new(),
            delivery_index: Default::default(),
            side_effect_commits: Default::default(),
            deadlines: Default::default(),
            cancellation: Default::default(),
            speculation: Default::default(),
            next_lease_epoch: 0,
            job_spec: None,
        }
    }

    #[test]
    fn command_receipt_and_event_are_one_transaction() {
        let store = Store::open(":memory:").unwrap();
        let mut snap = active_snapshot("atomic_receipt");
        let mut ev = sample_event("atomic_receipt", 1, DataClass::RepositorySource);
        ev.command_id = Some(CommandId::new("command_1"));
        store.commit_run_command(&snap, &ev, None).unwrap();
        let receipt = store
            .load_run_command_dedup("atomic_receipt", "command_1")
            .unwrap()
            .unwrap();
        assert_eq!(receipt.sequence, 1);
        // Duplicate receipt must roll back the attempted new event/projection too.
        snap.sequence = 2;
        ev.sequence = 2;
        ev.event_id = EventId::new("second_event");
        assert!(store.commit_run_command(&snap, &ev, None).is_err());
        assert_eq!(
            store
                .load_run_snapshot("atomic_receipt")
                .unwrap()
                .unwrap()
                .sequence,
            1
        );
        assert_eq!(store.list_run_events("atomic_receipt").unwrap().len(), 1);
    }

    /// VM-4 — a digest that does not describe its payload must be refused at the
    /// write boundary. Re-hashing on read cannot catch this: the row would be
    /// self-consistent from the moment it was stored.
    #[test]
    fn commit_refuses_a_digest_that_does_not_match_its_payload() {
        let store = Store::open(":memory:").unwrap();
        let run = "run_bad_digest";
        let snap = active_snapshot(run);
        let mut ev = sample_event(run, 1, DataClass::RepositorySource);
        ev.payload_digest = ContentDigest::new("sha256:p1".to_string());

        let err = store
            .commit_run_command(&snap, &ev, None)
            .expect_err("a fabricated digest must not be accepted");
        assert!(
            matches!(err, crate::StoreError::DigestMismatch(_)),
            "expected DigestMismatch, got {err:?}"
        );
    }

    /// VM-4 — the pre-M6 guard checked `is_empty()`, which the old fallback
    /// value passes: hashing empty *bytes* yields a well-formed
    /// `sha256:e3b0c442…`. The replacement check must reject it.
    #[test]
    fn commit_refuses_the_digest_of_empty_bytes() {
        let store = Store::open(":memory:").unwrap();
        let run = "run_empty_hash";
        let snap = active_snapshot(run);
        let mut ev = sample_event(run, 1, DataClass::RepositorySource);
        ev.payload_digest = ContentDigest::new(
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
        );

        assert!(matches!(
            store.commit_run_command(&snap, &ev, None),
            Err(crate::StoreError::DigestMismatch(_))
        ));
    }

    /// VM-5 — a payload corrupted after it was stored must be refused on read,
    /// not handed back with the digest it no longer matches.
    #[test]
    fn list_run_events_refuses_a_corrupted_payload() {
        let store = Store::open(":memory:").unwrap();
        let run = "run_corrupt";
        let snap = active_snapshot(run);
        let ev = sample_event(run, 1, DataClass::RepositorySource);
        store.commit_run_command(&snap, &ev, None).unwrap();
        assert!(store.list_run_events(run).is_ok());

        store
            .conn
            .execute(
                "UPDATE run_events SET payload_json = ?1 WHERE run_id = ?2",
                params!["{\"n\":999}", run],
            )
            .unwrap();

        let err = store
            .list_run_events(run)
            .expect_err("a payload that no longer matches its digest must not be returned");
        assert!(
            matches!(err, crate::StoreError::DigestMismatch(_)),
            "expected DigestMismatch, got {err:?}"
        );
    }

    /// VM-6 — a structurally damaged row must refuse rather than panic inside
    /// the rusqlite row mapper.
    #[test]
    fn list_run_events_refuses_a_malformed_row_without_panicking() {
        let store = Store::open(":memory:").unwrap();
        let run = "run_malformed";
        let snap = active_snapshot(run);
        let ev = sample_event(run, 1, DataClass::RepositorySource);
        store.commit_run_command(&snap, &ev, None).unwrap();

        store
            .conn
            .execute(
                "UPDATE run_events SET actor_json = ?1 WHERE run_id = ?2",
                params!["not json", run],
            )
            .unwrap();

        assert!(
            store.list_run_events(run).is_err(),
            "a malformed row must be an error, not a panic or a silent skip"
        );
    }

    #[test]
    fn compact_at_floor_retains_secret_rows() {
        let store = Store::open(":memory:").unwrap();
        let run = "run_secret";
        let mut tip = RunSnapshot {
            run_id: RunId::new(run),
            session_id: Some(SessionId::new("s")),
            state: RunState::Active,
            sequence: 3,
            workspace_version: None,
            tasks: Default::default(),
            attempts: Default::default(),
            dependencies: Default::default(),
            events: Vec::new(),
            delivery_index: Default::default(),
            side_effect_commits: Default::default(),
            deadlines: Default::default(),
            cancellation: Default::default(),
            speculation: Default::default(),
            next_lease_epoch: 0,
            job_spec: None,
        };
        for (seq, class) in [
            (1u64, DataClass::Secret),
            (2, DataClass::RepositorySource),
            (3, DataClass::RepositorySource),
        ] {
            tip.sequence = seq;
            let ev = sample_event(run, seq, class);
            store.commit_run_command(&tip, &ev, None).unwrap();
        }
        let floor_snap = {
            let mut s = tip.clone();
            s.sequence = 1;
            s
        };
        let report = store
            .compact_run_events_at_floor(run, 3, &floor_snap)
            .unwrap();
        assert_eq!(report.replay_floor, 3);
        assert_eq!(report.deleted, 1); // only seq 2
        let events = store.list_run_events(run).unwrap();
        assert!(events.iter().any(|e| e.sequence == 1));
        assert!(!events.iter().any(|e| e.sequence == 2));
        assert!(events.iter().any(|e| e.sequence == 3));
        let (floor, snap) = store.load_replay_floor(run).unwrap().unwrap();
        assert_eq!(floor, 3);
        assert!(snap.is_some());
    }

    #[test]
    fn none_session_id_persists_as_sql_null() {
        let store = Store::open(":memory:").unwrap();
        let run = "run_no_session";
        let mut snap = active_snapshot(run);
        snap.session_id = None;
        let ev = sample_event(run, 1, DataClass::RepositorySource);
        store.commit_run_command(&snap, &ev, None).unwrap();
        let col = store.load_run_projection_session_id(run).unwrap();
        assert!(col.is_none());
    }
}
