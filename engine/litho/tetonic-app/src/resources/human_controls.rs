//! Hierarchical stops, effect approvals and team effort (MVP-401/402).
use super::*;
use tetonic_memory::{ControlStop, EffectApproval, TeamEffortEntry, TeamWorkInspection};

impl ResourceService {
    pub async fn request_control_stop(
        &self,
        credential: &str,
        org: String,
        scope_kind: String,
        scope_id: String,
        mode: String,
        reason: String,
    ) -> Result<ControlStop, ResourceError> {
        self.request_control_stop_with_runs(credential, org, scope_kind, scope_id, mode, reason)
            .await
            .map(|(stop, _)| stop)
    }

    pub(crate) async fn request_control_stop_with_runs(
        &self,
        credential: &str,
        org: String,
        scope_kind: String,
        scope_id: String,
        mode: String,
        reason: String,
    ) -> Result<(ControlStop, Vec<String>), ResourceError> {
        let action = match scope_kind.as_str() {
            "org" | "agent" => ResourceAction::ManageOrganization {
                org_id: org.clone(),
            },
            "team" => ResourceAction::ManageTeam {
                org_id: org.clone(),
                team_id: scope_id.clone(),
            },
            _ => ResourceAction::ManageOrganization {
                org_id: org.clone(),
            },
        };
        let actor = self.authority.authorize(credential, &action).await?;
        Ok(self
            .store
            .write(move |db| {
                db.request_control_stop_with_runs(
                    &actor.principal_id,
                    &org,
                    &scope_kind,
                    &scope_id,
                    &mode,
                    &reason,
                )
            })
            .await??)
    }

    pub async fn clear_control_stop(
        &self,
        credential: &str,
        org: String,
        scope_kind: String,
        scope_id: String,
    ) -> Result<ControlStop, ResourceError> {
        let action = match scope_kind.as_str() {
            "team" => ResourceAction::ManageTeam {
                org_id: org.clone(),
                team_id: scope_id.clone(),
            },
            _ => ResourceAction::ManageOrganization {
                org_id: org.clone(),
            },
        };
        let actor = self.authority.authorize(credential, &action).await?;
        Ok(self
            .store
            .write(move |db| {
                db.clear_control_stop(&actor.principal_id, &org, &scope_kind, &scope_id)
            })
            .await??)
    }

    pub async fn propose_effect_approval(
        &self,
        credential: &str,
        command: crate::resources::ProposeEffectApproval,
    ) -> Result<EffectApproval, ResourceError> {
        let crate::resources::ProposeEffectApproval {
            org,
            team,
            approval_id,
            proposal_digest,
            request_id,
            expires_at,
            work_id,
        } = command;
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
                db.propose_effect_approval(tetonic_memory::ProposeEffectApproval {
                    actor: &actor.principal_id,
                    org: &org,
                    team: &team,
                    approval_id: &approval_id,
                    proposal_digest: &proposal_digest,
                    request_id: &request_id,
                    expires_at,
                    work_id: work_id.as_deref(),
                })
            })
            .await??)
    }

    pub async fn resolve_effect_approval(
        &self,
        credential: &str,
        command: crate::resources::ResolveEffectApproval,
    ) -> Result<EffectApproval, ResourceError> {
        let crate::resources::ResolveEffectApproval {
            org,
            team,
            approval_id,
            proposal_digest,
            allow,
            now_unix,
        } = command;
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
                db.resolve_effect_approval(tetonic_memory::ResolveEffectApproval {
                    actor: &actor.principal_id,
                    org: &org,
                    team: &team,
                    approval_id: &approval_id,
                    proposal_digest: &proposal_digest,
                    allow,
                    now_unix,
                })
            })
            .await??)
    }

    pub async fn effect_approval_allows_dispatch(
        &self,
        credential: &str,
        org: String,
        team: String,
        approval_id: String,
        proposal_digest: String,
        now_unix: i64,
    ) -> Result<bool, ResourceError> {
        let _actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ReadTeam {
                    org_id: org.clone(),
                    team_id: team.clone(),
                },
            )
            .await?;
        Ok(self
            .store
            .read(move |db| {
                db.effect_approval_allows_dispatch(
                    &org,
                    &team,
                    &approval_id,
                    &proposal_digest,
                    now_unix,
                )
            })
            .await??)
    }

    pub async fn record_team_effort(
        &self,
        credential: &str,
        command: crate::resources::RecordTeamEffort,
    ) -> Result<TeamEffortEntry, ResourceError> {
        let crate::resources::RecordTeamEffort {
            org,
            team,
            entry_id,
            request_id,
            measured_tokens,
            goal_id,
            work_id,
        } = command;
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
                db.record_team_effort(tetonic_memory::RecordTeamEffort {
                    actor: &actor.principal_id,
                    org: &org,
                    team: &team,
                    entry_id: &entry_id,
                    request_id: &request_id,
                    measured_tokens,
                    goal_id: goal_id.as_deref(),
                    work_id: work_id.as_deref(),
                })
            })
            .await??)
    }

    pub async fn inspect_team_work(
        &self,
        credential: &str,
        org: String,
        team: String,
    ) -> Result<TeamWorkInspection, ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ReadTeam {
                    org_id: org.clone(),
                    team_id: team.clone(),
                },
            )
            .await?;
        Ok(self
            .store
            .read(move |db| db.inspect_team_work(&actor.principal_id, &org, &team))
            .await??)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resource_service_stops_approvals_and_team_inspection() {
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
        resources
            .create_team_goal(
                secret,
                "org".into(),
                "team".into(),
                "g1".into(),
                "Goal".into(),
            )
            .await
            .unwrap();
        resources
            .create_team_work_item(
                secret,
                crate::resources::CreateTeamWorkItem {
                    org: "org".into(),
                    team: "team".into(),
                    work_id: "w1".into(),
                    title: "Ship".into(),
                    request_id: "req-1".into(),
                    goal_id: Some("g1".into()),
                },
            )
            .await
            .unwrap();
        let stop = resources
            .request_control_stop(
                secret,
                "org".into(),
                "goal".into(),
                "g1".into(),
                "pause".into(),
                "hold".into(),
            )
            .await
            .unwrap();
        assert_eq!(stop.mode, "pause");
        let view = resources
            .inspect_team_work(secret, "org".into(), "team".into())
            .await
            .unwrap();
        assert!(view
            .work_items
            .iter()
            .any(|w| w.work_id == "w1" && w.status == "parked"));
        assert!(!view.active_stops.is_empty());
        let approval = resources
            .propose_effect_approval(
                secret,
                crate::resources::ProposeEffectApproval {
                    org: "org".into(),
                    team: "team".into(),
                    approval_id: "ap1".into(),
                    proposal_digest: "digest-a".into(),
                    request_id: "areq".into(),
                    expires_at: 2_000_000_000,
                    work_id: Some("w1".into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(approval.status, "pending");
        assert!(!resources
            .effect_approval_allows_dispatch(
                secret,
                "org".into(),
                "team".into(),
                "ap1".into(),
                "digest-a".into(),
                1_000,
            )
            .await
            .unwrap());
        resources
            .resolve_effect_approval(
                secret,
                crate::resources::ResolveEffectApproval {
                    org: "org".into(),
                    team: "team".into(),
                    approval_id: "ap1".into(),
                    proposal_digest: "digest-a".into(),
                    allow: true,
                    now_unix: 1_000,
                },
            )
            .await
            .unwrap();
        assert!(resources
            .effect_approval_allows_dispatch(
                secret,
                "org".into(),
                "team".into(),
                "ap1".into(),
                "digest-a".into(),
                1_000,
            )
            .await
            .unwrap());
        let effort = resources
            .record_team_effort(
                secret,
                crate::resources::RecordTeamEffort {
                    org: "org".into(),
                    team: "team".into(),
                    entry_id: "e1".into(),
                    request_id: "ereq".into(),
                    measured_tokens: None,
                    goal_id: Some("g1".into()),
                    work_id: Some("w1".into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(effort.status, "unknown");
        resources
            .clear_control_stop(secret, "org".into(), "goal".into(), "g1".into())
            .await
            .unwrap();
    }
}
