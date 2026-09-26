//! Team goals, work items, huddles and bounded delegation (MVP-301/302).
use super::*;
use tetonic_memory::{
    HuddleProposal, TeamGoal, TeamWorkItem, WorkActivationCursor, WorkDelegation,
};

impl ResourceService {
    pub async fn create_team_goal(
        &self,
        credential: &str,
        org: String,
        team: String,
        goal_id: String,
        title: String,
    ) -> Result<TeamGoal, ResourceError> {
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
                db.create_team_goal(&actor.principal_id, &org, &team, &goal_id, &title)
            })
            .await??)
    }

    pub async fn create_team_work_item(
        &self,
        credential: &str,
        org: String,
        team: String,
        work_id: String,
        title: String,
        request_id: String,
        goal_id: Option<String>,
    ) -> Result<TeamWorkItem, ResourceError> {
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
                db.create_team_work_item(
                    &actor.principal_id,
                    &org,
                    &team,
                    &work_id,
                    &title,
                    &request_id,
                    goal_id.as_deref(),
                )
            })
            .await??)
    }

    pub async fn list_team_work_items(
        &self,
        credential: &str,
        org: String,
        team: String,
    ) -> Result<Vec<TeamWorkItem>, ResourceError> {
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
            .read(move |db| db.list_team_work_items(&actor.principal_id, &org, &team))
            .await??)
    }

    pub async fn park_team_work_item(
        &self,
        credential: &str,
        org: String,
        team: String,
        work_id: String,
    ) -> Result<TeamWorkItem, ResourceError> {
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
                db.park_team_work_item(&actor.principal_id, &org, &team, &work_id)
            })
            .await??)
    }

    pub async fn resume_team_work_item(
        &self,
        credential: &str,
        org: String,
        team: String,
        work_id: String,
    ) -> Result<TeamWorkItem, ResourceError> {
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
                db.resume_team_work_item(&actor.principal_id, &org, &team, &work_id)
            })
            .await??)
    }

    pub async fn activate_team_work_item(
        &self,
        credential: &str,
        org: String,
        team: String,
        work_id: String,
        attempt_id: String,
        run_id: String,
    ) -> Result<TeamWorkItem, ResourceError> {
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
                db.activate_team_work_item(
                    &actor.principal_id,
                    &org,
                    &team,
                    &work_id,
                    &attempt_id,
                    &run_id,
                )
            })
            .await??)
    }

    pub async fn get_team_work_item(
        &self,
        credential: &str,
        org: String,
        team: String,
        work_id: String,
    ) -> Result<Option<TeamWorkItem>, ResourceError> {
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
            .read(move |db| {
                db.require_team_participant(&actor.principal_id, &org, &team)?;
                db.get_team_work_item(&org, &team, &work_id)
            })
            .await??)
    }

    pub async fn accept_huddle_proposal(
        &self,
        credential: &str,
        proposal: HuddleProposal,
    ) -> Result<Vec<TeamWorkItem>, ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageTeam {
                    org_id: proposal.org_id.clone(),
                    team_id: proposal.team_id.clone(),
                },
            )
            .await?;
        Ok(self
            .store
            .write(move |db| db.accept_huddle_proposal(&actor.principal_id, &proposal))
            .await??)
    }

    pub async fn activate_from_cursor(
        &self,
        credential: &str,
        org: String,
        team: String,
        source: String,
        cursor_key: String,
        event_id: String,
        work_title: String,
    ) -> Result<(WorkActivationCursor, Option<TeamWorkItem>), ResourceError> {
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
                db.activate_from_cursor(
                    &actor.principal_id,
                    &org,
                    &team,
                    &source,
                    &cursor_key,
                    &event_id,
                    &work_title,
                )
            })
            .await??)
    }

    pub async fn create_work_delegation(
        &self,
        credential: &str,
        org: String,
        team: String,
        delegation_id: String,
        parent_work_id: String,
        child_work_id: String,
        child_title: String,
        request_id: String,
        parent_budget_tokens: i64,
        child_budget_tokens: i64,
        stop_scope: String,
        peer_org: Option<String>,
        peer_team: Option<String>,
    ) -> Result<WorkDelegation, ResourceError> {
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
                db.create_work_delegation(
                    &actor.principal_id,
                    &org,
                    &team,
                    &delegation_id,
                    &parent_work_id,
                    &child_work_id,
                    &child_title,
                    &request_id,
                    parent_budget_tokens,
                    child_budget_tokens,
                    &stop_scope,
                    peer_org.as_deref(),
                    peer_team.as_deref(),
                )
            })
            .await??)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resource_service_exposes_goals_huddles_activation_and_delegation() {
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
        let resources = local.resources();
        resources
            .create_team(
                issued.expose_secret(),
                "org".into(),
                "team".into(),
                "Team".into(),
            )
            .await
            .unwrap();
        resources
            .create_team_goal(
                issued.expose_secret(),
                "org".into(),
                "team".into(),
                "g1".into(),
                "Stay online".into(),
            )
            .await
            .unwrap();
        let work = resources
            .create_team_work_item(
                issued.expose_secret(),
                "org".into(),
                "team".into(),
                "w1".into(),
                "Ship".into(),
                "req-1".into(),
                Some("g1".into()),
            )
            .await
            .unwrap();
        assert_eq!(work.status, "open");
        let proposal = HuddleProposal {
            org_id: "org".into(),
            team_id: "team".into(),
            huddle_id: "h1".into(),
            proposal_version: 1,
            status: "accepted".into(),
            request_id: "huddle-1".into(),
            created_by: "admin".into(),
            work_titles: vec!["One".into(), "Two".into()],
        };
        let created = resources
            .accept_huddle_proposal(issued.expose_secret(), proposal.clone())
            .await
            .unwrap();
        assert_eq!(created.len(), 2);
        assert_eq!(
            resources
                .accept_huddle_proposal(issued.expose_secret(), proposal)
                .await
                .unwrap()
                .len(),
            2
        );
        let parked = resources
            .park_team_work_item(
                issued.expose_secret(),
                "org".into(),
                "team".into(),
                "w1".into(),
            )
            .await
            .unwrap();
        assert_eq!(parked.status, "parked");
        let resumed = resources
            .resume_team_work_item(
                issued.expose_secret(),
                "org".into(),
                "team".into(),
                "w1".into(),
            )
            .await
            .unwrap();
        assert_eq!(resumed.status, "open");
        let active = resources
            .activate_team_work_item(
                issued.expose_secret(),
                "org".into(),
                "team".into(),
                "w1".into(),
                "attempt-1".into(),
                "run-1".into(),
            )
            .await
            .unwrap();
        assert_eq!(active.status, "running");
        assert_eq!(active.run_id.as_deref(), Some("run-1"));
        let (cursor, event_work) = resources
            .activate_from_cursor(
                issued.expose_secret(),
                "org".into(),
                "team".into(),
                "event".into(),
                "inbox".into(),
                "e1".into(),
                "Handle".into(),
            )
            .await
            .unwrap();
        assert_eq!(cursor.last_event_id, "e1");
        assert!(event_work.is_some());
        assert!(resources
            .activate_from_cursor(
                issued.expose_secret(),
                "org".into(),
                "team".into(),
                "event".into(),
                "inbox".into(),
                "e1".into(),
                "Handle".into(),
            )
            .await
            .unwrap()
            .1
            .is_none());
        let delegation = resources
            .create_work_delegation(
                issued.expose_secret(),
                "org".into(),
                "team".into(),
                "d1".into(),
                "w1".into(),
                "child".into(),
                "Help".into(),
                "del-1".into(),
                50,
                20,
                "inherit".into(),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(delegation.child_budget_tokens, 20);
        assert!(resources
            .create_work_delegation(
                issued.expose_secret(),
                "org".into(),
                "team".into(),
                "d2".into(),
                "w1".into(),
                "x".into(),
                "Nope".into(),
                "del-x".into(),
                50,
                10,
                "inherit".into(),
                Some("other".into()),
                Some("team".into()),
            )
            .await
            .is_err());
    }
}
