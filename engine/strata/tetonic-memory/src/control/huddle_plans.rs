//! Structured revisions of existing huddles. Agreement records direction only;
//! neither proposing nor agreeing admits execution or expands authority.
use crate::{Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanAssignment {
    pub key: String,
    pub title: String,
    pub instructions: String,
    pub agent_key: String,
    pub depends_on: Vec<String>,
    pub tools: Vec<String>,
    pub deliverable: String,
    pub token_budget: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanContent {
    pub title: String,
    pub summary: String,
    pub assignments: Vec<PlanAssignment>,
    pub open_questions: Vec<String>,
    pub token_budget: u64,
}

impl PlanContent {
    pub fn validate(&self) -> Result<()> {
        field_text("title", &self.title, 160)?;
        field_text("summary", &self.summary, 2000)?;
        if self.assignments.is_empty() || self.assignments.len() > 12 {
            return invalid_field("assignments", "provide 1–12 assignments");
        }
        if self.open_questions.len() > 12 {
            return invalid_field("open_questions", "provide at most 12 questions");
        }
        for (index, question) in self.open_questions.iter().enumerate() {
            field_text(&format!("open_questions[{index}]"), question, 1000)?;
        }
        let mut keys = HashSet::new();
        for (index, assignment) in self.assignments.iter().enumerate() {
            let field = |name: &str| format!("assignments[{index}].{name}");
            field_text(&field("key"), &assignment.key, 48)?;
            if !assignment
                .key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return invalid_field(&field("key"), "use only ASCII letters, digits, hyphens or underscores; no spaces. This is an assignment ID, not the agent's name");
            }
            if !keys.insert(assignment.key.as_str()) {
                return invalid_field(&field("key"), "each assignment needs a unique key");
            }
            field_text(&field("title"), &assignment.title, 160)?;
            field_text(&field("instructions"), &assignment.instructions, 4000)?;
            field_text(&field("agent_key"), &assignment.agent_key, 128)?;
            field_text(&field("deliverable"), &assignment.deliverable, 1000)?;
            if assignment.token_budget == 0 || assignment.token_budget > 1_000_000 {
                return invalid_field(
                    &field("token_budget"),
                    "use an integer between 1 and 1000000, within the saved agent's allowance",
                );
            }
            if assignment.tools.len() > 16 {
                return invalid_field(&field("tools"), "use at most 16 granted tools");
            }
            if assignment.depends_on.len() > 12 {
                return invalid_field(&field("depends_on"), "use at most 12 assignment keys");
            }
            for tool in &assignment.tools {
                field_text(&field("tools"), tool, 128)?;
            }
            if assignment.tools.iter().collect::<HashSet<_>>().len() != assignment.tools.len() {
                return invalid_field(&field("tools"), "remove duplicate tool entries");
            }
            if assignment.depends_on.iter().collect::<HashSet<_>>().len()
                != assignment.depends_on.len()
            {
                return invalid_field(&field("depends_on"), "remove duplicate dependency keys");
            }
        }
        let total: u64 = self.assignments.iter().map(|a| a.token_budget).sum();
        if self.token_budget == 0 || self.token_budget > 1_000_000 {
            return invalid_field(
                "token_budget",
                "use an integer between 1 and 1000000 within the owner's allowance",
            );
        }
        if total > self.token_budget {
            return invalid_field("assignments.token_budget", "worker allocations exceed the plan total. Rebalance within the existing total and leave room for coordination");
        }
        for (index, assignment) in self.assignments.iter().enumerate() {
            if assignment
                .depends_on
                .iter()
                .any(|key| !keys.contains(key.as_str()) || key == &assignment.key)
            {
                return invalid_field(&format!("assignments[{index}].depends_on"), "reference another assignment's key in this plan, never an agent_key, agent name or this assignment's own key. Use [] for independent work");
            }
        }
        // Topological elimination validates cycles without relying on array order.
        let mut resolved = HashSet::new();
        loop {
            let before = resolved.len();
            for assignment in &self.assignments {
                if assignment
                    .depends_on
                    .iter()
                    .all(|key| resolved.contains(key.as_str()))
                {
                    resolved.insert(assignment.key.as_str());
                }
            }
            if resolved.len() == self.assignments.len() {
                break;
            }
            if before == resolved.len() {
                return invalid_field("assignments.depends_on", "dependencies contain a cycle. Preserve real prerequisites in an acyclic graph; independent assignments use []");
            }
        }
        if serde_json::to_vec(self)
            .map_err(|_| StoreError::ControlResourceConflict)?
            .len()
            > 48_000
        {
            return invalid_field("plan", "serialized plan must be at most 48000 bytes; shorten text without removing requested work");
        }
        Ok(())
    }
}

fn invalid_field<T>(field: &str, reason: &str) -> Result<T> {
    // Paths are engine-authored indices/names, never echoed model text.
    Err(StoreError::InvalidControlResource(format!(
        "plan.{field}: {reason}"
    )))
}

fn field_text(field: &str, value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        invalid_field(
            field,
            &format!("provide nonempty text of at most {max} UTF-8 bytes without null characters"),
        )
    } else {
        Ok(())
    }
}

fn invalid<T>() -> Result<T> {
    Err(StoreError::InvalidControlResource(
        "plan: use 1–12 unique assignments with valid dependencies and bounded text".into(),
    ))
}
fn text(value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        invalid()
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn seed(path: &std::path::Path) -> Store {
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
                work_id: "shape",
                title: "Question",
                request_id: "request",
                goal_id: None,
            },
            Some("private discussion"),
            crate::WorkPurpose::Explore,
        )
        .unwrap();
        db.save_work_brief(crate::SaveWorkBrief {
            actor: "owner",
            org: "org",
            team: "team",
            work: "shape",
            request: "brief",
            expected: 0,
            body: "shared direction",
        })
        .unwrap();
        db
    }
    fn content() -> PlanContent {
        serde_json::from_value(serde_json::json!({"title":"Compare options","summary":"Gather evidence and compare","token_budget":4000,"open_questions":[],"assignments":[
            {"key":"research","title":"Gather","instructions":"Read supplied evidence","agent_key":"Researcher","depends_on":[],"tools":[],"deliverable":"Cited facts","token_budget":2000},
            {"key":"compare","title":"Compare","instructions":"Use those facts","agent_key":"Analyst","depends_on":["research"],"tools":[],"deliverable":"Tradeoffs","token_budget":2000}
        ]})).unwrap()
    }
    #[test]
    fn validates_graph_budgets_and_bounded_payloads() {
        let plan = content();
        plan.validate().unwrap();
        let mut cycle = plan.clone();
        cycle.assignments[0].depends_on = vec!["compare".into()];
        assert!(cycle.validate().is_err());
        let mut missing = plan.clone();
        missing.assignments[1].depends_on = vec!["imaginary".into()];
        assert!(missing.validate().is_err());
        let mut duplicate = plan.clone();
        duplicate.assignments[1].key = "research".into();
        assert!(duplicate.validate().is_err());
        let mut budget = plan.clone();
        budget.token_budget = 3999;
        assert!(budget.validate().is_err());
        let mut zero = plan.clone();
        zero.assignments[0].token_budget = 0;
        assert!(zero.validate().is_err());
        let mut too_long = plan;
        too_long.assignments[0].instructions = "x".repeat(4001);
        assert!(too_long.validate().is_err());
    }
    #[test]
    fn validation_identifies_repairable_fields_without_echoing_private_text() {
        let mut plan = content();
        plan.assignments[0].key = "PRIVATE AGENT NAME".into();
        let error = plan.validate().unwrap_err().to_string();
        assert!(error.contains("assignments[0].key") && error.contains("no spaces"));
        assert!(!error.contains("PRIVATE AGENT NAME"));
        plan.assignments[0].key = "research".into();
        plan.assignments[1].depends_on = vec!["Researcher".into()];
        let error = plan.validate().unwrap_err().to_string();
        assert!(
            error.contains("assignments[1].depends_on") && error.contains("never an agent_key")
        );
        plan.assignments[1].depends_on = vec!["research".into()];
        plan.assignments[0].instructions = "private".repeat(600);
        let error = plan.validate().unwrap_err().to_string();
        assert!(error.contains("assignments[0].instructions") && error.contains("4000"));
        assert!(!error.contains("private"));
        plan.assignments[0].instructions = "Read supplied evidence".into();
        plan.token_budget = 3999;
        assert!(plan
            .validate()
            .unwrap_err()
            .to_string()
            .contains("allocations exceed the plan total"));
    }
    #[test]
    fn durable_huddle_revisions_agreement_retries_and_stale_direction() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plans.db");
        let db = seed(&path);
        let generated = db
            .save_huddle_plan(crate::SaveHuddlePlan {
                actor: "owner",
                org: "org",
                team: "team",
                work: "shape",
                request: "gen",
                expected: 0,
                brief_revision: 1,
                generation_id: "gen",
                generation_input: "only shared brief",
                content: None,
            })
            .unwrap();
        assert_eq!(generated.status, "drafting");
        assert!(db.huddle_plans("outsider", "org", "team", "shape").is_err());
        assert!(db
            .save_huddle_plan(crate::SaveHuddlePlan {
                actor: "outsider",
                org: "org",
                team: "team",
                work: "shape",
                request: "bad",
                expected: 0,
                brief_revision: 1,
                generation_id: "bad",
                generation_input: "prompt",
                content: None
            })
            .is_err());
        let captured = db
            .capture_huddle_plan("owner", "org", "team", "shape", 1, &content())
            .unwrap();
        assert_eq!(captured.status, "draft");
        assert_eq!(
            db.save_huddle_plan(crate::SaveHuddlePlan {
                actor: "owner",
                org: "org",
                team: "team",
                work: "shape",
                request: "gen",
                expected: 0,
                brief_revision: 1,
                generation_id: "gen",
                generation_input: "only shared brief",
                content: None
            })
            .unwrap(),
            captured
        );
        assert!(db
            .save_huddle_plan(crate::SaveHuddlePlan {
                actor: "owner",
                org: "org",
                team: "team",
                work: "shape",
                request: "gen",
                expected: 0,
                brief_revision: 1,
                generation_id: "gen",
                generation_input: "changed prompt",
                content: None
            })
            .is_err());
        assert!(db
            .save_huddle_plan(crate::SaveHuddlePlan {
                actor: "owner",
                org: "org",
                team: "team",
                work: "shape",
                request: "stale",
                expected: 0,
                brief_revision: 1,
                generation_id: "gen",
                generation_input: "only shared brief",
                content: Some(&content())
            })
            .is_err());
        let agreed = db
            .agree_huddle_plan("owner", "org", "team", "shape", 1, "agree")
            .unwrap();
        assert_eq!(agreed.status, "agreed");
        assert_eq!(
            db.agree_huddle_plan("owner", "org", "team", "shape", 1, "agree")
                .unwrap(),
            agreed
        );
        assert!(db
            .agree_huddle_plan("owner", "org", "team", "shape", 1, "other")
            .is_err());
        db.save_work_brief(crate::SaveWorkBrief {
            actor: "owner",
            org: "org",
            team: "team",
            work: "shape",
            request: "brief-2",
            expected: 1,
            body: "changed direction",
        })
        .unwrap();
        assert!(db
            .save_huddle_plan(crate::SaveHuddlePlan {
                actor: "owner",
                org: "org",
                team: "team",
                work: "shape",
                request: "edit",
                expected: 1,
                brief_revision: 1,
                generation_id: "gen",
                generation_input: "only shared brief",
                content: Some(&content())
            })
            .is_err());
        let edited = db
            .save_huddle_plan(crate::SaveHuddlePlan {
                actor: "owner",
                org: "org",
                team: "team",
                work: "shape",
                request: "edit",
                expected: 1,
                brief_revision: 2,
                generation_id: "gen",
                generation_input: "only shared brief",
                content: Some(&content()),
            })
            .unwrap();
        assert_eq!(edited.status, "draft");
        assert_eq!(edited.revision, 2);
        // Exact old receipts remain recoverable after the current direction changes.
        assert_eq!(
            db.agree_huddle_plan("owner", "org", "team", "shape", 1, "agree")
                .unwrap(),
            agreed
        );
        db.save_work_brief(crate::SaveWorkBrief {
            actor: "owner",
            org: "org",
            team: "team",
            work: "shape",
            request: "brief-3",
            expected: 2,
            body: "another change",
        })
        .unwrap();
        assert!(db
            .agree_huddle_plan("owner", "org", "team", "shape", 2, "agree-2")
            .is_err());
        assert_eq!(
            db.list_team_work_items("owner", "org", "team")
                .unwrap()
                .len(),
            1
        );
        assert!(db
            .get_team_work_item("org", "team", "shape")
            .unwrap()
            .unwrap()
            .run_id
            .is_none());
        assert!(db
            .accept_huddle_proposal(
                "owner",
                &crate::HuddleProposal {
                    org_id: "org".into(),
                    team_id: "team".into(),
                    huddle_id: "shape".into(),
                    proposal_version: 3,
                    status: "accepted".into(),
                    request_id: "legacy-bypass".into(),
                    created_by: "owner".into(),
                    work_titles: vec!["Execute".into()]
                }
            )
            .is_err());
        drop(db);
        let reopened = Store::open(&path).unwrap();
        assert_eq!(
            reopened
                .huddle_plans("owner", "org", "team", "shape")
                .unwrap(),
            vec![edited, agreed]
        );
        reopened.migrate_huddle_plans_v54().unwrap();
    }
    #[test]
    fn migrates_existing_huddles_without_erasing_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        let db = seed(&path);
        db.accept_huddle_proposal(
            "owner",
            &crate::HuddleProposal {
                org_id: "org".into(),
                team_id: "team".into(),
                huddle_id: "old".into(),
                proposal_version: 1,
                status: "accepted".into(),
                request_id: "old".into(),
                created_by: "owner".into(),
                work_titles: vec!["Historic".into()],
            },
        )
        .unwrap();
        db.conn
            .execute_batch("DROP INDEX huddle_agreement_receipts;")
            .unwrap();
        for column in [
            "source_work_id",
            "brief_revision",
            "generation_id",
            "generation_input",
            "plan_json",
            "expected_revision",
            "agreement_id",
            "agreed_by",
        ] {
            db.conn
                .execute_batch(&format!(
                    "ALTER TABLE huddle_proposals DROP COLUMN {column}"
                ))
                .unwrap();
        }
        db.conn
            .execute("DELETE FROM schema_versions WHERE version>=54", [])
            .unwrap();
        db.remove_agent_edits_schema_for_test();
        drop(db);
        let migrated = Store::open(path).unwrap();
        assert_eq!(
            migrated
                .list_team_work_items("owner", "org", "team")
                .unwrap()
                .len(),
            2
        );
        assert!(migrated
            .huddle_plans("owner", "org", "team", "shape")
            .unwrap()
            .is_empty());
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HuddlePlan {
    pub work_id: String,
    pub revision: i64,
    pub brief_revision: i64,
    pub request_id: String,
    pub generation_id: String,
    pub status: String,
    pub content: Option<PlanContent>,
    pub created_by: String,
    pub agreed_by: Option<String>,
    pub agreement_id: Option<String>,
    // Host-composed prompt, retained for exact retries. Not returned by HTTP.
    #[serde(skip)]
    pub generation_input: String,
}

impl Store {
    pub fn huddle_generation_ids(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<Vec<(String, String)>> {
        self.require_team_participant(actor, org, team)?;
        let mut stmt=self.conn.prepare("SELECT DISTINCT generation_id,source_work_id FROM huddle_proposals WHERE org_id=?1 AND team_id=?2 AND source_work_id IS NOT NULL")?;
        let rows = stmt
            .query_map(params![org, team], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    pub(crate) fn migrate_huddle_plans_v54(&self) -> Result<()> {
        let applied: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version=54)",
            [],
            |r| r.get(0),
        )?;
        if applied {
            return Ok(());
        }
        for (column, kind) in [
            ("source_work_id", "TEXT"),
            ("brief_revision", "INTEGER"),
            ("generation_id", "TEXT"),
            ("generation_input", "TEXT"),
            ("plan_json", "TEXT"),
            ("expected_revision", "INTEGER"),
            ("agreement_id", "TEXT"),
            ("agreed_by", "TEXT"),
            ("plan_request_json", "TEXT"),
        ] {
            let exists: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('huddle_proposals') WHERE name=?1)",
                [column],
                |r| r.get(0),
            )?;
            if !exists {
                self.conn.execute_batch(&format!(
                    "ALTER TABLE huddle_proposals ADD COLUMN {column} {kind}"
                ))?;
            }
        }
        self.conn.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS huddle_agreement_receipts ON huddle_proposals(org_id,team_id,agreement_id) WHERE agreement_id IS NOT NULL;")?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(54,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub fn huddle_plans(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
    ) -> Result<Vec<HuddlePlan>> {
        self.require_team_participant(actor, org, team)?;
        if self.get_team_work_item(org, team, work)?.is_none() {
            return Err(StoreError::ControlAccessDenied);
        }
        let mut stmt = self.conn.prepare("SELECT proposal_version,brief_revision,request_id,generation_id,status,plan_json,created_by,generation_input,agreed_by,agreement_id FROM huddle_proposals WHERE org_id=?1 AND team_id=?2 AND source_work_id=?3 ORDER BY proposal_version DESC LIMIT 50")?;
        let rows = stmt
            .query_map(params![org, team, work], |r| {
                let json: Option<String> = r.get(5)?;
                Ok(HuddlePlan {
                    work_id: work.into(),
                    revision: r.get(0)?,
                    brief_revision: r.get(1)?,
                    request_id: r.get(2)?,
                    generation_id: r.get(3)?,
                    status: r.get(4)?,
                    content: json
                        .map(|s| serde_json::from_str(&s))
                        .transpose()
                        .map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                5,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?,
                    created_by: r.get(6)?,
                    generation_input: r.get(7)?,
                    agreed_by: r.get(8)?,
                    agreement_id: r.get(9)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Append a generation request or a human-edited draft to the existing huddle.
    /// A source brief is mandatory; stale direction cannot silently replace it.
    pub fn save_huddle_plan(&self, command: crate::SaveHuddlePlan<'_>) -> Result<HuddlePlan> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let row = self.save_huddle_plan_in_transaction(command)?;
        tx.commit()?;
        Ok(row)
    }

    pub(crate) fn save_huddle_plan_in_transaction(
        &self,
        command: crate::SaveHuddlePlan<'_>,
    ) -> Result<HuddlePlan> {
        debug_assert!(!self.conn.is_autocommit());
        let crate::SaveHuddlePlan {
            actor,
            org,
            team,
            work,
            request,
            expected,
            brief_revision,
            generation_id,
            generation_input,
            content,
        } = command;
        text(request, 128)?;
        text(generation_id, 128)?;
        text(generation_input, 12_000)?;
        if expected < 0 || expected == i64::MAX || brief_revision < 1 {
            return invalid();
        }
        if let Some(content) = content {
            self.validate_work_team_assignments(actor, org, team, work, content)?;
            content.validate()?;
        }
        self.require_team_participant(actor, org, team)?;
        let rows = self.huddle_plans(actor, org, team, work)?;
        let retry: Option<i64> = self.conn.query_row("SELECT proposal_version FROM huddle_proposals WHERE org_id=?1 AND team_id=?2 AND request_id=?3",params![org,team,request],|r|r.get(0)).optional()?;
        if let Some(version) = retry {
            let old = rows
                .into_iter()
                .find(|r| r.revision == version && r.request_id == request)
                .ok_or(StoreError::ControlResourceConflict)?;
            // Generated content can later be captured, so compare the original request payload.
            let initial_json: String = self.conn.query_row("SELECT plan_request_json FROM huddle_proposals WHERE org_id=?1 AND team_id=?2 AND request_id=?3",params![org,team,request],|r|r.get(0))?;
            if old.revision != expected + 1
                || old.brief_revision != brief_revision
                || old.created_by != actor
                || old.generation_id != generation_id
                || old.generation_input != generation_input
                || initial_json != serde_json::to_string(&content).unwrap()
            {
                return Err(StoreError::ControlResourceConflict);
            }
            return Ok(old);
        }
        let source = self
            .get_team_work_item(org, team, work)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if source.purpose != crate::WorkPurpose::Explore {
            return Err(StoreError::ControlResourceConflict);
        }
        let latest_brief: i64 = self.conn.query_row("SELECT COALESCE(MAX(revision),0) FROM work_brief_revisions WHERE org_id=?1 AND team_id=?2 AND work_id=?3",params![org,team,work],|r|r.get(0))?;
        if rows.first().map_or(0, |r| r.revision) != expected || latest_brief != brief_revision {
            return Err(StoreError::ControlResourceConflict);
        }
        let revision = expected
            .checked_add(1)
            .ok_or(StoreError::ControlResourceConflict)?;
        let initial_json = serde_json::to_string(&content).unwrap();
        let status = if content.is_some() {
            "draft"
        } else {
            "drafting"
        };
        let json = content.map(|c| serde_json::to_string(c).unwrap());
        let titles = content
            .map(|c| {
                c.assignments
                    .iter()
                    .map(|a| a.title.as_str())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.conn.execute("INSERT INTO huddle_proposals(org_id,team_id,huddle_id,proposal_version,status,request_id,created_by,work_titles_json,created_at,source_work_id,brief_revision,generation_id,generation_input,plan_json,expected_revision,plan_request_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?3,?10,?11,?12,?13,?14,?15)",params![org,team,work,revision,status,request,actor,serde_json::to_string(&titles).unwrap(),crate::util::now(),brief_revision,generation_id,generation_input,json,expected,initial_json])?;
        let row = self.huddle_plans(actor, org, team, work)?.remove(0);
        Ok(row)
    }

    /// The host captures validated output from the exact recorded generation run.
    pub fn capture_huddle_plan(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
        revision: i64,
        content: &PlanContent,
    ) -> Result<HuddlePlan> {
        self.validate_work_team_assignments(actor, org, team, work, content)?;
        content.validate()?;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let row = self
            .huddle_plans(actor, org, team, work)?
            .into_iter()
            .find(|r| r.revision == revision)
            .ok_or(StoreError::ControlAccessDenied)?;
        if let Some(existing) = &row.content {
            if existing != content {
                return Err(StoreError::ControlResourceConflict);
            }
            tx.commit()?;
            return Ok(row);
        }
        if row.status != "drafting" || row.created_by != actor {
            return Err(StoreError::ControlAccessDenied);
        }
        let titles: Vec<_> = content
            .assignments
            .iter()
            .map(|a| a.title.as_str())
            .collect();
        self.conn.execute("UPDATE huddle_proposals SET plan_json=?5,status='draft',work_titles_json=?6 WHERE org_id=?1 AND team_id=?2 AND source_work_id=?3 AND proposal_version=?4",params![org,team,work,revision,serde_json::to_string(content).unwrap(),serde_json::to_string(&titles).unwrap()])?;
        let updated = self
            .huddle_plans(actor, org, team, work)?
            .into_iter()
            .find(|r| r.revision == revision)
            .unwrap();
        tx.commit()?;
        Ok(updated)
    }

    pub fn agree_huddle_plan(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
        revision: i64,
        request: &str,
    ) -> Result<HuddlePlan> {
        text(request, 128)?;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let rows = self.huddle_plans(actor, org, team, work)?;
        let row = rows
            .iter()
            .find(|r| r.revision == revision)
            .ok_or(StoreError::ControlAccessDenied)?;
        if row.agreement_id.as_deref() == Some(request) && row.agreed_by.as_deref() == Some(actor) {
            let row = row.clone();
            tx.commit()?;
            return Ok(row);
        }
        let latest_brief:i64=self.conn.query_row("SELECT COALESCE(MAX(revision),0) FROM work_brief_revisions WHERE org_id=?1 AND team_id=?2 AND work_id=?3",params![org,team,work],|r|r.get(0))?;
        if row.status != "draft"
            || rows.first().map(|r| r.revision) != Some(revision)
            || latest_brief != row.brief_revision
        {
            return Err(StoreError::ControlResourceConflict);
        }
        self.conn.execute("UPDATE huddle_proposals SET status='agreed',agreement_id=?5,agreed_by=?6 WHERE org_id=?1 AND team_id=?2 AND source_work_id=?3 AND proposal_version=?4",params![org,team,work,revision,request,actor])?;
        let updated = self.huddle_plans(actor, org, team, work)?.remove(0);
        tx.commit()?;
        Ok(updated)
    }
}
