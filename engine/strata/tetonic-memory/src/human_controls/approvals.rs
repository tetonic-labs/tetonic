//! Effect approval proposal, resolution, and dispatch checks.
use super::*;

impl Store {
    /// Propose an effect approval. Retries with the same request_id are idempotent.
    /// A changed proposal digest on the same request conflicts.
    pub fn propose_effect_approval(
        &self,
        command: crate::ProposeEffectApproval<'_>,
    ) -> Result<EffectApproval> {
        let crate::ProposeEffectApproval {
            actor,
            org,
            team,
            approval_id,
            proposal_digest,
            request_id,
            expires_at,
            work_id,
            proposal,
        } = command;
        validate_id(org, "org_id")?;
        validate_id(team, "team_id")?;
        validate_id(approval_id, "approval_id")?;
        validate_id(request_id, "request_id")?;
        validate_id(proposal_digest, "proposal_digest")?;
        if expires_at <= 0 {
            return Err(StoreError::InvalidControlResource("expires_at".into()));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        if let Some(p) = proposal {
            let deadline = self.human_live_deadline(
                org,
                team,
                work_id.ok_or(StoreError::ControlAccessDenied)?,
                &p.attempt_id,
                chrono::Utc::now().timestamp().max(0) as u64,
            )?;
            if p.command.trim().is_empty()
                || p.command.len() > 32_768
                || p.command.contains('\0')
                || p.digest() != proposal_digest
                || expires_at > deadline as i64
            {
                return Err(StoreError::ControlAccessDenied);
            }
        }
        if let Some(existing) = self.effect_approval_by_request(org, team, request_id)? {
            if existing.approval_id != approval_id
                || existing.proposal_digest != proposal_digest
                || existing.proposal.as_ref() != proposal
                || existing.work_id.as_deref() != work_id
            {
                return Err(StoreError::ControlResourceConflict);
            }
            tx.commit()?;
            return Ok(existing);
        }
        self.conn.execute(
            "INSERT INTO effect_approvals(
                org_id,team_id,approval_id,work_id,proposal_digest,status,request_id,
                expires_at,created_by,resolved_by,created_at,proposal_json
             ) VALUES(?1,?2,?3,?4,?5,'pending',?6,?7,?8,NULL,?9,?10)",
            params![
                org,
                team,
                approval_id,
                work_id,
                proposal_digest,
                request_id,
                expires_at,
                actor,
                crate::util::now(),
                proposal.map(|p| serde_json::to_string(p).expect("shell proposal"))
            ],
        )?;
        let row = self
            .get_effect_approval(org, team, approval_id)?
            .ok_or(StoreError::ControlResourceConflict)?;
        tx.commit()?;
        Ok(row)
    }

    fn effect_approval_by_request(
        &self,
        org: &str,
        team: &str,
        request_id: &str,
    ) -> Result<Option<EffectApproval>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,team_id,approval_id,work_id,proposal_digest,status,request_id,
                        expires_at,created_by,resolved_by,proposal_json
                 FROM effect_approvals WHERE org_id=?1 AND team_id=?2 AND request_id=?3",
                params![org, team, request_id],
                Self::map_effect_approval,
            )
            .optional()?)
    }

    pub fn get_effect_approval(
        &self,
        org: &str,
        team: &str,
        approval_id: &str,
    ) -> Result<Option<EffectApproval>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,team_id,approval_id,work_id,proposal_digest,status,request_id,
                        expires_at,created_by,resolved_by,proposal_json
                 FROM effect_approvals WHERE org_id=?1 AND team_id=?2 AND approval_id=?3",
                params![org, team, approval_id],
                Self::map_effect_approval,
            )
            .optional()?)
    }

    fn map_effect_approval(r: &rusqlite::Row<'_>) -> rusqlite::Result<EffectApproval> {
        Ok(EffectApproval {
            org_id: r.get(0)?,
            team_id: r.get(1)?,
            approval_id: r.get(2)?,
            work_id: r.get(3)?,
            proposal_digest: r.get(4)?,
            status: r.get(5)?,
            request_id: r.get(6)?,
            expires_at: r.get(7)?,
            created_by: r.get(8)?,
            resolved_by: r.get(9)?,
            proposal: r
                .get::<_, Option<String>>(10)?
                .map(|json| {
                    serde_json::from_str(&json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            10,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })
                })
                .transpose()?,
        })
    }

    /// Approve or reject. Rejected/expired approvals never authorize dispatch.
    /// A digest mismatch means the proposal changed and must be re-proposed.
    pub fn resolve_effect_approval(
        &self,
        command: crate::ResolveEffectApproval<'_>,
    ) -> Result<EffectApproval> {
        let crate::ResolveEffectApproval {
            actor,
            org,
            team,
            approval_id,
            proposal_digest,
            allow,
            now_unix,
        } = command;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        let current = self
            .get_effect_approval(org, team, approval_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        // Commands originate in the initiating person's participation context.
        // Team membership alone does not disclose or authorize that command.
        if current.proposal.is_some() && current.created_by != actor {
            return Err(StoreError::ControlAccessDenied);
        }
        if current.proposal_digest != proposal_digest {
            return Err(StoreError::ControlResourceConflict);
        }
        self.shell_approval_live(&current, now_unix)?;
        if current.status != "pending" {
            if current.status == "approved" && allow || current.status == "rejected" && !allow {
                tx.commit()?;
                return Ok(current);
            }
            return Err(StoreError::ControlResourceConflict);
        }
        if current.expires_at <= now_unix {
            self.conn.execute(
                "UPDATE effect_approvals SET status='expired', resolved_by=?4
                 WHERE org_id=?1 AND team_id=?2 AND approval_id=?3 AND status='pending'",
                params![org, team, approval_id, actor],
            )?;
            return Err(StoreError::ControlAccessDenied);
        }
        let status = if allow { "approved" } else { "rejected" };
        self.conn.execute(
            "UPDATE effect_approvals SET status=?4, resolved_by=?5
             WHERE org_id=?1 AND team_id=?2 AND approval_id=?3 AND status='pending'",
            params![org, team, approval_id, status, actor],
        )?;
        let row = self
            .get_effect_approval(org, team, approval_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        tx.commit()?;
        Ok(row)
    }

    /// Dispatch gate: only an unexpired approved digest may proceed.
    pub fn effect_approval_allows_dispatch(
        &self,
        org: &str,
        team: &str,
        approval_id: &str,
        proposal_digest: &str,
        now_unix: i64,
    ) -> Result<bool> {
        let Some(row) = self.get_effect_approval(org, team, approval_id)? else {
            return Ok(false);
        };
        if row.proposal_digest != proposal_digest {
            return Ok(false);
        }
        // Shell approvals must pass the live, single-consumption gate.
        if row.proposal.is_some() {
            return Ok(false);
        }
        if row.status == "approved" && row.expires_at > now_unix {
            return Ok(true);
        }
        if row.status == "pending" && row.expires_at <= now_unix {
            let _ = self.conn.execute(
                "UPDATE effect_approvals SET status='expired'
                 WHERE org_id=?1 AND team_id=?2 AND approval_id=?3 AND status='pending'",
                params![org, team, approval_id],
            );
        }
        Ok(false)
    }

    pub fn list_pending_effect_approvals(
        &self,
        actor: &str,
        org: &str,
        team: &str,
    ) -> Result<Vec<EffectApproval>> {
        self.require_team_participant(actor, org, team)?;
        let mut stmt = self.conn.prepare(
            "SELECT org_id,team_id,approval_id,work_id,proposal_digest,status,request_id,
                    expires_at,created_by,resolved_by,proposal_json
             FROM effect_approvals
             WHERE org_id=?1 AND team_id=?2 AND status='pending'
             ORDER BY created_at, approval_id",
        )?;
        let rows = stmt
            .query_map(params![org, team], Self::map_effect_approval)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let now = chrono::Utc::now().timestamp();
        Ok(rows
            .into_iter()
            .filter(|row| {
                row.proposal.is_none()
                    || (row.created_by == actor
                        && row.expires_at > now
                        && self.shell_approval_live(row, now).is_ok())
            })
            .collect())
    }
}
