//! Durable coordinator tool receipts. These neither admit nor resume execution.
use super::*;
use tetonic_domain::{AttemptId, LeaseProof, RunSnapshot};

const MAX_CALLS: i64 = 4096;
const MAX_RESULT_BYTES: usize = 2 * 1024 * 1024;
const MAX_RECEIPT_BYTES: i64 = 16 * 1024 * 1024;

pub struct HuddleDispatchCommand<'a> {
    pub actor: &'a str,
    pub org: &'a str,
    pub team: &'a str,
    pub source: &'a str,
    pub run: &'a str,
    pub attempt: &'a str,
    pub lease: &'a LeaseProof,
    pub call: &'a str,
    pub keys: &'a [String],
    pub grouped: bool,
    pub now: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HuddleDispatchResult {
    pub ok: bool,
    pub summary: String,
    pub content: String,
    pub error_kind: Option<String>,
    /// Contributions contained in this exact response, not every completed task.
    pub delivered_keys: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HuddleDispatchReceipt {
    pub run_id: String,
    pub attempt_id: String,
    pub call_id: String,
    pub keys: Vec<String>,
    pub grouped: bool,
    pub lease: LeaseProof,
    pub stop_binding: String,
    /// Protected harness state. Payload is never stored in this shared receipt.
    #[serde(default)]
    pub checkpoint: Option<tetonic_domain::ArtifactRef>,
    pub result: Option<HuddleDispatchResult>,
}

impl Store {
    pub(crate) fn migrate_huddle_dispatch_v71(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS huddle_dispatch_receipts (
            org_id TEXT NOT NULL, team_id TEXT NOT NULL, source_work_id TEXT NOT NULL,
            attempt_id TEXT NOT NULL, call_id TEXT NOT NULL, payload TEXT NOT NULL,
            PRIMARY KEY(org_id,team_id,source_work_id,attempt_id,call_id),
            FOREIGN KEY(org_id,team_id,source_work_id)
                REFERENCES huddle_executions(org_id,team_id,source_work_id)
        );",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_versions VALUES(71,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    /// Historical records for inspection/reconstruction, never execution authority.
    pub fn huddle_dispatch_receipts(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        source: &str,
    ) -> Result<Vec<HuddleDispatchReceipt>> {
        self.huddle_execution(actor, org, team, source)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let mut query = self.conn.prepare("SELECT payload FROM huddle_dispatch_receipts WHERE org_id=?1 AND team_id=?2 AND source_work_id=?3 ORDER BY rowid")?;
        let rows = query
            .query_map(params![org, team, source], |r| r.get::<_, String>(0))?
            .map(|row| decode(&row?))
            .collect();
        rows
    }

    /// Save selection before dispatch. A retry must name the identical ordered
    /// keys and tool shape. Current runtime ownership is required even for replay.
    pub fn accept_huddle_dispatch(
        &self,
        command: HuddleDispatchCommand<'_>,
    ) -> Result<HuddleDispatchReceipt> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let (execution, _, stop_binding) = self.require_dispatch_owner(&command)?;
        if command.call.is_empty()
            || command.call.len() > 512
            || command.call.contains('\0')
            || command.keys.is_empty()
            || command.keys.len() > 12
            || (!command.grouped && command.keys.len() != 1)
            || command
                .keys
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != command.keys.len()
            || command.keys.iter().any(|key| {
                !execution
                    .assignments
                    .iter()
                    .any(|a| &a.assignment_key == key)
            })
        {
            return Err(StoreError::ControlResourceConflict);
        }
        let row = match self.dispatch_receipt(&command)? {
            Some(mut old) => {
                require_same_request(&command, &old, &stop_binding)?;
                // A later managed incarnation may own a still-pending call. The
                // journal's current lease, not this receipt, grants that ownership.
                old.lease = command.lease.clone();
                old
            }
            None => {
                let count: i64 = self.conn.query_row("SELECT count(*) FROM huddle_dispatch_receipts WHERE org_id=?1 AND team_id=?2 AND source_work_id=?3",params![command.org,command.team,command.source],|r|r.get(0))?;
                if count >= MAX_CALLS {
                    return Err(StoreError::ControlResourceConflict);
                }
                HuddleDispatchReceipt {
                    run_id: command.run.into(),
                    attempt_id: command.attempt.into(),
                    call_id: command.call.into(),
                    keys: command.keys.into(),
                    grouped: command.grouped,
                    lease: command.lease.clone(),
                    stop_binding,
                    checkpoint: None,
                    result: None,
                }
            }
        };
        self.write_dispatch_receipt(&command, &row)?;
        tx.commit()?;
        Ok(row)
    }

    /// Bind the sealed pre-dispatch checkpoint before child admission. Retry is
    /// exact; a historical response cannot be retrofitted with guessed state.
    pub fn bind_huddle_dispatch_checkpoint(
        &self,
        command: HuddleDispatchCommand<'_>,
        checkpoint: tetonic_domain::ArtifactRef,
    ) -> Result<HuddleDispatchReceipt> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let (_, _, stop) = self.require_dispatch_owner(&command)?;
        let mut row = self
            .dispatch_receipt(&command)?
            .ok_or(StoreError::ControlResourceConflict)?;
        require_same_request(&command, &row, &stop)?;
        if row.lease != *command.lease
            || checkpoint.artifact_id.is_empty()
            || checkpoint.artifact_id.len() > 256
            || checkpoint.digest.len() != 71
            || !checkpoint.digest.starts_with("sha256:")
            || !checkpoint.digest[7..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(StoreError::ControlResourceConflict);
        }
        match &row.checkpoint {
            Some(old) if old != &checkpoint => return Err(StoreError::ControlResourceConflict),
            None if row.result.is_some() => return Err(StoreError::ControlResourceConflict),
            _ => {}
        }
        row.checkpoint = Some(checkpoint);
        self.write_dispatch_receipt(&command, &row)?;
        tx.commit()?;
        Ok(row)
    }

    /// Persist the exact response before it is delivered. A stale owner cannot
    /// overwrite it, and uncertain completion is resolved by reading this receipt.
    pub fn complete_huddle_dispatch(
        &self,
        command: HuddleDispatchCommand<'_>,
        result: HuddleDispatchResult,
    ) -> Result<HuddleDispatchReceipt> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let (execution, run, stop_binding) = self.require_dispatch_owner(&command)?;
        let mut row = self
            .dispatch_receipt(&command)?
            .ok_or(StoreError::ControlResourceConflict)?;
        require_same_request(&command, &row, &stop_binding)?;
        if row.lease != *command.lease || row.checkpoint.is_none() {
            return Err(StoreError::ControlAccessDenied);
        }
        if let Some(old) = &row.result {
            if *old != result {
                return Err(StoreError::ControlResourceConflict);
            }
        } else {
            let bytes =
                serde_json::to_vec(&result).map_err(|_| StoreError::ControlResourceConflict)?;
            if bytes.len() > MAX_RESULT_BYTES
                || result.delivered_keys.len() > 12
                || result
                    .delivered_keys
                    .iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    != result.delivered_keys.len()
            {
                return Err(StoreError::ControlResourceConflict);
            }
            for key in &result.delivered_keys {
                let pin = execution
                    .assignments
                    .iter()
                    .find(|a| &a.assignment_key == key)
                    .ok_or(StoreError::ControlResourceConflict)?;
                let work = self
                    .get_team_work_item(command.org, command.team, &pin.work_id)?
                    .ok_or(StoreError::ControlAccessDenied)?;
                if super::progress::assignment_state(&work, Some(&run), pin, Some(command.attempt))
                    != AssignmentState::Completed
                {
                    return Err(StoreError::ControlAccessDenied);
                }
            }
            row.result = Some(result);
            self.write_dispatch_receipt(&command, &row)?;
        }
        tx.commit()?;
        Ok(row)
    }

    fn dispatch_receipt(
        &self,
        c: &HuddleDispatchCommand<'_>,
    ) -> Result<Option<HuddleDispatchReceipt>> {
        let json: Option<String> = self.conn.query_row("SELECT payload FROM huddle_dispatch_receipts WHERE org_id=?1 AND team_id=?2 AND source_work_id=?3 AND attempt_id=?4 AND call_id=?5",params![c.org,c.team,c.source,c.attempt,c.call],|r|r.get(0)).optional()?;
        json.map(|value| decode(&value)).transpose()
    }

    fn write_dispatch_receipt(
        &self,
        c: &HuddleDispatchCommand<'_>,
        row: &HuddleDispatchReceipt,
    ) -> Result<()> {
        let json = serde_json::to_string(row).map_err(|_| StoreError::ControlResourceConflict)?;
        let others: i64 = self.conn.query_row("SELECT COALESCE(sum(length(CAST(payload AS BLOB))),0) FROM huddle_dispatch_receipts WHERE org_id=?1 AND team_id=?2 AND source_work_id=?3 AND NOT(attempt_id=?4 AND call_id=?5)",params![c.org,c.team,c.source,c.attempt,c.call],|r|r.get(0))?;
        if others + json.len() as i64 > MAX_RECEIPT_BYTES {
            return Err(StoreError::ControlResourceConflict);
        }
        self.conn.execute("INSERT INTO huddle_dispatch_receipts VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(org_id,team_id,source_work_id,attempt_id,call_id) DO UPDATE SET payload=excluded.payload",params![c.org,c.team,c.source,c.attempt,c.call,json])?;
        Ok(())
    }

    fn require_dispatch_owner(
        &self,
        c: &HuddleDispatchCommand<'_>,
    ) -> Result<(HuddleExecution, RunSnapshot, String)> {
        let execution = self
            .huddle_execution(c.actor, c.org, c.team, c.source)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let root = self
            .get_team_work_item(c.org, c.team, &execution.root_work_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if execution.start_error.is_some() || root.run_id.as_deref() != Some(c.run) {
            return Err(StoreError::ControlAccessDenied);
        }
        self.live_work_execution_deadline(
            c.org,
            c.team,
            &execution.root_work_id,
            c.attempt,
            c.now,
        )?;
        let run = self
            .load_run_snapshot(c.run)?
            .ok_or(StoreError::ControlAccessDenied)?;
        let attempt = run
            .attempts
            .get(&AttemptId::new(c.attempt))
            .ok_or(StoreError::ControlAccessDenied)?;
        let task = run
            .tasks
            .get(&attempt.task_id)
            .ok_or(StoreError::ControlAccessDenied)?;
        let job = task
            .binding
            .job_spec
            .as_ref()
            .ok_or(StoreError::ControlAccessDenied)?;
        let scope = task
            .binding
            .execution_scope
            .as_ref()
            .ok_or(StoreError::ControlAccessDenied)?;
        if task.active_attempt.as_ref() != Some(&attempt.attempt_id)
            || task.binding.task_definition_version != attempt.task_version
            || task.binding.delegation.is_some()
            || !attempt.execution_claimed
            || attempt.execution_quiesced
            || attempt.suspension.is_some()
            || job.definition_digest != execution.coordinator_digest
            || scope.organization_id != c.org
            || scope.principal_id != c.actor
            || run.cancellation.session_canceled
            || !self.context_access_in_organization(
                c.actor,
                &scope.information_context_id,
                c.org,
            )?
            || !attempt.lease.as_ref().is_some_and(|lease| {
                lease.attempt_id == attempt.attempt_id
                    && lease.lease_id == c.lease.lease_id
                    && lease.lease_epoch == c.lease.lease_epoch
                    && lease.holder == c.lease.holder
                    && lease.expires_at > c.now
            })
        {
            return Err(StoreError::ControlAccessDenied);
        }
        let stop = self.work_human_stop_binding(
            c.org,
            c.team,
            &execution.root_work_id,
            &job.identity_id.0,
        )?;
        Ok((execution, run, stop))
    }
}

fn decode(json: &str) -> Result<HuddleDispatchReceipt> {
    serde_json::from_str(json).map_err(|_| StoreError::ControlResourceConflict)
}

fn require_same_request(
    c: &HuddleDispatchCommand<'_>,
    old: &HuddleDispatchReceipt,
    stop: &str,
) -> Result<()> {
    if old.run_id != c.run
        || old.attempt_id != c.attempt
        || old.call_id != c.call
        || old.keys != c.keys
        || old.grouped != c.grouped
        || old.stop_binding != stop
    {
        return Err(StoreError::ControlResourceConflict);
    }
    Ok(())
}
