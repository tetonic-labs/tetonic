//! Versioned capability ceilings. Existing grants continue to own resource access.
use crate::{Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tetonic_policy::capabilities::CapabilityPolicy;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityScope {
    Workspace,
    Team,
    Agent,
}
impl CapabilityScope {
    fn key(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Team => "team",
            Self::Agent => "agent",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedCapabilityPolicy {
    pub scope: CapabilityScope,
    pub scope_id: String,
    pub revision: i64,
    pub policy: Option<CapabilityPolicy>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveCapabilityPolicy {
    pub scope: CapabilityScope,
    pub scope_id: String,
    pub expected_revision: i64,
    pub request_id: String,
    pub policy: Option<CapabilityPolicy>,
}
impl Store {
    pub(crate) fn migrate_capability_policies_v73(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS capability_policy_versions (
          org_id TEXT NOT NULL, team_id TEXT NOT NULL, scope TEXT NOT NULL, scope_id TEXT NOT NULL,
          revision INTEGER NOT NULL, request_id TEXT NOT NULL, payload TEXT NOT NULL,
          PRIMARY KEY(org_id,team_id,scope,scope_id,revision), UNIQUE(org_id,team_id,request_id),
          FOREIGN KEY(org_id,team_id) REFERENCES teams(org_id,team_id));
          CREATE TRIGGER IF NOT EXISTS capability_policy_no_update BEFORE UPDATE ON capability_policy_versions
          BEGIN SELECT RAISE(ABORT,'capability policy revisions are immutable'); END;
          CREATE TRIGGER IF NOT EXISTS capability_policy_no_delete BEFORE DELETE ON capability_policy_versions
          BEGIN SELECT RAISE(ABORT,'capability policy revisions are immutable'); END;")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions VALUES(73,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }
    pub fn capability_policies(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<Vec<ScopedCapabilityPolicy>> {
        self.require_team_participant(actor, org, team)?;
        let mut query=self.conn.prepare("SELECT payload FROM capability_policy_versions v WHERE org_id=?1 AND team_id=?2 AND revision=(SELECT MAX(revision) FROM capability_policy_versions WHERE org_id=v.org_id AND team_id=v.team_id AND scope=v.scope AND scope_id=v.scope_id) ORDER BY scope,scope_id")?;
        let rows = query
            .query_map(params![org, team], |r| r.get::<_, String>(0))?
            .map(|r| decode(&r?))
            .collect();
        rows
    }
    pub fn save_capability_policy(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        input: &SaveCapabilityPolicy,
    ) -> Result<ScopedCapabilityPolicy> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        if self
            .get_team(org, team)?
            .map_or(true, |t| t.owner_principal_id != actor)
        {
            return Err(StoreError::ControlAccessDenied);
        }
        if input.expected_revision < 0
            || input.expected_revision == i64::MAX
            || uuid::Uuid::parse_str(&input.request_id).is_err()
        {
            return Err(StoreError::ControlResourceConflict);
        }
        let valid=match input.scope {
            CapabilityScope::Workspace => input.scope_id.is_empty(),
            CapabilityScope::Team => self.work_teams(actor,org,team)?.iter().any(|t|t.id==input.scope_id),
            CapabilityScope::Agent => self.conn.query_row("SELECT EXISTS(SELECT 1 FROM organization_agents WHERE org_id=?1 AND identity_id=?2)",params![org,input.scope_id],|r|r.get(0))?,
        };
        if !valid {
            return Err(StoreError::ControlAccessDenied);
        }
        if let Some(tetonic_policy::capabilities::CommunicationScope::SelectedAgents {
            agent_ids,
        }) = input.policy.as_ref().and_then(|p| p.communication.as_ref())
        {
            if agent_ids.len() > 128
                || agent_ids
                    .iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    != agent_ids.len()
            {
                return Err(StoreError::InvalidControlResource(
                    "Select at most 128 distinct agents".into(),
                ));
            }
            for id in agent_ids {
                let exists:bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM organization_agents WHERE org_id=?1 AND identity_id=?2)",params![org,id],|r|r.get(0))?;
                if !exists {
                    return Err(StoreError::ControlAccessDenied);
                }
            }
        }
        let row = ScopedCapabilityPolicy {
            scope: input.scope,
            scope_id: input.scope_id.clone(),
            revision: input.expected_revision + 1,
            policy: input.policy.clone(),
        };
        let old:Option<String>=self.conn.query_row("SELECT payload FROM capability_policy_versions WHERE org_id=?1 AND team_id=?2 AND request_id=?3",params![org,team,input.request_id],|r|r.get(0)).optional()?;
        if let Some(old) = old {
            if decode(&old)? != row {
                return Err(StoreError::ControlResourceConflict);
            }
            tx.commit()?;
            return Ok(row);
        }
        let revision:i64=self.conn.query_row("SELECT COALESCE(MAX(revision),0) FROM capability_policy_versions WHERE org_id=?1 AND team_id=?2 AND scope=?3 AND scope_id=?4",params![org,team,input.scope.key(),input.scope_id],|r|r.get(0))?;
        if revision != input.expected_revision {
            return Err(StoreError::ControlResourceConflict);
        }
        self.conn.execute(
            "INSERT INTO capability_policy_versions VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                org,
                team,
                input.scope.key(),
                input.scope_id,
                row.revision,
                input.request_id,
                serde_json::to_string(&row).map_err(|_| StoreError::ControlResourceConflict)?
            ],
        )?;
        tx.commit()?;
        Ok(row)
    }
    /// Resolve from persisted work bindings, including dispatched plan children.
    /// A model cannot select a different team to evade these restrictions.
    pub fn work_capability_policies(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
        agent: &str,
    ) -> Result<Vec<CapabilityPolicy>> {
        let mut roster = self.work_team_for_work(actor, org, team, work)?;
        if roster.is_none() {
            if let Some(plan) = self.huddle_execution_for_work(actor, org, team, work)? {
                roster = self.work_team_for_work(actor, org, team, &plan.source_work_id)?;
            }
        }
        // Fetch at most three applicable rows, not every agent's policy per action.
        self.require_team_participant(actor, org, team)?;
        let mut query = self.conn.prepare("SELECT payload FROM capability_policy_versions v
            WHERE org_id=?1 AND team_id=?2
            AND ((scope='workspace' AND scope_id='') OR (scope='agent' AND scope_id=?3) OR (scope='team' AND scope_id=?4))
            AND revision=(SELECT MAX(revision) FROM capability_policy_versions
                WHERE org_id=v.org_id AND team_id=v.team_id AND scope=v.scope AND scope_id=v.scope_id)")?;
        let rows = query
            .query_map(
                params![org, team, agent, roster.as_ref().map(|t| t.id.as_str())],
                |r| r.get::<_, String>(0),
            )?
            .map(|r| decode(&r?))
            .collect::<Result<Vec<_>>>()?;
        Ok(rows.into_iter().filter_map(|r| r.policy).collect())
    }
}
fn decode(payload: &str) -> Result<ScopedCapabilityPolicy> {
    serde_json::from_str(payload).map_err(|_| StoreError::ControlResourceConflict)
}
