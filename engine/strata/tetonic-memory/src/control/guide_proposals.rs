//! Bounded proposal correction and atomic publication into the existing brief/huddle owners.
use crate::{HuddlePlan, PlanContent, Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

/// Host-composed authority and preflight result, never a model or HTTP command.
pub struct SubmitGuideProposal<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub team: &'a str,
    pub source: &'a str,
    pub turn: &'a str,
    pub call: &'a str,
    pub request: &'a str,
    pub expected_plan: i64,
    pub expected_brief: i64,
    pub direction: &'a str,
    pub content: &'a PlanContent,
    pub generation_id: &'a str,
    pub generation_input: &'a str,
    pub validation_error: Option<&'a str>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum GuideProposalOutcome {
    Saved { plan: Box<HuddlePlan> },
    RepairNeeded { reason: String },
    RepairFailed { reason: String },
}

fn allocation_only(original: &PlanContent, candidate: &PlanContent) -> bool {
    let mut comparison = candidate.clone();
    if original.assignments.len() != comparison.assignments.len() {
        return false;
    }
    for (first, next) in original.assignments.iter().zip(&mut comparison.assignments) {
        next.token_budget = first.token_budget;
    }
    &comparison == original
}

impl Store {
    pub(crate) fn migrate_guide_proposals_v75(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS guide_proposal_attempts (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, turn_id TEXT NOT NULL,
            ordinal INTEGER NOT NULL CHECK(ordinal IN (0,1)), call_id TEXT NOT NULL,
            request_json TEXT NOT NULL, outcome_json TEXT NOT NULL,
            PRIMARY KEY(org_id,team_id,turn_id,ordinal),
            UNIQUE(org_id,team_id,turn_id,call_id),
            FOREIGN KEY(org_id,team_id,turn_id) REFERENCES team_work_items(org_id,team_id,work_id));")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions VALUES(75,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub fn submit_guide_proposal(
        &self,
        input: SubmitGuideProposal<'_>,
    ) -> Result<GuideProposalOutcome> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let SubmitGuideProposal {
            actor,
            org,
            team,
            source,
            turn,
            call,
            request,
            expected_plan,
            expected_brief,
            direction,
            content,
            generation_id,
            generation_input,
            validation_error,
        } = input;
        self.require_team_participant(actor, org, team)?;
        for id in [source, turn] {
            let work = self
                .get_team_work_item(org, team, id)?
                .ok_or(StoreError::ControlAccessDenied)?;
            if work.created_by != actor || work.purpose != crate::WorkPurpose::Explore {
                return Err(StoreError::ControlAccessDenied);
            }
        }
        if call.is_empty()
            || call.len() > 512
            || call.contains('\0')
            || expected_plan < 0
            || expected_brief < 0
            || expected_plan == i64::MAX
            || expected_brief == i64::MAX
        {
            return Err(StoreError::InvalidControlResource(
                "Invalid proposal receipt".into(),
            ));
        }
        let encoded = serde_json::json!({"actor":actor,"source":source,"request":request,
            "expected_plan":expected_plan,"expected_brief":expected_brief,"direction":direction,"content":content,
            "generation_id":generation_id,"generation_input":generation_input}).to_string();
        let prior: Option<(String,String)> = self.conn.query_row(
            "SELECT request_json,outcome_json FROM guide_proposal_attempts WHERE org_id=?1 AND team_id=?2 AND turn_id=?3 AND call_id=?4",
            params![org,team,turn,call], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        if let Some((previous, outcome)) = prior {
            if previous != encoded {
                return Err(StoreError::ControlResourceConflict);
            }
            return serde_json::from_str(&outcome).map_err(|_| StoreError::ControlResourceConflict);
        }
        let plans = self.huddle_plans(actor, org, team, source)?;
        // Preserve successful exact retries even when the provider chooses a new call ID.
        if let Some(plan) = plans.iter().find(|p| p.request_id == request) {
            let briefs = self.work_briefs(actor, org, team, source)?;
            if plan.content.as_ref() == Some(content)
                && plan.revision == expected_plan + 1
                && plan.brief_revision == expected_brief + 1
                && briefs
                    .iter()
                    .any(|b| b.revision == plan.brief_revision && b.body == direction)
            {
                return Ok(GuideProposalOutcome::Saved {
                    plan: Box::new(plan.clone()),
                });
            }
            return Err(StoreError::ControlResourceConflict);
        }
        let started: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM huddle_executions WHERE org_id=?1 AND team_id=?2 AND source_work_id=?3)",
            params![org,team,source], |r|r.get(0))?;
        let brief_revision = self
            .work_briefs(actor, org, team, source)?
            .first()
            .map_or(0, |b| b.revision);
        if started
            || plans.first().map_or(0, |p| p.revision) != expected_plan
            || brief_revision != expected_brief
        {
            return Err(StoreError::ControlResourceConflict);
        }
        let attempts: Vec<String> = self.conn.prepare(
            "SELECT request_json FROM guide_proposal_attempts WHERE org_id=?1 AND team_id=?2 AND turn_id=?3 ORDER BY ordinal")?
            .query_map(params![org,team,turn], |r|r.get(0))?.collect::<std::result::Result<_,_>>()?;
        if attempts.len() >= 2 {
            return Ok(GuideProposalOutcome::RepairFailed { reason: "The proposal could not be corrected within this reply. Stop proposing; explain the remaining issue and ask for direction. The previous proposal is unchanged. Do not start work.".into() });
        }
        let changed_scope = if let Some(first) = attempts.first() {
            let first: serde_json::Value =
                serde_json::from_str(first).map_err(|_| StoreError::ControlResourceConflict)?;
            let original: PlanContent = serde_json::from_value(first["content"].clone())
                .map_err(|_| StoreError::ControlResourceConflict)?;
            first["source"] != source
                || first["direction"] != direction
                || !allocation_only(&original, content)
        } else {
            false
        };
        let outcome = if changed_scope {
            GuideProposalOutcome::RepairFailed { reason: "Correction changed the scope, total allowance, contributors, tools, dependencies or promised output. Keep the previous proposal unchanged and ask the owner before changing the approach. Do not propose again in this reply.".into() }
        } else if let Some(reason) = validation_error {
            if attempts.is_empty() {
                GuideProposalOutcome::RepairNeeded { reason: format!("{reason} You may make one corrected proposal within this reply's existing budget. Change only assignment token allocations; preserve the shared direction, total, contributors, tools, dependencies and all requested deliverables. If that is not possible, explain the blocker without another proposal.") }
            } else {
                GuideProposalOutcome::RepairFailed { reason: format!("{reason} The one correction attempt did not produce a valid proposal. Stop proposing in this reply and explain what needs to change; no work was started and the previous proposal is unchanged.") }
            }
        } else {
            let brief = self.save_work_brief_in_transaction(crate::SaveWorkBrief {
                actor,
                org,
                team,
                work: source,
                request,
                expected: expected_brief,
                body: direction,
            })?;
            let plan = self.save_huddle_plan_in_transaction(crate::SaveHuddlePlan {
                actor,
                org,
                team,
                work: source,
                request,
                expected: expected_plan,
                brief_revision: brief.revision,
                generation_id,
                generation_input,
                content: Some(content),
            })?;
            GuideProposalOutcome::Saved {
                plan: Box::new(plan),
            }
        };
        self.conn.execute(
            "INSERT INTO guide_proposal_attempts VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                org,
                team,
                turn,
                attempts.len() as i64,
                call,
                encoded,
                serde_json::to_string(&outcome).unwrap()
            ],
        )?;
        tx.commit()?;
        Ok(outcome)
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
                work_id: "discussion",
                title: "Discuss",
                request_id: "discussion",
                goal_id: None,
            },
            None,
            crate::WorkPurpose::Explore,
        )
        .unwrap();
        db
    }
    fn plan() -> PlanContent {
        serde_json::from_value(serde_json::json!({"title":"Compare options","summary":"Compare supplied options","token_budget":1000,"open_questions":[],"assignments":[
            {"key":"compare","title":"Compare","instructions":"Read the supplied options","agent_key":"worker","depends_on":[],"tools":[],"deliverable":"An evidence-based comparison","token_budget":1000}
        ]})).unwrap()
    }
    fn input<'a>(
        call: &'a str,
        content: &'a PlanContent,
        error: Option<&'a str>,
    ) -> SubmitGuideProposal<'a> {
        SubmitGuideProposal {
            actor: "owner",
            org: "org",
            team: "team",
            source: "discussion",
            turn: "discussion",
            call,
            request: "proposal",
            expected_plan: 0,
            expected_brief: 0,
            direction: "Compare the options",
            content,
            generation_id: "origin",
            generation_input: "Guide conversation",
            validation_error: error,
        }
    }
    #[test]
    fn correction_is_durable_retry_safe_and_publishes_one_atomic_proposal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("guide.db");
        let db = seed(&path);
        let original = plan();
        assert!(matches!(
            db.submit_guide_proposal(input("first", &original, Some("No coordination allowance")))
                .unwrap(),
            GuideProposalOutcome::RepairNeeded { .. }
        ));
        assert!(db
            .work_briefs("owner", "org", "team", "discussion")
            .unwrap()
            .is_empty());
        assert!(db
            .huddle_plans("owner", "org", "team", "discussion")
            .unwrap()
            .is_empty());
        drop(db);
        let db = Store::open(&path).unwrap();
        // Changed current validation does not rewrite the receipt of an exact call.
        assert!(matches!(
            db.submit_guide_proposal(input("first", &original, None))
                .unwrap(),
            GuideProposalOutcome::RepairNeeded { .. }
        ));
        let mut fixed = original.clone();
        fixed.assignments[0].token_budget = 744;
        assert!(db
            .submit_guide_proposal(input("first", &fixed, None))
            .is_err());
        for call in ["repair", "repair", "provider-retry"] {
            let GuideProposalOutcome::Saved { plan } =
                db.submit_guide_proposal(input(call, &fixed, None)).unwrap()
            else {
                panic!("not saved")
            };
            assert_eq!(plan.revision, 1);
            assert_eq!(plan.content, Some(fixed.clone()));
        }
        assert_eq!(
            db.work_briefs("owner", "org", "team", "discussion")
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            db.huddle_plans("owner", "org", "team", "discussion")
                .unwrap()
                .len(),
            1
        );
        let mut outsider = input("outsider", &fixed, None);
        outsider.actor = "other";
        assert!(db.submit_guide_proposal(outsider).is_err());
    }
    #[test]
    fn one_failed_correction_stops_and_cannot_change_scope_or_raise_the_total() {
        for variant in 0..8 {
            let dir = tempfile::tempdir().unwrap();
            let db = seed(&dir.path().join("guide.db"));
            let original = plan();
            db.submit_guide_proposal(input("first", &original, Some("No coordination allowance")))
                .unwrap();
            let mut candidate = original.clone();
            candidate.assignments[0].token_budget = 744;
            let mut error = None;
            match variant {
                0 => candidate.token_budget += 256,
                1 => candidate.assignments[0].deliverable = "Less useful work".into(),
                2 => candidate.assignments[0].agent_key = "another-worker".into(),
                3 => candidate.assignments[0].tools.push("run_shell".into()),
                4 => candidate.assignments[0].instructions = "Different scope".into(),
                5 => candidate.open_questions.push("Different objective?".into()),
                6 => candidate.assignments[0]
                    .depends_on
                    .push("new-dependency".into()),
                _ => error = Some("Still invalid"),
            }
            assert!(matches!(
                db.submit_guide_proposal(input("repair", &candidate, error))
                    .unwrap(),
                GuideProposalOutcome::RepairFailed { .. }
            ));
            let mut valid = original.clone();
            valid.assignments[0].token_budget = 744;
            assert!(matches!(
                db.submit_guide_proposal(input("third", &valid, None))
                    .unwrap(),
                GuideProposalOutcome::RepairFailed { .. }
            ));
            assert!(db
                .work_briefs("owner", "org", "team", "discussion")
                .unwrap()
                .is_empty());
            assert!(db
                .huddle_plans("owner", "org", "team", "discussion")
                .unwrap()
                .is_empty());
        }
    }
    #[test]
    fn failed_plan_write_rolls_back_the_brief_and_owner_edits_win_over_a_correction() {
        let dir = tempfile::tempdir().unwrap();
        let db = seed(&dir.path().join("guide.db"));
        let mut valid = plan();
        valid.assignments[0].token_budget = 744;
        db.conn.execute_batch("CREATE TRIGGER fail_proposal BEFORE INSERT ON huddle_proposals BEGIN SELECT RAISE(ABORT, 'injected failure'); END;").unwrap();
        assert!(db
            .submit_guide_proposal(input("write", &valid, None))
            .is_err());
        assert!(db
            .work_briefs("owner", "org", "team", "discussion")
            .unwrap()
            .is_empty());
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM guide_proposal_attempts", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
        db.conn.execute_batch("DROP TRIGGER fail_proposal").unwrap();
        let original = plan();
        db.submit_guide_proposal(input("first", &original, Some("Needs correction")))
            .unwrap();
        db.save_work_brief(crate::SaveWorkBrief {
            actor: "owner",
            org: "org",
            team: "team",
            work: "discussion",
            request: "owner-edit",
            expected: 0,
            body: "Owner's new direction",
        })
        .unwrap();
        assert!(db
            .submit_guide_proposal(input("repair", &valid, None))
            .is_err());
        assert_eq!(
            db.work_briefs("owner", "org", "team", "discussion")
                .unwrap()[0]
                .body,
            "Owner's new direction"
        );
        assert!(db
            .huddle_plans("owner", "org", "team", "discussion")
            .unwrap()
            .is_empty());
    }
}
