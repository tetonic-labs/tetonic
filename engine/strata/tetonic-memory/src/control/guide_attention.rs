//! Durable attention receipts. This is a bounded inbox/admission ledger, not an
//! execution scheduler. Existing managed work owns every assessment's lifecycle.
use crate::{Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GuideAttentionSettings {
    pub revision: i64,
    pub enabled: bool,
    pub daily_tokens: i64,
    pub assessment_tokens: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuideSignal {
    pub key: String,
    pub work_id: String,
    pub signature: String,
    pub kind: String,
    pub title: String,
    pub detail: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuideAttentionItem {
    pub signal: GuideSignal,
    pub assessment_id: Option<String>,
    pub state: String,
    pub update: Option<String>,
    pub observed_at: String,
    pub seen: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct GuideAttentionView {
    pub observed_at: Option<String>,
    pub settings: GuideAttentionSettings,
    pub reserved_today: i64,
    pub items: Vec<GuideAttentionItem>,
    pub partial: bool,
}

impl Store {
    pub(crate) fn migrate_guide_attention_v78(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS guide_attention_observation (org_id TEXT NOT NULL,team_id TEXT NOT NULL,actor TEXT NOT NULL,observed_at TEXT NOT NULL,PRIMARY KEY(org_id,team_id,actor));
            CREATE TABLE IF NOT EXISTS guide_attention_settings (
            org_id TEXT NOT NULL,team_id TEXT NOT NULL,actor TEXT NOT NULL,
            revision INTEGER NOT NULL,request_id TEXT NOT NULL,previous_revision INTEGER NOT NULL,
            enabled INTEGER NOT NULL,daily_tokens INTEGER NOT NULL,assessment_tokens INTEGER NOT NULL,
            PRIMARY KEY(org_id,team_id,actor,revision),UNIQUE(org_id,team_id,actor,request_id));
            CREATE TABLE IF NOT EXISTS guide_attention (
            org_id TEXT NOT NULL,team_id TEXT NOT NULL,actor TEXT NOT NULL,event_key TEXT NOT NULL,
            work_id TEXT NOT NULL,signature TEXT NOT NULL,signal_json TEXT NOT NULL,
            assessment_id TEXT, state TEXT NOT NULL,update_text TEXT,observed_at TEXT NOT NULL,seen INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY(org_id,team_id,actor,event_key));
            CREATE TABLE IF NOT EXISTS guide_attention_admissions (
            org_id TEXT NOT NULL,team_id TEXT NOT NULL,actor TEXT NOT NULL,assessment_id TEXT NOT NULL,
            event_key TEXT NOT NULL,signature TEXT NOT NULL,day TEXT NOT NULL,allowance INTEGER NOT NULL,active INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY(org_id,team_id,actor,assessment_id),UNIQUE(org_id,team_id,actor,event_key,signature));")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions VALUES(78,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    pub fn guide_attention_settings(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<GuideAttentionSettings> {
        self.require_team_participant(actor, org, team)?;
        Ok(self.conn.query_row("SELECT revision,enabled,daily_tokens,assessment_tokens FROM guide_attention_settings WHERE org_id=?1 AND team_id=?2 AND actor=?3 ORDER BY revision DESC LIMIT 1",params![org,team,actor],|r|Ok(GuideAttentionSettings{revision:r.get(0)?,enabled:r.get(1)?,daily_tokens:r.get(2)?,assessment_tokens:r.get(3)?})).optional()?.unwrap_or_default())
    }

    pub fn save_guide_attention_settings(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        request: &str,
        settings: GuideAttentionSettings,
    ) -> Result<GuideAttentionSettings> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        if uuid::Uuid::parse_str(request).is_err()
            || settings.revision < 0
            || !(0..=200_000).contains(&settings.daily_tokens)
            || !(0..=32_000).contains(&settings.assessment_tokens)
            || settings.enabled
                && (settings.assessment_tokens < 1024
                    || settings.daily_tokens < settings.assessment_tokens)
        {
            return Err(StoreError::InvalidControlResource(
                "guide attention allowance".into(),
            ));
        }
        let retry:Option<(i64,GuideAttentionSettings)>=self.conn.query_row("SELECT previous_revision,revision,enabled,daily_tokens,assessment_tokens FROM guide_attention_settings WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND request_id=?4",params![org,team,actor,request],|r|Ok((r.get(0)?,GuideAttentionSettings{revision:r.get(1)?,enabled:r.get(2)?,daily_tokens:r.get(3)?,assessment_tokens:r.get(4)?}))).optional()?;
        if let Some((expected, old)) = retry {
            if expected != settings.revision
                || old.enabled != settings.enabled
                || old.daily_tokens != settings.daily_tokens
                || old.assessment_tokens != settings.assessment_tokens
            {
                return Err(StoreError::ControlResourceConflict);
            }
            return Ok(old);
        }
        if self.guide_attention_settings(actor, org, team)?.revision != settings.revision {
            return Err(StoreError::ControlResourceConflict);
        }
        let revision = settings
            .revision
            .checked_add(1)
            .ok_or(StoreError::ControlResourceConflict)?;
        self.conn.execute(
            "INSERT INTO guide_attention_settings VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                org,
                team,
                actor,
                revision,
                request,
                settings.revision,
                settings.enabled,
                settings.daily_tokens,
                settings.assessment_tokens
            ],
        )?;
        tx.commit()?;
        Ok(GuideAttentionSettings {
            revision,
            ..settings
        })
    }

    /// Reconciliation is authoritative: absent signals resolve instead of
    /// retaining stale alarms. Repeated same-incident evidence does not requeue.
    pub fn observe_guide_signals(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        signals: Vec<GuideSignal>,
    ) -> Result<()> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        let keys: Vec<_> = signals.iter().map(|s| s.key.clone()).collect();
        let mut stmt=self.conn.prepare("SELECT event_key FROM guide_attention WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND state!='resolved'")?;
        let old = stmt
            .query_map(params![org, team, actor], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        for key in old.into_iter().filter(|key| !keys.contains(key)) {
            self.conn.execute("UPDATE guide_attention SET state='resolved' WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND event_key=?4",params![org,team,actor,key])?;
        }
        for signal in signals {
            let item = self
                .get_team_work_item(org, team, &signal.work_id)?
                .ok_or(StoreError::ControlAccessDenied)?;
            if item.purpose == crate::WorkPurpose::Explore
                || item.created_by != actor
                || signal.key.len() > 256
                || signal.detail.len() > 2000
                || signal.title.len() > 400
            {
                return Err(StoreError::ControlAccessDenied);
            }
            let json =
                serde_json::to_string(&signal).map_err(|_| StoreError::ControlResourceConflict)?;
            self.conn.execute("INSERT INTO guide_attention(org_id,team_id,actor,event_key,work_id,signature,signal_json,state,observed_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'pending',?8)
                ON CONFLICT(org_id,team_id,actor,event_key) DO UPDATE SET work_id=excluded.work_id,signature=excluded.signature,signal_json=excluded.signal_json,state=CASE WHEN guide_attention.signature=excluded.signature THEN 'unavailable' ELSE 'pending' END,assessment_id=NULL,update_text=NULL,observed_at=excluded.observed_at,seen=0 WHERE guide_attention.signature!=excluded.signature OR guide_attention.state='resolved'",params![org,team,actor,signal.key,signal.work_id,signal.signature,json,crate::util::now()])?;
            self.conn.execute("UPDATE guide_attention SET work_id=?5,signal_json=?6 WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND event_key=?4 AND signature=?7",params![org,team,actor,signal.key,signal.work_id,json,signal.signature])?;
        }
        // Keep evidence inspectable but bound the model backlog. Older overflow
        // remains an unassessed recorded change; it is never silently completed.
        self.conn.execute("UPDATE guide_attention SET state='unavailable' WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND event_key IN (SELECT event_key FROM guide_attention WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND state='pending' ORDER BY observed_at DESC,event_key LIMIT -1 OFFSET 256)",params![org,team,actor])?;
        self.conn.execute("INSERT INTO guide_attention_observation VALUES(?1,?2,?3,?4) ON CONFLICT(org_id,team_id,actor) DO UPDATE SET observed_at=excluded.observed_at",params![org,team,actor,crate::util::now()])?;
        tx.commit()?;
        Ok(())
    }

    pub fn guide_attention_view(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<GuideAttentionView> {
        let settings = self.guide_attention_settings(actor, org, team)?;
        let observed_at=self.conn.query_row("SELECT observed_at FROM guide_attention_observation WHERE org_id=?1 AND team_id=?2 AND actor=?3",params![org,team,actor],|r|r.get(0)).optional()?;
        let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let reserved_today=self.conn.query_row("SELECT COALESCE(SUM(allowance),0) FROM guide_attention_admissions WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND day=?4",params![org,team,actor,day],|r|r.get(0))?;
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM guide_attention WHERE org_id=?1 AND team_id=?2 AND actor=?3",
            params![org, team, actor],
            |r| r.get(0),
        )?;
        let mut stmt=self.conn.prepare("SELECT signal_json,assessment_id,state,update_text,observed_at,seen FROM guide_attention WHERE org_id=?1 AND team_id=?2 AND actor=?3 ORDER BY CASE state WHEN 'assessing' THEN 0 WHEN 'pending' THEN 1 ELSE 2 END,seen,observed_at DESC,event_key LIMIT 100")?;
        let rows = stmt
            .query_map(params![org, team, actor], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut items = vec![];
        for (json, assessment_id, state, update, observed_at, seen) in rows {
            let signal: GuideSignal =
                serde_json::from_str(&json).map_err(|_| StoreError::ControlResourceConflict)?;
            if self
                .get_team_work_item(org, team, &signal.work_id)?
                .is_none()
            {
                continue;
            }
            items.push(GuideAttentionItem {
                signal,
                assessment_id,
                state,
                update,
                observed_at,
                seen,
            });
        }
        Ok(GuideAttentionView {
            observed_at,
            settings,
            reserved_today,
            items,
            partial: count > 100,
        })
    }

    pub fn claim_guide_attention(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<Option<GuideAttentionItem>> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let view = self.guide_attention_view(actor, org, team)?;
        let settings = &view.settings;
        let outstanding:bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM guide_attention_admissions WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND active=1)",params![org,team,actor],|r|r.get(0))?;
        if !settings.enabled
            || settings.assessment_tokens <= 0
            || view
                .reserved_today
                .saturating_add(settings.assessment_tokens)
                > settings.daily_tokens
            || outstanding
        {
            return Ok(None);
        }
        let Some(mut item) = view.items.into_iter().find(|i| i.state == "pending") else {
            return Ok(None);
        };
        let id = uuid::Uuid::new_v4().to_string();
        let inserted = self.conn.execute(
            "INSERT OR IGNORE INTO guide_attention_admissions VALUES(?1,?2,?3,?4,?5,?6,?7,?8,1)",
            params![
                org,
                team,
                actor,
                id,
                item.signal.key,
                item.signal.signature,
                chrono::Utc::now().format("%Y-%m-%d").to_string(),
                settings.assessment_tokens
            ],
        )?;
        if inserted == 0 {
            self.conn.execute("UPDATE guide_attention SET state='unavailable' WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND event_key=?4",params![org,team,actor,item.signal.key])?;
            tx.commit()?;
            return Ok(None);
        }
        self.conn.execute("UPDATE guide_attention SET state='assessing',assessment_id=?5 WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND event_key=?4",params![org,team,actor,item.signal.key,id])?;
        item.assessment_id = Some(id);
        item.state = "assessing".into();
        tx.commit()?;
        Ok(Some(item))
    }

    pub fn guide_assessment_allowance(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        id: &str,
    ) -> Result<Option<i64>> {
        self.require_team_participant(actor, org, team)?;
        Ok(self.conn.query_row("SELECT allowance FROM guide_attention_admissions WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND assessment_id=?4",params![org,team,actor,id],|r|r.get(0)).optional()?)
    }

    pub fn complete_guide_attention(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        id: &str,
        update: Option<String>,
    ) -> Result<()> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        if update.as_ref().is_some_and(|s| s.len() > 12_000) {
            return Err(StoreError::InvalidControlResource(
                "attention update".into(),
            ));
        }
        self.conn.execute("UPDATE guide_attention SET state=?5,update_text=?6 WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND assessment_id=?4 AND state='assessing'",params![org,team,actor,id,if update.is_some(){"reviewed"}else{"unavailable"},update])?;
        self.conn.execute("UPDATE guide_attention_admissions SET active=0 WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND assessment_id=?4",params![org,team,actor,id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn outstanding_guide_assessments(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<Vec<String>> {
        self.require_team_participant(actor, org, team)?;
        let mut stmt=self.conn.prepare("SELECT assessment_id FROM guide_attention_admissions WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND active=1")?;
        let rows = stmt
            .query_map(params![org, team, actor], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn see_guide_attention(&self, actor: &str, org: &str, team: &str, key: &str) -> Result<()> {
        self.require_team_participant(actor, org, team)?;
        self.conn.execute("UPDATE guide_attention SET seen=1 WHERE org_id=?1 AND team_id=?2 AND actor=?3 AND event_key=?4",params![org,team,actor,key])?;
        Ok(())
    }
}
