//! Append-only working briefs on existing team work. Saving never admits work.
use crate::{Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct WorkBrief {
    pub work_id: String,
    pub revision: i64,
    pub body: String,
    pub request_id: String,
    pub created_by: String,
}

impl Store {
    pub(crate) fn migrate_work_shaping_v53(&self) -> Result<()> {
        let applied: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version=53)",
            [],
            |r| r.get(0),
        )?;
        if applied {
            return Ok(());
        }
        let has_purpose: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('team_work_items') WHERE name='purpose')",
            [],
            |r| r.get(0),
        )?;
        if !has_purpose {
            self.conn.execute_batch(
                "ALTER TABLE team_work_items ADD COLUMN purpose TEXT NOT NULL DEFAULT 'work'
                 CHECK(purpose IN ('work','explore'));",
            )?;
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS work_brief_revisions (
                org_id TEXT NOT NULL, team_id TEXT NOT NULL, work_id TEXT NOT NULL,
                revision INTEGER NOT NULL, body TEXT NOT NULL, request_id TEXT NOT NULL,
                expected_revision INTEGER NOT NULL, created_by TEXT NOT NULL,
                PRIMARY KEY(org_id,team_id,work_id,revision),
                UNIQUE(org_id,team_id,work_id,request_id),
                FOREIGN KEY(org_id,team_id,work_id) REFERENCES team_work_items(org_id,team_id,work_id)
             );")?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(53,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub fn work_briefs(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
    ) -> Result<Vec<WorkBrief>> {
        self.require_team_participant(actor, org, team)?;
        if self.get_team_work_item(org, team, work)?.is_none() {
            return Err(StoreError::ControlAccessDenied);
        }
        let mut stmt = self.conn.prepare(
            "SELECT work_id,revision,body,request_id,created_by FROM work_brief_revisions
            WHERE org_id=?1 AND team_id=?2 AND work_id=?3 ORDER BY revision DESC LIMIT 50",
        )?;
        let rows = stmt
            .query_map(params![org, team, work], |r| {
                Ok(WorkBrief {
                    work_id: r.get(0)?,
                    revision: r.get(1)?,
                    body: r.get(2)?,
                    request_id: r.get(3)?,
                    created_by: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn save_work_brief(&self, command: crate::SaveWorkBrief<'_>) -> Result<WorkBrief> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let row = self.save_work_brief_in_transaction(command)?;
        tx.commit()?;
        Ok(row)
    }

    pub(crate) fn save_work_brief_in_transaction(
        &self,
        command: crate::SaveWorkBrief<'_>,
    ) -> Result<WorkBrief> {
        debug_assert!(!self.conn.is_autocommit());
        let crate::SaveWorkBrief {
            actor,
            org,
            team,
            work,
            request,
            expected,
            body,
        } = command;
        if request.is_empty()
            || request.len() > 128
            || request.contains('\0')
            || expected < 0
            || body.trim().is_empty()
            || body.len() > 12_000
            || body.contains('\0')
        {
            return Err(StoreError::InvalidControlResource("brief".into()));
        }
        self.require_team_participant(actor, org, team)?;
        let work_item = self
            .get_team_work_item(org, team, work)?
            .ok_or(StoreError::ControlAccessDenied)?;
        // For now these briefs describe exploration, not accepted execution plans.
        if work_item.purpose != crate::WorkPurpose::Explore {
            return Err(StoreError::ControlResourceConflict);
        }
        let retry: Option<(i64,String,i64,String)> = self.conn.query_row(
            "SELECT revision,body,expected_revision,created_by FROM work_brief_revisions WHERE org_id=?1 AND team_id=?2 AND work_id=?3 AND request_id=?4",
            params![org,team,work,request], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        if let Some((revision, old_body, old_expected, created_by)) = retry {
            if old_body != body || old_expected != expected || created_by != actor {
                return Err(StoreError::ControlResourceConflict);
            }
            return Ok(WorkBrief {
                work_id: work.into(),
                revision,
                body: old_body,
                request_id: request.into(),
                created_by,
            });
        }
        let latest:i64 = self.conn.query_row("SELECT COALESCE(MAX(revision),0) FROM work_brief_revisions WHERE org_id=?1 AND team_id=?2 AND work_id=?3",params![org,team,work],|r|r.get(0))?;
        if latest != expected {
            return Err(StoreError::ControlResourceConflict);
        }
        let revision = latest
            .checked_add(1)
            .ok_or(StoreError::ControlResourceConflict)?;
        self.conn.execute("INSERT INTO work_brief_revisions(org_id,team_id,work_id,revision,body,request_id,expected_revision,created_by) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![org,team,work,revision,body,request,expected,actor])?;
        Ok(WorkBrief {
            work_id: work.into(),
            revision,
            body: body.into(),
            request_id: request.into(),
            created_by: actor.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn seed(path: &str) -> Store {
        let db = Store::open(path).unwrap();
        db.bootstrap_control("owner", "org", "Org").unwrap();
        db.create_team(&crate::TeamRow {
            org_id: "org".into(),
            team_id: "team".into(),
            name: "Team".into(),
            owner_principal_id: "owner".into(),
        })
        .unwrap();
        db.create_team_work_item_for_purpose(
            crate::CreateTeamWorkItem {
                actor: "owner",
                org: "org",
                team: "team",
                work_id: "exploration",
                title: "Understand the problem",
                request_id: "request",
                goal_id: None,
            },
            Some("Original words"),
            crate::WorkPurpose::Explore,
        )
        .unwrap();
        db
    }
    #[test]
    fn brief_revisions_are_scoped_idempotent_and_never_launch_work() {
        let db = seed(":memory:");
        let first = db
            .save_work_brief(crate::SaveWorkBrief {
                actor: "owner",
                org: "org",
                team: "team",
                work: "exploration",
                request: "save-1",
                expected: 0,
                body: "We need evidence",
            })
            .unwrap();
        assert_eq!(first.revision, 1);
        assert_eq!(
            db.save_work_brief(crate::SaveWorkBrief {
                actor: "owner",
                org: "org",
                team: "team",
                work: "exploration",
                request: "save-1",
                expected: 0,
                body: "We need evidence"
            })
            .unwrap(),
            first
        );
        assert!(db
            .save_work_brief(crate::SaveWorkBrief {
                actor: "owner",
                org: "org",
                team: "team",
                work: "exploration",
                request: "save-1",
                expected: 0,
                body: "Changed retry"
            })
            .is_err());
        assert!(db
            .save_work_brief(crate::SaveWorkBrief {
                actor: "owner",
                org: "org",
                team: "team",
                work: "exploration",
                request: "save-2",
                expected: 0,
                body: "Stale edit"
            })
            .is_err());
        let second = db
            .save_work_brief(crate::SaveWorkBrief {
                actor: "owner",
                org: "org",
                team: "team",
                work: "exploration",
                request: "save-2",
                expected: 1,
                body: "Compare two approaches",
            })
            .unwrap();
        assert_eq!(second.revision, 2);
        assert_eq!(
            db.save_work_brief(crate::SaveWorkBrief {
                actor: "owner",
                org: "org",
                team: "team",
                work: "exploration",
                request: "save-1",
                expected: 0,
                body: "We need evidence"
            })
            .unwrap(),
            first
        );
        assert!(db
            .work_briefs("stranger", "org", "team", "exploration")
            .is_err());
        assert!(db
            .work_briefs("owner", "other-org", "team", "exploration")
            .is_err());
        assert!(db
            .save_work_brief(crate::SaveWorkBrief {
                actor: "stranger",
                org: "org",
                team: "team",
                work: "exploration",
                request: "forged",
                expected: 2,
                body: "Secret"
            })
            .is_err());
        let work = db
            .get_team_work_item("org", "team", "exploration")
            .unwrap()
            .unwrap();
        assert_eq!(work.input.as_deref(), Some("Original words"));
        assert_eq!(work.run_id, None);
        assert_eq!(work.status, "open");
        assert!(db
            .create_team_work_item_with_input(
                crate::CreateTeamWorkItem {
                    actor: "owner",
                    org: "org",
                    team: "team",
                    work_id: "exploration",
                    title: "Understand the problem",
                    request_id: "request",
                    goal_id: None
                },
                Some("Original words")
            )
            .is_err());
        assert_eq!(
            db.work_briefs("owner", "org", "team", "exploration")
                .unwrap(),
            vec![second, first]
        );
    }
    #[test]
    fn shaping_migration_upgrades_legacy_work_and_briefs_survive_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("work.db");
        let db = seed(path.to_str().unwrap());
        db.remove_agent_edits_schema_for_test();
        db.conn.execute_batch("DROP TABLE work_brief_revisions; ALTER TABLE team_work_items DROP COLUMN purpose; DELETE FROM schema_versions WHERE version>=53;").unwrap();
        drop(db);
        let db = Store::open(&path).unwrap();
        assert_eq!(
            db.get_team_work_item("org", "team", "exploration")
                .unwrap()
                .unwrap()
                .purpose,
            crate::WorkPurpose::Work
        );
        db.create_team_work_item_for_purpose(
            crate::CreateTeamWorkItem {
                actor: "owner",
                org: "org",
                team: "team",
                work_id: "new",
                title: "New exploration",
                request_id: "new-request",
                goal_id: None,
            },
            Some("Question"),
            crate::WorkPurpose::Explore,
        )
        .unwrap();
        let saved = db
            .save_work_brief(crate::SaveWorkBrief {
                actor: "owner",
                org: "org",
                team: "team",
                work: "new",
                request: "save",
                expected: 0,
                body: "Retained decision",
            })
            .unwrap();
        drop(db);
        let reopened = Store::open(&path).unwrap();
        assert_eq!(
            reopened.work_briefs("owner", "org", "team", "new").unwrap(),
            vec![saved]
        );
        reopened.migrate_work_shaping_v53().unwrap();
    }
}
