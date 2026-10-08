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
        command: crate::resources::CreateTeamWorkItem,
    ) -> Result<TeamWorkItem, ResourceError> {
        self.create_team_work_item_with_input(credential, command, None)
            .await
    }

    pub async fn create_team_work_item_with_input(
        &self,
        credential: &str,
        command: crate::resources::CreateTeamWorkItem,
        input: Option<String>,
    ) -> Result<TeamWorkItem, ResourceError> {
        self.create_team_work_item_for_purpose(
            credential,
            command,
            input,
            tetonic_memory::WorkPurpose::Work,
        )
        .await
    }

    pub async fn create_team_work_item_for_purpose(
        &self,
        credential: &str,
        command: crate::resources::CreateTeamWorkItem,
        input: Option<String>,
        purpose: tetonic_memory::WorkPurpose,
    ) -> Result<TeamWorkItem, ResourceError> {
        self.create_work_with_roster(credential, command, input, purpose, None)
            .await
    }

    pub(crate) async fn create_work_with_roster(
        &self,
        credential: &str,
        command: crate::resources::CreateTeamWorkItem,
        input: Option<String>,
        purpose: tetonic_memory::WorkPurpose,
        roster: Option<(Option<tetonic_memory::WorkTeamSelection>, Option<String>)>,
    ) -> Result<TeamWorkItem, ResourceError> {
        let crate::resources::CreateTeamWorkItem {
            org,
            team,
            work_id,
            title,
            request_id,
            goal_id,
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
                let command = tetonic_memory::CreateTeamWorkItem {
                    actor: &actor.principal_id,
                    org: &org,
                    team: &team,
                    work_id: &work_id,
                    title: &title,
                    request_id: &request_id,
                    goal_id: goal_id.as_deref(),
                };
                if let Some((selection, parent)) = roster {
                    db.create_rostered_work(
                        command,
                        input.as_deref(),
                        purpose,
                        selection.as_ref(),
                        parent.as_deref(),
                    )
                } else {
                    db.create_team_work_item_for_purpose(command, input.as_deref(), purpose)
                }
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
            .write(move |db| db.park_team_work_item(&actor.principal_id, &org, &team, &work_id))
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
            .write(move |db| db.resume_team_work_item(&actor.principal_id, &org, &team, &work_id))
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
        command: crate::resources::ActivateWorkCursor,
    ) -> Result<(WorkActivationCursor, Option<TeamWorkItem>), ResourceError> {
        let crate::resources::ActivateWorkCursor {
            org,
            team,
            source,
            cursor_key,
            event_id,
            work_title,
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
                db.activate_from_cursor(tetonic_memory::ActivateWorkCursor {
                    actor: &actor.principal_id,
                    org: &org,
                    team: &team,
                    source: &source,
                    cursor_key: &cursor_key,
                    event_id: &event_id,
                    work_title: &work_title,
                })
            })
            .await??)
    }

    pub async fn create_work_delegation(
        &self,
        credential: &str,
        command: crate::resources::CreateWorkDelegation,
    ) -> Result<WorkDelegation, ResourceError> {
        let crate::resources::CreateWorkDelegation {
            org,
            team,
            delegation_id,
            parent_work_id,
            child_work_id,
            child_title,
            request_id,
            parent_budget_tokens,
            child_budget_tokens,
            stop_scope,
            peer_org,
            peer_team,
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
                db.create_work_delegation(tetonic_memory::CreateWorkDelegation {
                    actor: &actor.principal_id,
                    org: &org,
                    team: &team,
                    delegation_id: &delegation_id,
                    parent_work_id: &parent_work_id,
                    child_work_id: &child_work_id,
                    child_title: &child_title,
                    request_id: &request_id,
                    parent_budget_tokens,
                    child_budget_tokens,
                    stop_scope: &stop_scope,
                    peer_org: peer_org.as_deref(),
                    peer_team: peer_team.as_deref(),
                })
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
        assert_eq!(work.status, "open");
        resources
            .authorize_work_budget(
                issued.expose_secret(),
                "org".into(),
                "team".into(),
                "w1".into(),
                "budget-w1".into(),
                50,
            )
            .await
            .unwrap();
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
                crate::resources::ActivateWorkCursor {
                    org: "org".into(),
                    team: "team".into(),
                    source: "event".into(),
                    cursor_key: "inbox".into(),
                    event_id: "e1".into(),
                    work_title: "Handle".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(cursor.last_event_id, "e1");
        assert!(event_work.is_some());
        assert!(resources
            .activate_from_cursor(
                issued.expose_secret(),
                crate::resources::ActivateWorkCursor {
                    org: "org".into(),
                    team: "team".into(),
                    source: "event".into(),
                    cursor_key: "inbox".into(),
                    event_id: "e1".into(),
                    work_title: "Handle".into()
                }
            )
            .await
            .unwrap()
            .1
            .is_none());
        let delegation = resources
            .create_work_delegation(
                issued.expose_secret(),
                crate::resources::CreateWorkDelegation {
                    org: "org".into(),
                    team: "team".into(),
                    delegation_id: "d1".into(),
                    parent_work_id: "w1".into(),
                    child_work_id: "child".into(),
                    child_title: "Help".into(),
                    request_id: "del-1".into(),
                    parent_budget_tokens: 50,
                    child_budget_tokens: 20,
                    stop_scope: "inherit".into(),
                    peer_org: None,
                    peer_team: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(delegation.child_budget_tokens, 20);
        let reserved = resources
            .reserve_work_budget(
                issued.expose_secret(),
                "org".into(),
                "team".into(),
                "w1".into(),
                "orchestrator-effort".into(),
                5,
            )
            .await
            .unwrap();
        assert_eq!(reserved.tokens, 5);
        let budget = resources
            .work_budget(
                issued.expose_secret(),
                "org".into(),
                "team".into(),
                "w1".into(),
            )
            .await
            .unwrap();
        assert_eq!(
            (
                budget.delegated_tokens,
                budget.reserved_tokens,
                budget.available_tokens
            ),
            (20, 5, 25)
        );
        assert!(resources
            .create_work_delegation(
                issued.expose_secret(),
                crate::resources::CreateWorkDelegation {
                    org: "org".into(),
                    team: "team".into(),
                    delegation_id: "d2".into(),
                    parent_work_id: "w1".into(),
                    child_work_id: "x".into(),
                    child_title: "Nope".into(),
                    request_id: "del-x".into(),
                    parent_budget_tokens: 50,
                    child_budget_tokens: 10,
                    stop_scope: "inherit".into(),
                    peer_org: Some("other".into()),
                    peer_team: Some("team".into())
                }
            )
            .await
            .is_err());
    }
}
