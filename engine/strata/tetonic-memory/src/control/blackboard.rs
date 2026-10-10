//! Deliberately shared, plan-scoped messages. This is not a run transcript or a scheduler.
use crate::{Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlackboardPeer {
    pub agent_id: String,
    pub name: String,
    pub work_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BlackboardKind {
    Finding,
    Question,
    Handoff,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum BlackboardEmoji {
    #[serde(rename = "👍")]
    Like,
    #[serde(rename = "❤️")]
    Heart,
    #[serde(rename = "👀")]
    Looking,
    #[serde(rename = "🎉")]
    Celebrate,
    #[serde(rename = "💡")]
    Idea,
    #[serde(rename = "🙏")]
    Thanks,
    #[serde(rename = "🤔")]
    Thinking,
    #[serde(rename = "✅")]
    Acknowledge,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlackboardReaction {
    pub emoji: BlackboardEmoji,
    pub agents: Vec<BlackboardPeer>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlackboardMessage {
    pub id: String,
    pub author: BlackboardPeer,
    pub body: String,
    pub created_at: String,
    pub reply_to: Option<String>,
    /// Older persisted messages have no reactions. Identity comes from the live attempt.
    #[serde(default)]
    pub reactions: Vec<BlackboardReaction>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlackboardThread {
    pub id: String,
    pub source_work_id: String,
    pub root_work_id: String,
    pub title: String,
    pub kind: BlackboardKind,
    pub audience: Vec<BlackboardPeer>,
    pub resolved: bool,
    pub messages: Vec<BlackboardMessage>,
    pub reply_count: usize,
    pub updated_at: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlackboardPage {
    pub threads: Vec<BlackboardThread>,
    pub has_more: bool,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlackboardQuery {
    pub thread_id: Option<String>,
    #[serde(default)]
    pub offset: usize,
    /// None means all authorized work; an empty selection matches nothing.
    pub work_ids: Option<Vec<String>>,
}
impl BlackboardQuery {
    pub fn validate(&self) -> Result<()> {
        if self.offset > 100_000
            || self.thread_id.as_ref().is_some_and(|id| !text(id, 128))
            || self
                .work_ids
                .as_ref()
                .is_some_and(|ids| ids.len() > 512 || ids.iter().any(|id| !text(id, 128)))
        {
            return Err(StoreError::InvalidControlResource(
                "Invalid Blackboard scope or page.".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum BlackboardCommand {
    Peers,
    Read {
        #[serde(default)]
        thread_id: Option<String>,
        #[serde(default)]
        offset: usize,
    },
    Post {
        audience: Vec<String>,
        title: String,
        kind: BlackboardKind,
        body: String,
    },
    Reply {
        thread_id: String,
        body: String,
        #[serde(default)]
        reply_to: Option<String>,
    },
    Resolve {
        thread_id: String,
    },
    React {
        thread_id: String,
        message_id: String,
        emoji: BlackboardEmoji,
        /// Explicit desired state; retries must never toggle a reaction.
        present: bool,
    },
}
impl BlackboardCommand {
    pub fn validate(&self) -> Result<()> {
        let valid = match self {
            Self::Peers => true,
            Self::Read { thread_id, offset } => {
                *offset <= 100_000 && thread_id.as_ref().map_or(true, |id| text(id, 128))
            }
            Self::Resolve { thread_id: id } => text(id, 128),
            Self::React {
                thread_id,
                message_id,
                ..
            } => text(thread_id, 128) && text(message_id, 128),
            Self::Post {
                audience,
                title,
                body,
                ..
            } => {
                !audience.is_empty()
                    && audience.len() <= 12
                    && audience.iter().all(|id| text(id, 128))
                    && text(title, 140)
                    && text(body, 4000)
            }
            Self::Reply {
                thread_id,
                body,
                reply_to,
            } => {
                text(thread_id, 128)
                    && text(body, 4000)
                    && reply_to.as_ref().map_or(true, |id| text(id, 128))
            }
        };
        if valid {
            Ok(())
        } else {
            Err(StoreError::InvalidControlResource(
                "Keep Blackboard messages concise and choose an existing audience.".into(),
            ))
        }
    }
}
fn text(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.len() <= max && !s.contains('\0')
}

/// Trusted host binding; never deserialized from model arguments.
pub struct BlackboardAccess<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub team: &'a str,
    pub work: &'a str,
    pub attempt: &'a str,
    pub call_id: &'a str,
    pub now: u64,
    pub lease: &'a tetonic_domain::LeaseProof,
}

impl Store {
    pub(crate) fn migrate_blackboard_v74(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS blackboard_threads (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, source_work_id TEXT NOT NULL,
            thread_id TEXT NOT NULL, payload TEXT NOT NULL, updated_at TEXT NOT NULL,
            PRIMARY KEY(org_id,team_id,thread_id),
            FOREIGN KEY(org_id,team_id,source_work_id) REFERENCES huddle_executions(org_id,team_id,source_work_id));
            CREATE INDEX IF NOT EXISTS blackboard_scope ON blackboard_threads(org_id,team_id,source_work_id,updated_at);
            CREATE TABLE IF NOT EXISTS blackboard_receipts (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, call_id TEXT NOT NULL,
            work_id TEXT NOT NULL, command TEXT NOT NULL, result TEXT NOT NULL,
            PRIMARY KEY(org_id,team_id,call_id));")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions VALUES(74,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    fn blackboard_roster(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work: &str,
    ) -> Result<(crate::HuddleExecution, Vec<BlackboardPeer>)> {
        let plan = self
            .huddle_execution_for_work(actor, org, team, work)?
            .ok_or(StoreError::ControlAccessDenied)?;
        // The first-use product has one owner. Do not silently turn team membership
        // into access to another human's collaborations when multi-user lands.
        if plan.created_by != actor {
            return Err(StoreError::ControlAccessDenied);
        }
        if !self.control_access(actor, crate::ControlPermission::ReadOrganization, org, "")? {
            return Err(StoreError::ControlAccessDenied);
        }
        let mut peers = Vec::new();
        for pin in &plan.assignments {
            let Some(agent) = self.registered_agent_revision_unchecked(
                org,
                &pin.agent_key,
                &pin.definition_digest,
            )?
            else {
                continue;
            };
            let Some(current) = self.registered_agent_unchecked(org, &pin.agent_key)? else {
                continue;
            };
            let granted = |json: &str| {
                serde_json::from_str::<serde_json::Value>(json)
                    .ok()
                    .and_then(|v| {
                        v.pointer("/configuration/requested_tools")
                            .and_then(|v| v.as_array())
                            .cloned()
                    })
                    .is_some_and(|tools| tools.iter().any(|t| t.as_str() == Some("blackboard")))
            };
            if granted(&agent.definition_json) && granted(&current.definition_json) {
                let name = serde_json::from_str::<serde_json::Value>(&agent.definition_json)
                    .ok()
                    .and_then(|v| {
                        v.pointer("/configuration/preferences/display_name")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| pin.agent_key.clone());
                peers.push(BlackboardPeer {
                    agent_id: agent.identity.identity_id,
                    name,
                    work_id: pin.work_id.clone(),
                });
            }
        }
        Ok((plan, peers))
    }
    fn blackboard_pair(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        a: &BlackboardPeer,
        b: &BlackboardPeer,
    ) -> Result<bool> {
        for (from, to) in [(a, b), (b, a)] {
            let policies =
                self.work_capability_policies(actor, org, team, &from.work_id, &from.agent_id)?;
            if policies
                .iter()
                .any(|p| !p.allows_communication(&to.agent_id))
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn blackboard_audience(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        roster: &[BlackboardPeer],
        audience: &[BlackboardPeer],
    ) -> Result<()> {
        if audience.len() < 2 || audience.iter().any(|p| !roster.contains(p)) {
            return Err(StoreError::ControlAccessDenied);
        }
        for (i, a) in audience.iter().enumerate() {
            for b in &audience[i + 1..] {
                if !self.blackboard_pair(actor, org, team, a, b)? {
                    return Err(StoreError::ControlAccessDenied);
                }
            }
        }
        Ok(())
    }
    fn blackboard_visible_to(
        &self,
        a: &BlackboardAccess<'_>,
        roster: &[BlackboardPeer],
        author: &BlackboardPeer,
        row: &BlackboardThread,
    ) -> Result<bool> {
        if !row
            .audience
            .iter()
            .any(|peer| peer.agent_id == author.agent_id)
        {
            return Ok(false);
        }
        if self
            .blackboard_audience(a.actor, a.org, a.team, roster, &row.audience)
            .is_err()
        {
            return Ok(false);
        }
        // The same identity can have another assignment with tighter team policies.
        for peer in &row.audience {
            if peer.agent_id != author.agent_id
                && !self.blackboard_pair(a.actor, a.org, a.team, author, peer)?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn blackboard_rows(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        source: Option<&str>,
        filter: &BlackboardQuery,
        visible: impl Fn(&BlackboardThread) -> Result<bool>,
    ) -> Result<BlackboardPage> {
        filter.validate()?;
        let work_ids = filter
            .work_ids
            .as_ref()
            .map(|ids| serde_json::to_string(ids).unwrap());
        let mut query=self.conn.prepare("SELECT b.payload FROM blackboard_threads b
            JOIN huddle_executions h USING(org_id,team_id,source_work_id)
            WHERE b.org_id=?1 AND b.team_id=?2 AND json_extract(h.payload,'$.created_by')=?3
            AND (?4 IS NULL OR b.source_work_id=?4) AND (?5 IS NULL OR b.thread_id=?5)
            AND (?6 IS NULL OR EXISTS (SELECT 1 FROM huddle_execution_work w
                WHERE w.org_id=b.org_id AND w.team_id=b.team_id AND w.source_work_id=b.source_work_id
                AND w.work_id IN (SELECT value FROM json_each(?6))))
            ORDER BY b.updated_at DESC,b.thread_id")?;
        let records = query.query_map(
            params![org, team, actor, source, filter.thread_id, work_ids],
            |r| r.get::<_, String>(0),
        )?;
        // Apply authorization before pagination. Hidden conversations cannot crowd out
        // permitted ones or leak their existence via the next-page indicator.
        let mut rows = Vec::new();
        let mut skipped = 0;
        for record in records {
            let row: BlackboardThread =
                serde_json::from_str(&record?).map_err(|_| StoreError::ControlResourceConflict)?;
            if !visible(&row)? {
                continue;
            }
            if skipped < filter.offset {
                skipped += 1;
                continue;
            }
            rows.push(row);
            if rows.len() == 21 {
                break;
            }
        }
        let has_more = rows.len() > 20;
        rows.truncate(20);
        if filter.thread_id.is_none() {
            for row in &mut rows {
                row.messages.truncate(1);
            }
        }
        Ok(BlackboardPage {
            threads: rows,
            has_more,
        })
    }
    /// Operator inspection. Authorization precedes pagination, including mixed-owner workspaces.
    pub fn inspect_blackboard(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        filter: &BlackboardQuery,
    ) -> Result<BlackboardPage> {
        self.require_team_participant(actor, org, team)?;
        if self
            .get_team(org, team)?
            .map_or(true, |t| t.owner_principal_id != actor)
        {
            return Err(StoreError::ControlAccessDenied);
        }
        self.blackboard_rows(actor, org, team, None, filter, |_| Ok(true))
    }
    pub fn use_blackboard(
        &self,
        a: BlackboardAccess<'_>,
        command: BlackboardCommand,
    ) -> Result<serde_json::Value> {
        command.validate()?;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.live_work_execution_deadline(a.org, a.team, a.work, a.attempt, a.now)?;
        let work = self
            .get_team_work_item(a.org, a.team, a.work)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let run = self
            .load_run_snapshot(
                work.run_id
                    .as_deref()
                    .ok_or(StoreError::ControlAccessDenied)?,
            )?
            .ok_or(StoreError::ControlAccessDenied)?;
        let attempt = run
            .attempts
            .get(&tetonic_domain::AttemptId::new(a.attempt))
            .ok_or(StoreError::ControlAccessDenied)?;
        let task = run
            .tasks
            .get(&attempt.task_id)
            .ok_or(StoreError::ControlAccessDenied)?;
        let scope = task
            .binding
            .execution_scope
            .as_ref()
            .ok_or(StoreError::ControlAccessDenied)?;
        if task.active_attempt.as_ref() != Some(&attempt.attempt_id)
            || task.binding.task_definition_version != attempt.task_version
            || !attempt.execution_claimed
            || attempt.execution_quiesced
            || attempt.suspension.is_some()
            || run.cancellation.session_canceled
            || scope.organization_id != a.org
            || scope.principal_id != a.actor
            || !self.context_access_in_organization(
                a.actor,
                &scope.information_context_id,
                a.org,
            )?
            || !attempt.lease.as_ref().is_some_and(|l| {
                l.attempt_id == attempt.attempt_id
                    && l.expires_at > a.now
                    && l.lease_id == a.lease.lease_id
                    && l.lease_epoch == a.lease.lease_epoch
                    && l.holder == a.lease.holder
            })
        {
            return Err(StoreError::ControlAccessDenied);
        }
        let job = task
            .binding
            .job_spec
            .as_ref()
            .ok_or(StoreError::ControlAccessDenied)?;
        let agent = job.identity_id.0.as_str();
        self.work_human_stop_binding(a.org, a.team, a.work, agent)?;
        let (plan, roster) = self.blackboard_roster(a.actor, a.org, a.team, a.work)?;
        if !plan
            .assignments
            .iter()
            .any(|pin| pin.work_id == a.work && pin.definition_digest == job.definition_digest)
        {
            return Err(StoreError::ControlAccessDenied);
        }
        let author = roster
            .iter()
            .find(|p| p.work_id == a.work && p.agent_id == agent)
            .ok_or(StoreError::ControlAccessDenied)?;
        let result = match &command {
            BlackboardCommand::Peers => {
                let mut seen = std::collections::HashSet::new();
                let peers = roster
                    .iter()
                    .filter(|p| p.agent_id != agent)
                    .map(|p| Ok((p, self.blackboard_pair(a.actor, a.org, a.team, author, p)?)))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .filter_map(|(p, allowed)| {
                        (allowed && seen.insert(p.agent_id.clone())).then_some(p)
                    })
                    .collect::<Vec<_>>();
                serde_json::json!({"peers":peers,"delivery":"Asynchronous. Posting never waits, interrupts, dispatches or creates a dependency. Only active agents checking the board see new messages; inactive agents are not woken."})
            }
            BlackboardCommand::Read { thread_id, offset } => {
                let filter = BlackboardQuery {
                    thread_id: thread_id.clone(),
                    offset: *offset,
                    work_ids: None,
                };
                let page = self.blackboard_rows(
                    a.actor,
                    a.org,
                    a.team,
                    Some(&plan.source_work_id),
                    &filter,
                    |t| self.blackboard_visible_to(&a, &roster, author, t),
                )?;
                serde_json::to_value(page).unwrap()
            }
            _ => self.write_blackboard(&a, command.clone(), &plan, &roster, author)?,
        };
        tx.commit()?;
        Ok(result)
    }
    fn write_blackboard(
        &self,
        a: &BlackboardAccess<'_>,
        command: BlackboardCommand,
        plan: &crate::HuddleExecution,
        roster: &[BlackboardPeer],
        author: &BlackboardPeer,
    ) -> Result<serde_json::Value> {
        let call = format!("{}:{}", a.attempt, a.call_id);
        let encoded = serde_json::to_string(&command).unwrap();
        if !text(&call, 512) {
            return Err(StoreError::ControlResourceConflict);
        }
        let existing:Option<(String,String)>=self.conn.query_row("SELECT command,result FROM blackboard_receipts WHERE org_id=?1 AND team_id=?2 AND call_id=?3",params![a.org,a.team,call],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        // Recheck the current audience even for an idempotent retry.
        let mut row = match &command {
            BlackboardCommand::Post {
                audience,
                title,
                kind,
                body,
            } => {
                let mut people = vec![author.clone()];
                for id in audience {
                    if id == &author.agent_id || people.iter().any(|p| p.agent_id == *id) {
                        continue;
                    }
                    // One stable identity may own several assignments. Resolve to its pinned
                    // assignment; callers never provide work or impersonated author IDs.
                    let mut chosen = None;
                    for peer in roster.iter().filter(|p| p.agent_id == *id) {
                        if self.blackboard_pair(a.actor, a.org, a.team, author, peer)? {
                            chosen = Some(peer.clone());
                            break;
                        }
                    }
                    people.push(chosen.ok_or(StoreError::ControlAccessDenied)?);
                }
                BlackboardThread {
                    id: uuid::Uuid::new_v4().to_string(),
                    source_work_id: plan.source_work_id.clone(),
                    root_work_id: plan.root_work_id.clone(),
                    title: title.clone(),
                    kind: kind.clone(),
                    audience: people,
                    resolved: false,
                    messages: vec![BlackboardMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        author: author.clone(),
                        body: body.clone(),
                        created_at: crate::util::now(),
                        reply_to: None,
                        reactions: Vec::new(),
                    }],
                    reply_count: 0,
                    updated_at: crate::util::now(),
                }
            }
            BlackboardCommand::Reply { thread_id, .. }
            | BlackboardCommand::React { thread_id, .. }
            | BlackboardCommand::Resolve { thread_id } => {
                let filter = BlackboardQuery {
                    thread_id: Some(thread_id.clone()),
                    ..Default::default()
                };
                self.blackboard_rows(
                    a.actor,
                    a.org,
                    a.team,
                    Some(&plan.source_work_id),
                    &filter,
                    |_| Ok(true),
                )?
                .threads
                .into_iter()
                .next()
                .ok_or(StoreError::ControlAccessDenied)?
            }
            _ => return Err(StoreError::ControlResourceConflict),
        };
        if !self.blackboard_visible_to(a, roster, author, &row)? {
            return Err(StoreError::ControlAccessDenied);
        }
        if let Some((old, result)) = existing {
            if old != encoded {
                return Err(StoreError::ControlResourceConflict);
            }
            return serde_json::from_str(&result).map_err(|_| StoreError::ControlResourceConflict);
        }
        let count:i64=self.conn.query_row("SELECT COUNT(*) FROM blackboard_receipts WHERE org_id=?1 AND team_id=?2 AND work_id=?3",params![a.org,a.team,a.work],|r|r.get(0))?;
        if count >= 32 {
            return Err(StoreError::InvalidControlResource(
                "Blackboard contribution limit reached for this assignment; continue useful work."
                    .into(),
            ));
        }
        let is_reaction = matches!(command, BlackboardCommand::React { .. });
        match command {
            BlackboardCommand::Reply { body, reply_to, .. } => {
                if row.resolved
                    || row.messages.len() >= 32
                    || reply_to
                        .as_ref()
                        .is_some_and(|id| !row.messages.iter().any(|m| m.id == *id))
                {
                    return Err(StoreError::ControlResourceConflict);
                }
                row.messages.push(BlackboardMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    author: author.clone(),
                    body,
                    created_at: crate::util::now(),
                    reply_to,
                    reactions: Vec::new(),
                });
                row.reply_count = row.messages.len() - 1;
            }
            BlackboardCommand::Resolve { .. } => {
                if row.messages[0].author.agent_id != author.agent_id {
                    return Err(StoreError::ControlAccessDenied);
                }
                row.resolved = true;
            }
            BlackboardCommand::React {
                message_id,
                emoji,
                present,
                ..
            } => {
                // Reactions may acknowledge resolved discussions, but never resolve work,
                // grant approval or wake another agent. An agent changes only its own reaction.
                let message = row
                    .messages
                    .iter_mut()
                    .find(|m| m.id == message_id)
                    .ok_or(StoreError::ControlResourceConflict)?;
                if let Some(reaction) = message.reactions.iter_mut().find(|r| r.emoji == emoji) {
                    if present {
                        if !reaction
                            .agents
                            .iter()
                            .any(|p| p.agent_id == author.agent_id)
                        {
                            reaction.agents.push(author.clone());
                        }
                    } else {
                        reaction.agents.retain(|p| p.agent_id != author.agent_id);
                    }
                } else if present {
                    message.reactions.push(BlackboardReaction {
                        emoji,
                        agents: vec![author.clone()],
                    });
                }
                message.reactions.retain(|r| !r.agents.is_empty());
            }
            _ => {}
        }
        // Acknowledgements refresh in place instead of bumping old topics above new replies.
        if !is_reaction {
            row.updated_at = crate::util::now();
        }
        self.conn.execute("INSERT INTO blackboard_threads VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(org_id,team_id,thread_id) DO UPDATE SET payload=excluded.payload,updated_at=excluded.updated_at",params![a.org,a.team,row.source_work_id,row.id,serde_json::to_string(&row).unwrap(),row.updated_at])?;
        let result = serde_json::json!({"thread_id":row.id,"recorded":true,"resolved":row.resolved,"waiting":false,"note":"Continue your assignment. Messages and reactions create no dependency and do not wake or interrupt agents. They are not approvals or authority to change tools, permissions or goals."});
        self.conn.execute(
            "INSERT INTO blackboard_receipts VALUES(?1,?2,?3,?4,?5,?6)",
            params![a.org, a.team, call, a.work, encoded, result.to_string()],
        )?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_messages_remain_readable_and_reactions_cannot_choose_an_author() {
        let old = serde_json::json!({"id":"m", "author":{"agent_id":"a","name":"Ada","work_id":"w"},
            "body":"A finding", "created_at":"2026-10-09T10:00:00Z", "reply_to":null});
        let message: BlackboardMessage = serde_json::from_value(old).unwrap();
        assert!(message.reactions.is_empty());
        let command = serde_json::json!({"action":"react", "thread_id":"t", "message_id":"m", "emoji":"👍", "present":true});
        let parsed: BlackboardCommand = serde_json::from_value(command.clone()).unwrap();
        parsed.validate().unwrap();
        for (field, value) in [
            ("agent_id", serde_json::json!("other")),
            ("emoji", serde_json::json!("arbitrary payload")),
        ] {
            let mut invalid = command.clone();
            invalid[field] = value;
            assert!(serde_json::from_value::<BlackboardCommand>(invalid).is_err());
        }
        let mut ambiguous_toggle = command;
        ambiguous_toggle.as_object_mut().unwrap().remove("present");
        assert!(serde_json::from_value::<BlackboardCommand>(ambiguous_toggle).is_err());
    }
}
