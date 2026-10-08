//! Scoped presentation metadata. Execution state remains in the work/run journals.
use crate::{LocalWorkData, Result, Store, StoreError};
use rusqlite::{params, OptionalExtension};

#[derive(Default)]
pub struct WorkMetadataPatch {
    pub notes: Option<Vec<String>>,
    pub status: Option<String>,
    pub lead_id: Option<String>,
    pub agent_ids: Option<Vec<String>>,
}

impl Store {
    pub(crate) fn migrate_work_metadata_v70(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS work_metadata (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, work_id TEXT NOT NULL,
            payload TEXT NOT NULL,
            PRIMARY KEY(org_id,team_id,work_id),
            FOREIGN KEY(org_id,team_id,work_id) REFERENCES team_work_items(org_id,team_id,work_id)
        );",
        )?;
        let legacy: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='local_work_notes')",
            [], |row| row.get(0),
        )?;
        if legacy {
            // Only migrate an unambiguous binding. Orphans and duplicated IDs
            // remain in the legacy table; never guess which team owns their data.
            let mut query = self.conn.prepare(
                "SELECT w.org_id,w.team_id,w.work_id
                FROM team_work_items w JOIN local_work_notes n ON n.work_id=w.work_id
                GROUP BY w.work_id HAVING COUNT(*)=1",
            )?;
            let rows = query.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                let (org, team, work) = row?;
                let data = self.get_local_work_data(&work)?;
                self.write_work_metadata(&org, &team, &work, &data)?;
            }
        }
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(70,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub fn work_metadata(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
    ) -> Result<LocalWorkData> {
        self.require_team_participant(actor, org, team)?;
        self.get_team_work_item(org, team, work)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let payload: Option<String> = self
            .conn
            .query_row(
                "SELECT payload FROM work_metadata WHERE org_id=?1 AND team_id=?2 AND work_id=?3",
                params![org, team, work],
                |row| row.get(0),
            )
            .optional()?;
        payload
            .map(|p| {
                serde_json::from_str(&p)
                    .map_err(|_| StoreError::InvalidControlResource("invalid work metadata".into()))
            })
            .transpose()
            .map(Option::unwrap_or_default)
    }

    pub fn update_work_metadata(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
        update: WorkMetadataPatch,
    ) -> Result<()> {
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let old = self.work_metadata(actor, org, team, work)?;
        if !self.control_access(actor, crate::ControlPermission::ManageTeam, org, team)? {
            return Err(StoreError::ControlAccessDenied);
        }
        self.write_work_metadata(
            org,
            team,
            work,
            &LocalWorkData {
                notes: update.notes.unwrap_or(old.notes),
                status: update.status.or(old.status),
                lead_id: update.lead_id.or(old.lead_id),
                agent_ids: update.agent_ids.or(old.agent_ids),
            },
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn write_work_metadata(
        &self,
        org: &str,
        team: &str,
        work: &str,
        data: &LocalWorkData,
    ) -> Result<()> {
        let payload = serde_json::to_string(data)
            .map_err(|_| StoreError::InvalidControlResource("invalid work metadata".into()))?;
        self.conn.execute(
            "INSERT INTO work_metadata(org_id,team_id,work_id,payload) VALUES(?1,?2,?3,?4)
            ON CONFLICT(org_id,team_id,work_id) DO UPDATE SET payload=excluded.payload",
            params![org, team, work, payload],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(path: &std::path::Path) -> Store {
        let db = Store::open(path).unwrap();
        db.bootstrap_control("owner", "org", "Organization")
            .unwrap();
        for team in ["first", "second"] {
            db.create_team(&crate::TeamRow {
                org_id: "org".into(),
                team_id: team.into(),
                name: team.into(),
                owner_principal_id: "owner".into(),
            })
            .unwrap();
        }
        for (team, work) in [
            ("first", "unique"),
            ("first", "duplicate"),
            ("second", "duplicate"),
        ] {
            db.create_team_work_item(crate::CreateTeamWorkItem {
                actor: "owner",
                org: "org",
                team,
                work_id: work,
                title: work,
                request_id: work,
                goal_id: None,
            })
            .unwrap();
        }
        db
    }

    #[test]
    fn upgrade_preserves_legacy_notes_without_guessing_ambiguous_ownership() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("metadata.db");
        {
            let db = seed(&path);
            // Reproduce the oldest two-column notes format before upgrading.
            db.conn.execute_batch("CREATE TABLE local_work_notes(work_id TEXT PRIMARY KEY, notes_json TEXT NOT NULL);
                INSERT INTO local_work_notes VALUES('unique','[\"legacy\"]'),('duplicate','[\"ambiguous\"]'),('orphan','[\"retained\"]');
                DROP TABLE work_metadata;
                DELETE FROM schema_versions WHERE version=70;").unwrap();
        }
        {
            let db = Store::open(&path).unwrap();
            assert_eq!(
                db.work_metadata("owner", "org", "first", "unique")
                    .unwrap()
                    .notes,
                ["legacy"]
            );
            for team in ["first", "second"] {
                assert!(db
                    .work_metadata("owner", "org", team, "duplicate")
                    .unwrap()
                    .notes
                    .is_empty());
            }
            assert_eq!(db.get_local_work_notes("duplicate").unwrap(), ["ambiguous"]);
            assert_eq!(db.get_local_work_notes("orphan").unwrap(), ["retained"]);
            db.update_work_metadata(
                "owner",
                "org",
                "first",
                "unique",
                WorkMetadataPatch {
                    notes: Some(vec!["new".into()]),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let db = Store::open(path).unwrap();
        assert_eq!(
            db.work_metadata("owner", "org", "first", "unique")
                .unwrap()
                .notes,
            ["new"]
        );
        assert_eq!(crate::backup::schema_version(&db.conn).unwrap(), 70);
    }

    #[test]
    fn upgrade_preserves_full_legacy_work_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("metadata.db");
        {
            let db = seed(&path);
            db.set_local_work_data(
                "unique",
                Some(&["saved notes".into()]),
                Some("review"),
                Some("lead"),
                Some(&["lead".into(), "reviewer".into()]),
            )
            .unwrap();
            db.conn
                .execute_batch(
                    "DROP TABLE work_metadata; DELETE FROM schema_versions WHERE version=70;",
                )
                .unwrap();
        }
        let db = Store::open(path).unwrap();
        let data = db.work_metadata("owner", "org", "first", "unique").unwrap();
        assert_eq!(data.notes, ["saved notes"]);
        assert_eq!(data.status.as_deref(), Some("review"));
        assert_eq!(data.lead_id.as_deref(), Some("lead"));
        assert_eq!(data.agent_ids.unwrap(), ["lead", "reviewer"]);
    }

    #[test]
    fn metadata_updates_preserve_other_fields_and_team_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let db = seed(&dir.path().join("metadata.db"));
        db.update_work_metadata(
            "owner",
            "org",
            "first",
            "duplicate",
            WorkMetadataPatch {
                notes: Some(vec!["first team".into()]),
                status: Some("review".into()),
                lead_id: Some("agent".into()),
                agent_ids: Some(vec!["agent".into()]),
            },
        )
        .unwrap();
        db.update_work_metadata(
            "owner",
            "org",
            "first",
            "duplicate",
            WorkMetadataPatch {
                notes: Some(vec![]),
                ..Default::default()
            },
        )
        .unwrap();
        let data = db
            .work_metadata("owner", "org", "first", "duplicate")
            .unwrap();
        assert!(data.notes.is_empty());
        assert_eq!(data.status.as_deref(), Some("review"));
        assert_eq!(data.lead_id.as_deref(), Some("agent"));
        assert_eq!(data.agent_ids.unwrap(), ["agent"]);
        assert!(db
            .work_metadata("owner", "org", "second", "duplicate")
            .unwrap()
            .status
            .is_none());
        assert!(db
            .work_metadata("owner", "org", "second", "unique")
            .is_err());
        assert!(db
            .update_work_metadata(
                "unknown",
                "org",
                "first",
                "unique",
                WorkMetadataPatch::default()
            )
            .is_err());
    }
}
