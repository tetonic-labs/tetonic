//! Owner-private subject associations over existing messages. These records carry
//! no execution or sharing authority; originals and work ownership never move.
use crate::{Result, Store, StoreError, WorkPurpose};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GuideSubjectDraft {
    pub id: String,
    pub expected_revision: i64,
    pub title: String,
    pub understanding: String,
    pub source_turns: Vec<String>,
    pub work_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuideSubject {
    pub id: String,
    pub revision: i64,
    pub title: String,
    pub understanding: String,
    pub source_turns: Vec<String>,
    pub work_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct GuideFocus {
    pub revision: i64,
    pub subject_id: Option<String>,
}

impl Store {
    pub fn guide_referenced_source(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        id: &str,
    ) -> Result<bool> {
        self.require_guide_source(actor, org, team, id)?;
        Ok(self.conn.query_row("SELECT EXISTS(SELECT 1 FROM guide_subject_revisions s,json_each(s.value_json,'$.source_turns') source WHERE s.org_id=?1 AND s.team_id=?2 AND s.actor=?3 AND source.value=?4 AND s.revision=(SELECT MAX(latest.revision) FROM guide_subject_revisions latest WHERE latest.org_id=s.org_id AND latest.team_id=s.team_id AND latest.actor=s.actor AND latest.subject_id=s.subject_id))",params![org,team,actor,id],|r|r.get(0))?)
    }
    pub(crate) fn migrate_guide_context_v77(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS guide_subject_revisions (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, actor TEXT NOT NULL,
            subject_id TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0),
            request_id TEXT NOT NULL, draft_json TEXT NOT NULL, value_json TEXT NOT NULL,
            PRIMARY KEY(org_id,team_id,actor,subject_id,revision),
            UNIQUE(org_id,team_id,actor,request_id));
            CREATE TABLE IF NOT EXISTS guide_focus_revisions (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, actor TEXT NOT NULL,
            conversation_id TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0),
            subject_id TEXT, request_id TEXT NOT NULL, expected_revision INTEGER NOT NULL,
            PRIMARY KEY(org_id,team_id,actor,conversation_id,revision),
            UNIQUE(org_id,team_id,actor,request_id),
            FOREIGN KEY(org_id,team_id,conversation_id) REFERENCES team_work_items(org_id,team_id,work_id));")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions VALUES(77,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub fn require_guide_source(&self, actor: &str, org: &str, team: &str, id: &str) -> Result<()> {
        self.require_team_participant(actor, org, team)?;
        let item = self
            .get_team_work_item(org, team, id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if item.created_by != actor || item.purpose != WorkPurpose::Explore {
            return Err(StoreError::ControlAccessDenied);
        }
        Ok(())
    }

    pub fn guide_subject(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        id: &str,
    ) -> Result<Option<GuideSubject>> {
        self.require_team_participant(actor, org, team)?;
        let json:Option<String>=self.conn.query_row("SELECT value_json FROM guide_subject_revisions WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND subject_id=?4 ORDER BY revision DESC LIMIT 1",params![org,team,actor,id],|r|r.get(0)).optional()?;
        let value: Option<GuideSubject> = json
            .map(|json| serde_json::from_str(&json))
            .transpose()
            .map_err(|_| StoreError::ControlResourceConflict)?;
        if let Some(value) = &value {
            for source in &value.source_turns {
                self.require_guide_source(actor, org, team, source)?;
            }
            for work in &value.work_ids {
                if self.get_team_work_item(org, team, work)?.is_none() {
                    return Err(StoreError::ControlAccessDenied);
                }
            }
        }
        Ok(value)
    }

    pub fn guide_subjects(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        offset: usize,
    ) -> Result<Vec<GuideSubject>> {
        self.require_team_participant(actor, org, team)?;
        let mut stmt=self.conn.prepare("SELECT subject_id FROM guide_subject_revisions WHERE org_id=?1 AND team_id=?2 AND actor=?3 GROUP BY subject_id ORDER BY MAX(rowid) DESC LIMIT 24 OFFSET ?4")?;
        let ids = stmt
            .query_map(params![org, team, actor, offset as i64], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.iter()
            .map(|id| {
                self.guide_subject(actor, org, team, id)?
                    .ok_or(StoreError::ControlAccessDenied)
            })
            .collect()
    }

    pub fn save_guide_subject(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        request: &str,
        draft: GuideSubjectDraft,
    ) -> Result<GuideSubject> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        if uuid::Uuid::parse_str(&draft.id).is_err()
            || uuid::Uuid::parse_str(request).is_err()
            || draft.expected_revision < 0
            || draft.title.trim().is_empty()
            || draft.title.len() > 160
            || draft.understanding.len() > 6000
            || draft.source_turns.is_empty()
            || draft.source_turns.len() > 64
            || draft.work_ids.len() > 24
            || draft.title.contains('\0')
            || draft.understanding.contains('\0')
        {
            return Err(StoreError::InvalidControlResource("guide subject".into()));
        }
        for source in &draft.source_turns {
            self.require_guide_source(actor, org, team, source)?;
        }
        for work in &draft.work_ids {
            let item = self
                .get_team_work_item(org, team, work)?
                .ok_or(StoreError::ControlAccessDenied)?;
            if item.purpose == WorkPurpose::Explore {
                self.require_guide_source(actor, org, team, work)?;
            }
        }
        let json =
            serde_json::to_string(&draft).map_err(|_| StoreError::ControlResourceConflict)?;
        let retry:Option<(String,String)>=self.conn.query_row("SELECT draft_json,value_json FROM guide_subject_revisions WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND request_id=?4",params![org,team,actor,request],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((old, value)) = retry {
            if old != json {
                return Err(StoreError::ControlResourceConflict);
            }
            return serde_json::from_str(&value).map_err(|_| StoreError::ControlResourceConflict);
        }
        let old = self.guide_subject(actor, org, team, &draft.id)?;
        if old.as_ref().map_or(0, |s| s.revision) != draft.expected_revision {
            return Err(StoreError::ControlResourceConflict);
        }
        let value = GuideSubject {
            id: draft.id,
            title: draft.title,
            understanding: draft.understanding,
            source_turns: draft.source_turns,
            work_ids: draft.work_ids,
            revision: draft
                .expected_revision
                .checked_add(1)
                .ok_or(StoreError::ControlResourceConflict)?,
        };
        let encoded =
            serde_json::to_string(&value).map_err(|_| StoreError::ControlResourceConflict)?;
        self.conn.execute(
            "INSERT INTO guide_subject_revisions VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                org,
                team,
                actor,
                value.id,
                value.revision,
                request,
                json,
                encoded
            ],
        )?;
        tx.commit()?;
        Ok(value)
    }

    pub fn guide_focus(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        conversation: &str,
    ) -> Result<GuideFocus> {
        self.require_guide_source(actor, org, team, conversation)?;
        let value=self.conn.query_row("SELECT revision,subject_id FROM guide_focus_revisions WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND conversation_id=?4 ORDER BY revision DESC LIMIT 1",params![org,team,actor,conversation],|r|Ok(GuideFocus{revision:r.get(0)?,subject_id:r.get(1)?})).optional()?.unwrap_or_default();
        if let Some(id) = &value.subject_id {
            self.guide_subject(actor, org, team, id)?
                .ok_or(StoreError::ControlAccessDenied)?;
        }
        Ok(value)
    }

    pub fn set_guide_focus(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        conversation: &str,
        request: &str,
        change: GuideFocus,
    ) -> Result<GuideFocus> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_guide_source(actor, org, team, conversation)?;
        if uuid::Uuid::parse_str(request).is_err() || change.revision < 0 {
            return Err(StoreError::InvalidControlResource("guide focus".into()));
        }
        if let Some(id) = &change.subject_id {
            self.guide_subject(actor, org, team, id)?
                .ok_or(StoreError::ControlAccessDenied)?;
        }
        let retry:Option<(String,i64,i64,Option<String>)>=self.conn.query_row("SELECT conversation_id,expected_revision,revision,subject_id FROM guide_focus_revisions WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND request_id=?4",params![org,team,actor,request],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        if let Some((source, expected, revision, subject_id)) = retry {
            if source != conversation
                || expected != change.revision
                || subject_id != change.subject_id
            {
                return Err(StoreError::ControlResourceConflict);
            }
            return Ok(GuideFocus {
                revision,
                subject_id,
            });
        }
        let old = self.guide_focus(actor, org, team, conversation)?;
        if old.revision != change.revision {
            return Err(StoreError::ControlResourceConflict);
        }
        let revision = change
            .revision
            .checked_add(1)
            .ok_or(StoreError::ControlResourceConflict)?;
        self.conn.execute(
            "INSERT INTO guide_focus_revisions VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                org,
                team,
                actor,
                conversation,
                revision,
                change.subject_id,
                request,
                change.revision
            ],
        )?;
        tx.commit()?;
        Ok(GuideFocus {
            revision,
            subject_id: change.subject_id,
        })
    }
}
