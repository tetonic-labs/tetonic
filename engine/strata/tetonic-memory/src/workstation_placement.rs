//! Org-scoped workstations and assignment generations (MVP-501/502).
//! Device enrollment is distinct from employee credentials. Team membership
//! alone never grants workstation access. Offline/revoked devices park pinned
//! work instead of relocating it.

use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::{Result, Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Workstation {
    pub org_id: String,
    pub workstation_id: String,
    pub label: String,
    pub owner_principal_id: String,
    pub platform: String,
    pub status: String,
    pub shared_assignment: bool,
    pub assignment_generation: i64,
    pub device_credential_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WorkstationGrant {
    pub org_id: String,
    pub workstation_id: String,
    pub grant_id: String,
    pub resource_kind: String,
    pub resource_ref: String,
    pub approved_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WorkPlacementPin {
    pub org_id: String,
    pub team_id: String,
    pub work_id: String,
    pub workstation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WorkerAssignmentClaim {
    pub org_id: String,
    pub workstation_id: String,
    pub assignment_id: String,
    pub generation: i64,
    pub work_id: Option<String>,
    pub status: String,
    pub request_id: String,
}

fn validate_id(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') || value.len() > 128 {
        return Err(StoreError::InvalidControlResource(field.into()));
    }
    Ok(())
}

fn hash_secret(secret: &str) -> String {
    format!("{:x}", Sha256::digest(secret.as_bytes()))
}

impl Store {
    pub(crate) fn migrate_workstation_placement_v50(&self) -> Result<()> {
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version=50)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workstations (
                org_id TEXT NOT NULL,
                workstation_id TEXT NOT NULL,
                label TEXT NOT NULL,
                owner_principal_id TEXT NOT NULL REFERENCES control_principals(principal_id),
                platform TEXT NOT NULL,
                status TEXT NOT NULL,
                shared_assignment INTEGER NOT NULL DEFAULT 0,
                assignment_generation INTEGER NOT NULL DEFAULT 1,
                device_credential_id TEXT NOT NULL,
                device_credential_hash TEXT NOT NULL,
                created_at TEXT NOT NULL,
                revoked_at TEXT,
                PRIMARY KEY (org_id, workstation_id)
             );
             CREATE TABLE IF NOT EXISTS workstation_resource_grants (
                org_id TEXT NOT NULL,
                workstation_id TEXT NOT NULL,
                grant_id TEXT NOT NULL,
                resource_kind TEXT NOT NULL,
                resource_ref TEXT NOT NULL,
                approved_by TEXT NOT NULL REFERENCES control_principals(principal_id),
                created_at TEXT NOT NULL,
                PRIMARY KEY (org_id, workstation_id, grant_id),
                FOREIGN KEY (org_id, workstation_id) REFERENCES workstations(org_id, workstation_id)
             );
             CREATE TABLE IF NOT EXISTS work_placement_pins (
                org_id TEXT NOT NULL,
                team_id TEXT NOT NULL,
                work_id TEXT NOT NULL,
                workstation_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (org_id, team_id, work_id),
                FOREIGN KEY (org_id, team_id, work_id)
                    REFERENCES team_work_items(org_id, team_id, work_id),
                FOREIGN KEY (org_id, workstation_id) REFERENCES workstations(org_id, workstation_id)
             );
             CREATE TABLE IF NOT EXISTS worker_assignment_claims (
                org_id TEXT NOT NULL,
                workstation_id TEXT NOT NULL,
                assignment_id TEXT NOT NULL,
                generation INTEGER NOT NULL,
                work_id TEXT,
                status TEXT NOT NULL,
                request_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (org_id, workstation_id, assignment_id),
                FOREIGN KEY (org_id, workstation_id) REFERENCES workstations(org_id, workstation_id),
                UNIQUE (org_id, workstation_id, request_id)
             );
             CREATE INDEX IF NOT EXISTS idx_workstations_status
                ON workstations(org_id, status);
             CREATE INDEX IF NOT EXISTS idx_placement_pins_ws
                ON work_placement_pins(org_id, workstation_id);",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(50,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    /// Enroll a workstation under an org. Device credential is distinct from
    /// the owner employee credential. Supported platforms are explicit.
    pub fn enroll_workstation(
        &self,
        actor: &str,
        org: &str,
        workstation_id: &str,
        label: &str,
        platform: &str,
        device_secret: &str,
        shared_assignment: bool,
    ) -> Result<Workstation> {
        validate_id(org, "org_id")?;
        validate_id(workstation_id, "workstation_id")?;
        validate_id(label, "label")?;
        validate_id(device_secret, "device_secret")?;
        match platform {
            "linux" | "macos" | "windows" => {}
            _ => return Err(StoreError::InvalidControlResource("platform".into())),
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.control_access(
            actor,
            crate::ControlPermission::ManageOrganization,
            org,
            "",
        )? {
            return Err(StoreError::ControlAccessDenied);
        }
        if let Some(existing) = self.get_workstation(org, workstation_id)? {
            if existing.status == "revoked" {
                return Err(StoreError::ControlAccessDenied);
            }
            tx.commit()?;
            return Ok(existing);
        }
        let credential_id = format!("wscred/{workstation_id}");
        self.conn.execute(
            "INSERT INTO workstations(
                org_id,workstation_id,label,owner_principal_id,platform,status,
                shared_assignment,assignment_generation,device_credential_id,
                device_credential_hash,created_at,revoked_at
             ) VALUES(?1,?2,?3,?4,?5,'enrolled',?6,1,?7,?8,?9,NULL)",
            params![
                org,
                workstation_id,
                label,
                actor,
                platform,
                if shared_assignment { 1 } else { 0 },
                credential_id,
                hash_secret(device_secret),
                crate::util::now()
            ],
        )?;
        let row = self
            .get_workstation(org, workstation_id)?
            .ok_or(StoreError::ControlResourceConflict)?;
        tx.commit()?;
        Ok(row)
    }

    pub fn get_workstation(
        &self,
        org: &str,
        workstation_id: &str,
    ) -> Result<Option<Workstation>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,workstation_id,label,owner_principal_id,platform,status,
                        shared_assignment,assignment_generation,device_credential_id
                 FROM workstations WHERE org_id=?1 AND workstation_id=?2",
                params![org, workstation_id],
                |r| {
                    Ok(Workstation {
                        org_id: r.get(0)?,
                        workstation_id: r.get(1)?,
                        label: r.get(2)?,
                        owner_principal_id: r.get(3)?,
                        platform: r.get(4)?,
                        status: r.get(5)?,
                        shared_assignment: r.get::<_, i64>(6)? != 0,
                        assignment_generation: r.get(7)?,
                        device_credential_id: r.get(8)?,
                    })
                },
            )
            .optional()?)
    }

    /// Verify the device secret. Employee tokens never authenticate as devices.
    pub fn verify_workstation_device_credential(
        &self,
        org: &str,
        workstation_id: &str,
        device_secret: &str,
    ) -> Result<bool> {
        let hash: Option<String> = self
            .conn
            .query_row(
                "SELECT device_credential_hash FROM workstations
                 WHERE org_id=?1 AND workstation_id=?2 AND status IN ('enrolled','draining','offline')",
                params![org, workstation_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(hash.is_some_and(|h| h == hash_secret(device_secret)))
    }

    /// Owner-approved local resource grant. Shared assignment remains opt-in on
    /// the workstation row; grants alone do not schedule other employees' work.
    pub fn approve_workstation_grant(
        &self,
        actor: &str,
        org: &str,
        workstation_id: &str,
        grant_id: &str,
        resource_kind: &str,
        resource_ref: &str,
    ) -> Result<WorkstationGrant> {
        validate_id(grant_id, "grant_id")?;
        validate_id(resource_kind, "resource_kind")?;
        validate_id(resource_ref, "resource_ref")?;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let ws = self
            .get_workstation(org, workstation_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if ws.owner_principal_id != actor && !self.control_access(
            actor,
            crate::ControlPermission::ManageOrganization,
            org,
            "",
        )? {
            return Err(StoreError::ControlAccessDenied);
        }
        if ws.status == "revoked" {
            return Err(StoreError::ControlAccessDenied);
        }
        self.conn.execute(
            "INSERT INTO workstation_resource_grants(
                org_id,workstation_id,grant_id,resource_kind,resource_ref,approved_by,created_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(org_id,workstation_id,grant_id) DO NOTHING",
            params![
                org,
                workstation_id,
                grant_id,
                resource_kind,
                resource_ref,
                actor,
                crate::util::now()
            ],
        )?;
        let row = self
            .get_workstation_grant(org, workstation_id, grant_id)?
            .ok_or(StoreError::ControlResourceConflict)?;
        tx.commit()?;
        Ok(row)
    }

    pub fn get_workstation_grant(
        &self,
        org: &str,
        workstation_id: &str,
        grant_id: &str,
    ) -> Result<Option<WorkstationGrant>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,workstation_id,grant_id,resource_kind,resource_ref,approved_by
                 FROM workstation_resource_grants
                 WHERE org_id=?1 AND workstation_id=?2 AND grant_id=?3",
                params![org, workstation_id, grant_id],
                |r| {
                    Ok(WorkstationGrant {
                        org_id: r.get(0)?,
                        workstation_id: r.get(1)?,
                        grant_id: r.get(2)?,
                        resource_kind: r.get(3)?,
                        resource_ref: r.get(4)?,
                        approved_by: r.get(5)?,
                    })
                },
            )
            .optional()?)
    }

    /// Team membership does not imply grant presence.
    pub fn workstation_grants_for(
        &self,
        org: &str,
        workstation_id: &str,
    ) -> Result<Vec<WorkstationGrant>> {
        let mut stmt = self.conn.prepare(
            "SELECT org_id,workstation_id,grant_id,resource_kind,resource_ref,approved_by
             FROM workstation_resource_grants
             WHERE org_id=?1 AND workstation_id=?2 ORDER BY grant_id",
        )?;
        let rows = stmt
            .query_map(params![org, workstation_id], |r| {
                Ok(WorkstationGrant {
                    org_id: r.get(0)?,
                    workstation_id: r.get(1)?,
                    grant_id: r.get(2)?,
                    resource_kind: r.get(3)?,
                    resource_ref: r.get(4)?,
                    approved_by: r.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Pin work to a workstation. Shared assignment required unless the actor
    /// is the workstation owner.
    pub fn pin_work_to_workstation(
        &self,
        actor: &str,
        org: &str,
        team: &str,
        work_id: &str,
        workstation_id: &str,
    ) -> Result<WorkPlacementPin> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_team_participant(actor, org, team)?;
        let ws = self
            .get_workstation(org, workstation_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if ws.status == "revoked" {
            return Err(StoreError::ControlAccessDenied);
        }
        if actor != ws.owner_principal_id && !ws.shared_assignment {
            return Err(StoreError::ControlAccessDenied);
        }
        if self.get_team_work_item(org, team, work_id)?.is_none() {
            return Err(StoreError::ControlAccessDenied);
        }
        self.conn.execute(
            "INSERT INTO work_placement_pins(org_id,team_id,work_id,workstation_id,created_at)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(org_id,team_id,work_id) DO UPDATE SET workstation_id=excluded.workstation_id",
            params![org, team, work_id, workstation_id, crate::util::now()],
        )?;
        tx.commit()?;
        Ok(WorkPlacementPin {
            org_id: org.into(),
            team_id: team.into(),
            work_id: work_id.into(),
            workstation_id: workstation_id.into(),
        })
    }

    pub fn get_work_placement_pin(
        &self,
        org: &str,
        team: &str,
        work_id: &str,
    ) -> Result<Option<WorkPlacementPin>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,team_id,work_id,workstation_id FROM work_placement_pins
                 WHERE org_id=?1 AND team_id=?2 AND work_id=?3",
                params![org, team, work_id],
                |r| {
                    Ok(WorkPlacementPin {
                        org_id: r.get(0)?,
                        team_id: r.get(1)?,
                        work_id: r.get(2)?,
                        workstation_id: r.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    /// Mark workstation offline and park pinned work in place (no upload/move).
    pub fn mark_workstation_offline(
        &self,
        actor: &str,
        org: &str,
        workstation_id: &str,
    ) -> Result<(Workstation, Vec<String>)> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let ws = self.require_ws_operator(actor, org, workstation_id)?;
        if ws.status == "revoked" {
            return Err(StoreError::ControlAccessDenied);
        }
        self.conn.execute(
            "UPDATE workstations SET status='offline'
             WHERE org_id=?1 AND workstation_id=?2 AND status IN ('enrolled','draining','offline')",
            params![org, workstation_id],
        )?;
        let parked = self.park_pins_for_workstation(org, workstation_id)?;
        let row = self
            .get_workstation(org, workstation_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        tx.commit()?;
        Ok((row, parked))
    }

    fn park_pins_for_workstation(&self, org: &str, workstation_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT team_id, work_id FROM work_placement_pins
             WHERE org_id=?1 AND workstation_id=?2",
        )?;
        let pins: Vec<(String, String)> = stmt
            .query_map(params![org, workstation_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut parked = Vec::new();
        for (team, work) in pins {
            let n = self.conn.execute(
                "UPDATE team_work_items
                 SET status='parked', attempt_id=NULL, run_id=NULL, version=version+1
                 WHERE org_id=?1 AND team_id=?2 AND work_id=?3 AND status IN ('open','running')",
                params![org, team, work],
            )?;
            if n > 0 {
                parked.push(work);
            }
        }
        Ok(parked)
    }

    fn require_ws_operator(
        &self,
        actor: &str,
        org: &str,
        workstation_id: &str,
    ) -> Result<Workstation> {
        let ws = self
            .get_workstation(org, workstation_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if ws.owner_principal_id == actor
            || self.control_access(
                actor,
                crate::ControlPermission::ManageOrganization,
                org,
                "",
            )?
        {
            Ok(ws)
        } else {
            Err(StoreError::ControlAccessDenied)
        }
    }

    /// Reconnect an offline device. Does not create duplicate work items.
    pub fn reconnect_workstation(
        &self,
        actor: &str,
        org: &str,
        workstation_id: &str,
    ) -> Result<Workstation> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let ws = self.require_ws_operator(actor, org, workstation_id)?;
        if ws.status == "revoked" {
            return Err(StoreError::ControlAccessDenied);
        }
        self.conn.execute(
            "UPDATE workstations SET status='enrolled'
             WHERE org_id=?1 AND workstation_id=?2 AND status IN ('offline','enrolled','draining')",
            params![org, workstation_id],
        )?;
        // Generation is unchanged on reconnect — claims remain unique by request_id.
        let row = self
            .get_workstation(org, workstation_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        tx.commit()?;
        Ok(row)
    }

    /// Revoke device credential: bump generation (fence), clear grants, park pins.
    pub fn revoke_workstation(
        &self,
        actor: &str,
        org: &str,
        workstation_id: &str,
    ) -> Result<Workstation> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let _ws = self.require_ws_operator(actor, org, workstation_id)?;
        self.conn.execute(
            "UPDATE workstations
             SET status='revoked', assignment_generation=assignment_generation+1,
                 revoked_at=?3, device_credential_hash='revoked'
             WHERE org_id=?1 AND workstation_id=?2",
            params![org, workstation_id, crate::util::now()],
        )?;
        self.conn.execute(
            "DELETE FROM workstation_resource_grants WHERE org_id=?1 AND workstation_id=?2",
            params![org, workstation_id],
        )?;
        let _ = self.park_pins_for_workstation(org, workstation_id)?;
        // Fence in-flight claims from the prior generation.
        self.conn.execute(
            "UPDATE worker_assignment_claims SET status='fenced'
             WHERE org_id=?1 AND workstation_id=?2 AND status IN ('claimed','accepted')",
            params![org, workstation_id],
        )?;
        let row = self
            .get_workstation(org, workstation_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        tx.commit()?;
        Ok(row)
    }

    /// Drain: stop new claims, keep generation, mark draining.
    pub fn drain_workstation(
        &self,
        actor: &str,
        org: &str,
        workstation_id: &str,
    ) -> Result<Workstation> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let _ws = self.require_ws_operator(actor, org, workstation_id)?;
        self.conn.execute(
            "UPDATE workstations SET status='draining'
             WHERE org_id=?1 AND workstation_id=?2 AND status IN ('enrolled','draining')",
            params![org, workstation_id],
        )?;
        let row = self
            .get_workstation(org, workstation_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        tx.commit()?;
        Ok(row)
    }

    /// Claim work under the current assignment generation. Stale generations and
    /// draining/revoked/offline devices cannot obtain new mediated effects.
    pub fn claim_worker_assignment(
        &self,
        org: &str,
        workstation_id: &str,
        device_secret: &str,
        assignment_id: &str,
        request_id: &str,
        work_id: Option<&str>,
        claimed_generation: i64,
    ) -> Result<WorkerAssignmentClaim> {
        validate_id(assignment_id, "assignment_id")?;
        validate_id(request_id, "request_id")?;
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.verify_workstation_device_credential(org, workstation_id, device_secret)? {
            return Err(StoreError::ControlAccessDenied);
        }
        let ws = self
            .get_workstation(org, workstation_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if ws.status != "enrolled" {
            return Err(StoreError::ControlAccessDenied);
        }
        if claimed_generation != ws.assignment_generation {
            return Err(StoreError::ControlAccessDenied);
        }
        if let Some(existing) = self.assignment_by_request(org, workstation_id, request_id)? {
            if existing.assignment_id != assignment_id
                || existing.generation != claimed_generation
            {
                return Err(StoreError::ControlResourceConflict);
            }
            tx.commit()?;
            return Ok(existing);
        }
        self.conn.execute(
            "INSERT INTO worker_assignment_claims(
                org_id,workstation_id,assignment_id,generation,work_id,status,request_id,created_at
             ) VALUES(?1,?2,?3,?4,?5,'claimed',?6,?7)",
            params![
                org,
                workstation_id,
                assignment_id,
                claimed_generation,
                work_id,
                request_id,
                crate::util::now()
            ],
        )?;
        let row = self
            .get_assignment_claim(org, workstation_id, assignment_id)?
            .ok_or(StoreError::ControlResourceConflict)?;
        tx.commit()?;
        Ok(row)
    }

    fn assignment_by_request(
        &self,
        org: &str,
        workstation_id: &str,
        request_id: &str,
    ) -> Result<Option<WorkerAssignmentClaim>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,workstation_id,assignment_id,generation,work_id,status,request_id
                 FROM worker_assignment_claims
                 WHERE org_id=?1 AND workstation_id=?2 AND request_id=?3",
                params![org, workstation_id, request_id],
                Self::map_claim,
            )
            .optional()?)
    }

    pub fn get_assignment_claim(
        &self,
        org: &str,
        workstation_id: &str,
        assignment_id: &str,
    ) -> Result<Option<WorkerAssignmentClaim>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id,workstation_id,assignment_id,generation,work_id,status,request_id
                 FROM worker_assignment_claims
                 WHERE org_id=?1 AND workstation_id=?2 AND assignment_id=?3",
                params![org, workstation_id, assignment_id],
                Self::map_claim,
            )
            .optional()?)
    }

    fn map_claim(r: &rusqlite::Row<'_>) -> rusqlite::Result<WorkerAssignmentClaim> {
        Ok(WorkerAssignmentClaim {
            org_id: r.get(0)?,
            workstation_id: r.get(1)?,
            assignment_id: r.get(2)?,
            generation: r.get(3)?,
            work_id: r.get(4)?,
            status: r.get(5)?,
            request_id: r.get(6)?,
        })
    }

    /// Accept a result only for an unfenced claim on the current generation.
    pub fn accept_worker_assignment_result(
        &self,
        org: &str,
        workstation_id: &str,
        device_secret: &str,
        assignment_id: &str,
        claimed_generation: i64,
    ) -> Result<WorkerAssignmentClaim> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !self.verify_workstation_device_credential(org, workstation_id, device_secret)? {
            return Err(StoreError::ControlAccessDenied);
        }
        let ws = self
            .get_workstation(org, workstation_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if claimed_generation != ws.assignment_generation {
            return Err(StoreError::ControlAccessDenied);
        }
        let current = self
            .get_assignment_claim(org, workstation_id, assignment_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        if current.status == "fenced" || current.generation != claimed_generation {
            return Err(StoreError::ControlAccessDenied);
        }
        if current.status == "accepted" {
            tx.commit()?;
            return Ok(current);
        }
        if current.status != "claimed" {
            return Err(StoreError::ControlAccessDenied);
        }
        self.conn.execute(
            "UPDATE worker_assignment_claims SET status='accepted'
             WHERE org_id=?1 AND workstation_id=?2 AND assignment_id=?3 AND status='claimed'",
            params![org, workstation_id, assignment_id],
        )?;
        let row = self
            .get_assignment_claim(org, workstation_id, assignment_id)?
            .ok_or(StoreError::ControlAccessDenied)?;
        tx.commit()?;
        Ok(row)
    }

    /// Activation gate: pinned work requires an enrolled (not offline/revoked) workstation.
    pub fn placement_blocks_activation(
        &self,
        org: &str,
        team: &str,
        work_id: &str,
    ) -> Result<Option<String>> {
        let Some(pin) = self.get_work_placement_pin(org, team, work_id)? else {
            return Ok(None);
        };
        let Some(ws) = self.get_workstation(org, &pin.workstation_id)? else {
            return Ok(Some("pinned workstation missing".into()));
        };
        match ws.status.as_str() {
            "enrolled" => Ok(None),
            "offline" => Ok(Some("pinned workstation is offline".into())),
            "draining" => Ok(Some("pinned workstation is draining".into())),
            "revoked" => Ok(Some("pinned workstation is revoked".into())),
            other => Ok(Some(format!("pinned workstation status is {other}"))),
        }
    }
}

#[cfg(test)]
#[path = "workstation_placement_tests.rs"]
mod tests;
