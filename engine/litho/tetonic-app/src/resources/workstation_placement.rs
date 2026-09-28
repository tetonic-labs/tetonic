//! Org workstations, placement pins and assignment fencing (MVP-501/502).
use super::*;
use tetonic_memory::{
    WorkPlacementPin, WorkerAssignmentClaim, Workstation, WorkstationGrant,
};

impl ResourceService {
    pub async fn enroll_workstation(
        &self,
        credential: &str,
        org: String,
        workstation_id: String,
        label: String,
        platform: String,
        device_secret: String,
        shared_assignment: bool,
    ) -> Result<Workstation, ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageOrganization {
                    org_id: org.clone(),
                },
            )
            .await?;
        Ok(self
            .store
            .write(move |db| {
                db.enroll_workstation(
                    &actor.principal_id,
                    &org,
                    &workstation_id,
                    &label,
                    &platform,
                    &device_secret,
                    shared_assignment,
                )
            })
            .await??)
    }

    pub async fn approve_workstation_grant(
        &self,
        credential: &str,
        org: String,
        workstation_id: String,
        grant_id: String,
        resource_kind: String,
        resource_ref: String,
    ) -> Result<WorkstationGrant, ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageOrganization {
                    org_id: org.clone(),
                },
            )
            .await?;
        Ok(self
            .store
            .write(move |db| {
                db.approve_workstation_grant(
                    &actor.principal_id,
                    &org,
                    &workstation_id,
                    &grant_id,
                    &resource_kind,
                    &resource_ref,
                )
            })
            .await??)
    }

    pub async fn pin_work_to_workstation(
        &self,
        credential: &str,
        org: String,
        team: String,
        work_id: String,
        workstation_id: String,
    ) -> Result<WorkPlacementPin, ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageTeam {
                    org_id: org.clone(),
                    team_id: team.clone(),
                },
            )
            .await?;
        Ok(self
            .store
            .write(move |db| {
                db.pin_work_to_workstation(
                    &actor.principal_id,
                    &org,
                    &team,
                    &work_id,
                    &workstation_id,
                )
            })
            .await??)
    }

    pub async fn mark_workstation_offline(
        &self,
        credential: &str,
        org: String,
        workstation_id: String,
    ) -> Result<(Workstation, Vec<String>), ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageOrganization {
                    org_id: org.clone(),
                },
            )
            .await?;
        Ok(self
            .store
            .write(move |db| {
                db.mark_workstation_offline(&actor.principal_id, &org, &workstation_id)
            })
            .await??)
    }

    pub async fn reconnect_workstation(
        &self,
        credential: &str,
        org: String,
        workstation_id: String,
    ) -> Result<Workstation, ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageOrganization {
                    org_id: org.clone(),
                },
            )
            .await?;
        Ok(self
            .store
            .write(move |db| {
                db.reconnect_workstation(&actor.principal_id, &org, &workstation_id)
            })
            .await??)
    }

    pub async fn revoke_workstation(
        &self,
        credential: &str,
        org: String,
        workstation_id: String,
    ) -> Result<Workstation, ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageOrganization {
                    org_id: org.clone(),
                },
            )
            .await?;
        Ok(self
            .store
            .write(move |db| {
                db.revoke_workstation(&actor.principal_id, &org, &workstation_id)
            })
            .await??)
    }

    pub async fn drain_workstation(
        &self,
        credential: &str,
        org: String,
        workstation_id: String,
    ) -> Result<Workstation, ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageOrganization {
                    org_id: org.clone(),
                },
            )
            .await?;
        Ok(self
            .store
            .write(move |db| {
                db.drain_workstation(&actor.principal_id, &org, &workstation_id)
            })
            .await??)
    }

    pub async fn claim_worker_assignment(
        &self,
        org: String,
        workstation_id: String,
        device_secret: String,
        assignment_id: String,
        request_id: String,
        work_id: Option<String>,
        claimed_generation: i64,
    ) -> Result<WorkerAssignmentClaim, ResourceError> {
        // Device secret is the authority — not an employee ManageTeam credential.
        Ok(self
            .store
            .write(move |db| {
                db.claim_worker_assignment(
                    &org,
                    &workstation_id,
                    &device_secret,
                    &assignment_id,
                    &request_id,
                    work_id.as_deref(),
                    claimed_generation,
                )
            })
            .await??)
    }

    pub async fn accept_worker_assignment_result(
        &self,
        org: String,
        workstation_id: String,
        device_secret: String,
        assignment_id: String,
        claimed_generation: i64,
    ) -> Result<WorkerAssignmentClaim, ResourceError> {
        Ok(self
            .store
            .write(move |db| {
                db.accept_worker_assignment_result(
                    &org,
                    &workstation_id,
                    &device_secret,
                    &assignment_id,
                    claimed_generation,
                )
            })
            .await??)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resource_service_workstation_placement_and_fencing() {
        let dir = tempfile::tempdir().unwrap();
        let local = LocalControl::open(dir.path().join("control.db"), "test".into())
            .await
            .unwrap();
        local
            .bootstrap("admin".into(), "org".into(), "Org".into())
            .await
            .unwrap();
        let issued = local
            .credentials()
            .issue("admin".into(), 3600)
            .await
            .unwrap();
        let secret = issued.expose_secret();
        let resources = local.resources();
        resources
            .create_team(secret, "org".into(), "team".into(), "Team".into())
            .await
            .unwrap();
        let ws = resources
            .enroll_workstation(
                secret,
                "org".into(),
                "laptop".into(),
                "Admin laptop".into(),
                "linux".into(),
                "device-secret".into(),
                true,
            )
            .await
            .unwrap();
        assert_eq!(ws.status, "enrolled");
        resources
            .approve_workstation_grant(
                secret,
                "org".into(),
                "laptop".into(),
                "g1".into(),
                "path".into(),
                "/work".into(),
            )
            .await
            .unwrap();
        resources
            .create_team_work_item(
                secret,
                "org".into(),
                "team".into(),
                "w1".into(),
                "Ship".into(),
                "req".into(),
                None,
            )
            .await
            .unwrap();
        resources
            .pin_work_to_workstation(
                secret,
                "org".into(),
                "team".into(),
                "w1".into(),
                "laptop".into(),
            )
            .await
            .unwrap();
        let (offline, parked) = resources
            .mark_workstation_offline(secret, "org".into(), "laptop".into())
            .await
            .unwrap();
        assert_eq!(offline.status, "offline");
        assert_eq!(parked, vec!["w1".to_string()]);
        resources
            .reconnect_workstation(secret, "org".into(), "laptop".into())
            .await
            .unwrap();
        let claim = resources
            .claim_worker_assignment(
                "org".into(),
                "laptop".into(),
                "device-secret".into(),
                "a1".into(),
                "areq".into(),
                Some("w1".into()),
                1,
            )
            .await
            .unwrap();
        assert_eq!(claim.status, "claimed");
        resources
            .drain_workstation(secret, "org".into(), "laptop".into())
            .await
            .unwrap();
        assert!(resources
            .claim_worker_assignment(
                "org".into(),
                "laptop".into(),
                "device-secret".into(),
                "a2".into(),
                "areq2".into(),
                None,
                1,
            )
            .await
            .is_err());
        resources
            .reconnect_workstation(secret, "org".into(), "laptop".into())
            .await
            .unwrap();
        resources
            .accept_worker_assignment_result(
                "org".into(),
                "laptop".into(),
                "device-secret".into(),
                "a1".into(),
                1,
            )
            .await
            .unwrap();
        let revoked = resources
            .revoke_workstation(secret, "org".into(), "laptop".into())
            .await
            .unwrap();
        assert_eq!(revoked.status, "revoked");
        assert!(resources
            .accept_worker_assignment_result(
                "org".into(),
                "laptop".into(),
                "device-secret".into(),
                "a1".into(),
                1,
            )
            .await
            .is_err());
    }
}
