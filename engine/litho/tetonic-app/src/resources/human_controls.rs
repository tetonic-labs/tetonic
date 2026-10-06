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
        org: String,
        team: String,
        approval_id: String,
        proposal_digest: String,
        request_id: String,
        expires_at: i64,
        work_id: Option<String>,
    ) -> Result<EffectApproval, ResourceError> {
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
                db.propose_effect_approval(
                    &actor.principal_id,
                    &org,
                    &team,
                    &approval_id,
                    &proposal_digest,
                    &request_id,
                    expires_at,
                    work_id.as_deref(),
                )
            })
            .await??)
    }

    pub async fn resolve_effect_approval(
        &self,
        credential: &str,
        org: String,
        team: String,
        approval_id: String,
        proposal_digest: String,
        allow: bool,
        now_unix: i64,
    ) -> Result<EffectApproval, ResourceError> {
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
                db.resolve_effect_approval(
                    &actor.principal_id,
                    &org,
                    &team,
                    &approval_id,
                    &proposal_digest,
                    allow,
                    now_unix,
                )
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
        org: String,
        team: String,
        entry_id: String,
        request_id: String,
        measured_tokens: Option<i64>,
        goal_id: Option<String>,
        work_id: Option<String>,
    ) -> Result<TeamEffortEntry, ResourceError> {
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
                db.record_team_effort(
                    &actor.principal_id,
                    &org,
                    &team,
                    &entry_id,
                    &request_id,
                    measured_tokens,
                    goal_id.as_deref(),
                    work_id.as_deref(),
                )
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
                "org".into(),
                "team".into(),
                "w1".into(),
                "Ship".into(),
                "req-1".into(),
                Some("g1".into()),
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
                "org".into(),
                "team".into(),
                "ap1".into(),
                "digest-a".into(),
                "areq".into(),
                2_000_000_000,
                Some("w1".into()),
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
                "org".into(),
                "team".into(),
                "ap1".into(),
                "digest-a".into(),
                true,
                1_000,
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
                "org".into(),
                "team".into(),
                "e1".into(),
                "ereq".into(),
                None,
                Some("g1".into()),
                Some("w1".into()),
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
