//! Activate durable team work through the one registered managed-job path.
use super::*;
use crate::errors::AppError;
use crate::resources::activation::resource_error;
use tetonic_memory::TeamWorkItem;

/// Host selectors for launching an existing work item. Request identity comes
/// from the work row so retries stay on the same managed activation.
pub struct TeamWorkLaunch {
    pub organization_id: String,
    pub team_id: String,
    pub work_id: String,
    pub information_context_id: String,
    pub agent_key: String,
    pub definition_digest: String,
    pub execution_grant_id: String,
    /// When absent, the work title is the job input.
    pub input: Option<String>,
    pub recovery_id: String,
}

impl crate::Application {
    /// Submit through `submit_registered_job`, then bind the resulting run/attempt
    /// onto the work item. Delegated work requires the separate host entry with
    /// a live parent proof; it may never fall back to an independent root.
    pub async fn activate_team_work(
        &self,
        credential: &str,
        verifier: Arc<dyn CredentialVerifier>,
        launch: TeamWorkLaunch,
        settings: RegisteredExecutionSettings,
    ) -> Result<(TeamWorkItem, RegisteredAgentSubmission), AppError> {
        self.activate_team_work_with_parent(credential, verifier, launch, settings, None)
            .await
    }

    /// Trusted host entry for a funded child, using the existing managed task
    /// runner, provider broker and usage ledger. The parent proof is not an API ID.
    pub async fn activate_delegated_team_work(
        &self,
        credential: &str,
        verifier: Arc<dyn CredentialVerifier>,
        launch: TeamWorkLaunch,
        settings: RegisteredExecutionSettings,
        parent: tetonic_run::managed::DelegationParent,
    ) -> Result<(TeamWorkItem, RegisteredAgentSubmission), AppError> {
        self.activate_team_work_with_parent(credential, verifier, launch, settings, Some(parent))
            .await
    }

    async fn activate_team_work_with_parent(
        &self,
        credential: &str,
        verifier: Arc<dyn CredentialVerifier>,
        launch: TeamWorkLaunch,
        settings: RegisteredExecutionSettings,
        parent: Option<tetonic_run::managed::DelegationParent>,
    ) -> Result<(TeamWorkItem, RegisteredAgentSubmission), AppError> {
        let store = self
            .run_manager
            .managed()
            .store()
            .cloned()
            .ok_or_else(|| resource_error(ResourceError::StorageRequired))?;
        let resources = ResourceService {
            store: store.clone(),
            authority: Arc::new(membership::MembershipAuthority {
                store: store.clone(),
                verifier: verifier.clone(),
            }),
        };
        let work = resources
            .get_team_work_item(
                credential,
                launch.organization_id.clone(),
                launch.team_id.clone(),
                launch.work_id.clone(),
            )
            .await
            .map_err(resource_error)?
            .ok_or_else(|| AppError::InvalidRequest("team work item not found".into()))?;
        if work.status != "open" && work.status != "parked" && work.status != "running" {
            return Err(AppError::InvalidRequest(
                "team work item is not activatable".into(),
            ));
        }
        let org_check = launch.organization_id.clone();
        let team_check = launch.team_id.clone();
        let work_check = launch.work_id.clone();
        let goal_check = work.goal_id.clone();
        let agent_check = launch.agent_key.clone();
        if let Some(stop) = store
            .read(move |db| {
                db.activation_blocked_by_stop(
                    &org_check,
                    &team_check,
                    &work_check,
                    goal_check.as_deref(),
                    Some(&agent_check),
                )
            })
            .await
            .map_err(|_| resource_error(ResourceError::Storage))?
            .map_err(|e| resource_error(e.into()))?
        {
            return Err(AppError::PolicyDenied(format!(
                "activation blocked by {} stop on {}/{}",
                stop.mode, stop.scope_kind, stop.scope_id
            )));
        }
        let org_pin = launch.organization_id.clone();
        let team_pin = launch.team_id.clone();
        let work_pin = launch.work_id.clone();
        if let Some(reason) = store
            .read(move |db| db.placement_blocks_activation(&org_pin, &team_pin, &work_pin))
            .await
            .map_err(|_| resource_error(ResourceError::Storage))?
            .map_err(|e| resource_error(e.into()))?
        {
            return Err(AppError::PolicyDenied(reason));
        }
        let org = launch.organization_id.clone();
        let team = launch.team_id.clone();
        let work_id = launch.work_id.clone();
        let (delegation, parent_parked) = store
            .read(move |db| {
                let delegation = db.work_delegation_for_child(&org, &team, &work_id)?;
                let parent_parked = match &delegation {
                    Some(d) => db
                        .get_team_work_item(&org, &team, &d.parent_work_id)?
                        .is_some_and(|p| p.status == "parked"),
                    None => false,
                };
                Ok::<_, tetonic_memory::StoreError>((delegation, parent_parked))
            })
            .await
            .map_err(|_| resource_error(ResourceError::Storage))?
            .map_err(|e| resource_error(e.into()))?;
        if parent_parked {
            return Err(AppError::PolicyDenied(
                "parent work is parked; child activation denied".into(),
            ));
        }
        if delegation.is_some() && parent.is_none() {
            return Err(AppError::PolicyDenied(
                "Governed delegation requires a live parent. A delegated work item cannot launch as an independent root run.".into(),
            ));
        }
        if parent.is_some() && delegation.is_none() {
            return Err(AppError::PolicyDenied(
                "Delegated execution requires an allocated child work item.".into(),
            ));
        }
        // Durable retries share the work request id with the managed activation key.
        let request_id = sanitize_activation_request_id(&work.request_id)?;
        let input = launch.input.unwrap_or_else(|| work.title.clone());
        let submission = self
            .submit_registered_job_for_work(
                credential,
                verifier,
                RegisteredAgentJob {
                    request_id,
                    organization_id: launch.organization_id.clone(),
                    information_context_id: launch.information_context_id,
                    agent_key: launch.agent_key,
                    definition_digest: launch.definition_digest,
                    execution_grant_id: launch.execution_grant_id,
                    input,
                    recovery_id: launch.recovery_id,
                },
                settings,
                Some((launch.team_id.clone(), launch.work_id.clone())),
                parent,
            )
            .await?;
        let attempt_id = match &submission.execution {
            Some(execution) => execution.attempt_id.0.clone(),
            None => {
                // Receipts locate durable tasks; they never invent attempt IDs or
                // resume a partially admitted task after a lost response.
                let run = submission.run_id.0.clone();
                let task = submission.task_id.clone();
                store.read(move |db| {
                    let snapshot=db.load_run_snapshot(&run)?.ok_or(tetonic_memory::StoreError::ControlAccessDenied)?;
                    Ok::<_,tetonic_memory::StoreError>(snapshot.attempts.values().find(|attempt|attempt.task_id==task).map(|attempt|attempt.attempt_id.0.clone()))
                }).await.map_err(|_|resource_error(ResourceError::Storage))?
                    .map_err(|_|resource_error(ResourceError::Storage))?
                    .ok_or_else(||AppError::InvalidRequest("Activation has an incomplete admission; inspect the run before retrying.".into()))?
            }
        };
        let bound = resources
            .activate_team_work_item(
                credential,
                launch.organization_id.clone(),
                launch.team_id.clone(),
                launch.work_id.clone(),
                attempt_id,
                submission.run_id.0.clone(),
            )
            .await;
        let bound = match bound {
            Ok(bound) => bound,
            Err(error) => {
                // A stop or permission change can win after managed admission.
                // Do not strand the fresh execution when its work binding loses.
                if submission.execution.is_some()
                    && self
                        .run_manager
                        .managed()
                        .cancel_run(&submission.run_id)
                        .await
                        .is_err()
                {
                    let run = submission.run_id.0.clone();
                    let org = launch.organization_id;
                    let team = launch.team_id;
                    let goal = work.goal_id.clone();
                    let work = launch.work_id;
                    let _ = store
                        .write(move |db| {
                            if let Some(stop) = db.activation_blocked_by_stop(
                                &org,
                                &team,
                                &work,
                                goal.as_deref(),
                                None,
                            )? {
                                db.record_unresolved_stop_effect(
                                    &org,
                                    &stop.scope_kind,
                                    &stop.scope_id,
                                    stop.generation,
                                    &run,
                                    "work binding lost; managed cancellation unconfirmed",
                                )?;
                            }
                            Ok::<_, tetonic_memory::StoreError>(())
                        })
                        .await;
                    return Err(AppError::InvalidRequest(
                            "Work could not be activated; cancellation is unconfirmed. Inspect the managed run before retrying.".into(),
                        ));
                }
                return Err(resource_error(error));
            }
        };
        Ok((bound, submission))
    }

    /// Record a hierarchical stop, park matching work, and cancel known managed runs.
    /// Unreachable cancellation targets are recorded as unresolved effects.
    pub async fn apply_control_stop(
        &self,
        credential: &str,
        verifier: Arc<dyn CredentialVerifier>,
        command: crate::resources::ApplyControlStop,
    ) -> Result<tetonic_memory::ControlStop, AppError> {
        let crate::resources::ApplyControlStop {
            org,
            scope_kind,
            scope_id,
            mode,
            reason,
        } = command;
        let store = self
            .run_manager
            .managed()
            .store()
            .cloned()
            .ok_or_else(|| resource_error(ResourceError::StorageRequired))?;
        let resources = ResourceService {
            store: store.clone(),
            authority: Arc::new(membership::MembershipAuthority {
                store: store.clone(),
                verifier,
            }),
        };
        let (stop, run_ids) = resources
            .request_control_stop_with_runs(
                credential,
                org.clone(),
                scope_kind.clone(),
                scope_id.clone(),
                mode.clone(),
                reason,
            )
            .await
            .map_err(resource_error)?;
        if matches!(mode.as_str(), "cancel" | "estop") {
            for run_id in run_ids {
                match self
                    .run_manager
                    .managed()
                    .cancel_run(&tetonic_domain::RunId::new(run_id.clone()))
                    .await
                {
                    Ok(()) => {}
                    Err(_) => {
                        let org = org.clone();
                        let kind = scope_kind.clone();
                        let sid = scope_id.clone();
                        let generation = stop.generation;
                        let effect = run_id.clone();
                        let _ = store
                            .write(move |db| {
                                db.record_unresolved_stop_effect(
                                    &org,
                                    &kind,
                                    &sid,
                                    generation,
                                    &effect,
                                    "managed cancel did not confirm quiescence",
                                )
                            })
                            .await;
                    }
                }
            }
        }
        Ok(stop)
    }
}

fn sanitize_activation_request_id(request_id: &str) -> Result<String, AppError> {
    Ok(tetonic_memory::work_activation_request_id(request_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::{HarnessPreparationLimits, LocalControl, RegisteredExecutionSettings};
    use crate::{Application, ComputePlaneRequest};
    use tetonic_domain::ExecutionScope;
    use tetonic_memory::ContextOwner;

    #[tokio::test]
    async fn activate_team_work_binds_root_and_denies_ungoverned_child_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let database = dir.path().join("control.db");
        let local = LocalControl::open(database.clone(), "test".into())
            .await
            .unwrap();
        local
            .bootstrap("admin".into(), "org".into(), "Org".into())
            .await
            .unwrap();
        let credential = local
            .credentials()
            .issue("admin".into(), 3600)
            .await
            .unwrap();
        let secret = credential.expose_secret();
        let resources = local.resources();
        resources
            .create_team(secret, "org".into(), "team".into(), "Team".into())
            .await
            .unwrap();
        let contexts = local.contexts();
        contexts
            .create(
                secret,
                "shared".into(),
                ContextOwner::Team {
                    org_id: "org".into(),
                    team_id: "team".into(),
                },
            )
            .await
            .unwrap();
        let registered = resources
            .register_agent(
                secret,
                "org".into(),
                "agent".into(),
                "general".into(),
                serde_json::json!({
                    "instructions":"Finish the assigned work",
                    "requested_tools":["finish"],
                    "max_steps":2
                }),
            )
            .await
            .unwrap();
        let limits = || HarnessPreparationLimits {
            human_handoff: false,
            max_steps: 2,
            max_input_bytes: 1024,
        };
        let prepared = resources
            .prepare_general_revision(
                secret,
                "org".into(),
                "agent".into(),
                registered.identity.bound_definition_digest.clone(),
                "Ship preview".into(),
                limits(),
            )
            .await
            .unwrap();
        let command = prepared.start_command("job".into()).unwrap();
        resources
            .issue_execution_grant(
                secret,
                tetonic_memory::ExecutionGrant {
                    grant_id: "grant".into(),
                    scope: ExecutionScope {
                        principal_id: "admin".into(),
                        organization_id: "org".into(),
                        information_context_id: "shared".into(),
                    },
                    job: command.job_spec,
                    expires_at: chrono::Utc::now().timestamp() + 3600,
                },
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
                    title: "Ship preview".into(),
                    request_id: "work-req-1".into(),
                    goal_id: None,
                },
            )
            .await
            .unwrap();
        resources
            .authorize_work_budget(
                secret,
                "org".into(),
                "team".into(),
                "w1".into(),
                "budget-w1".into(),
                100,
            )
            .await
            .unwrap();
        resources
            .create_work_delegation(
                secret,
                crate::resources::CreateWorkDelegation {
                    org: "org".into(),
                    team: "team".into(),
                    delegation_id: "d1".into(),
                    parent_work_id: "w1".into(),
                    child_work_id: "child".into(),
                    child_title: "Help".into(),
                    request_id: "del-1".into(),
                    parent_budget_tokens: 100,
                    child_budget_tokens: 25,
                    stop_scope: "inherit".into(),
                    peer_org: None,
                    peer_team: None,
                },
            )
            .await
            .unwrap();
        resources
            .park_team_work_item(secret, "org".into(), "team".into(), "w1".into())
            .await
            .unwrap();
        let store = tetonic_memory::SharedStore::open(database, 1).unwrap();
        let (sink, _) = crate::events::RecordingEventSink::new();
        let app = Application::bootstrap_mock_with_store(dir.path(), Some(store), sink, vec![]);
        let (url, _, server) = crate::tui_mvp_tests::inference_server_with_behavior(
            true,
            "finish",
            serde_json::json!({}),
            false,
        )
        .await;
        let guard = Arc::new(tetonic_egress::EgressGuard::new());
        guard.configure_loopback_inference(url.rsplit(':').next().unwrap().parse().unwrap());
        let plane = crate::build_compute_plane(ComputePlaneRequest {
            guard,
            ollama_base: url,
            policy: app.turn.runtime.policy().clone(),
            workspace_root: workspace.clone(),
            artifact_store: app.turn.runtime.artifact_store().clone(),
            store: app.run_manager.managed().store().cloned(),
            coordinator: None,
            placement_sink: None,
            previous_pooled: None,
        })
        .await;
        app.install_compute_services(&plane);
        let settings = || RegisteredExecutionSettings {
            plan_dispatch: None,
            response_schema: None,
            hosted: None,
            max_elapsed_seconds: 30,
            reported_token_ceiling: Some(200),
            workspace_root: None,
            model: "qwen3.5:latest".into(),
            num_ctx: 8192,
            data_class: tetonic_domain::DataClass::RepositorySource,
            allowed_tools: ["finish".into()].into_iter().collect(),
            limits: limits(),
        };
        assert!(
            app.activate_team_work(
                secret,
                local.credentials().clone(),
                TeamWorkLaunch {
                    organization_id: "org".into(),
                    team_id: "team".into(),
                    work_id: "child".into(),
                    information_context_id: "shared".into(),
                    agent_key: "agent".into(),
                    definition_digest: registered.identity.bound_definition_digest.clone(),
                    execution_grant_id: "grant".into(),
                    input: None,
                    recovery_id: "job-child".into(),
                },
                settings(),
            )
            .await
            .is_err(),
            "parked parent must block child activation"
        );
        resources
            .resume_team_work_item(secret, "org".into(), "team".into(), "w1".into())
            .await
            .unwrap();
        let (bound, submission) = tokio::task::LocalSet::new()
            .run_until(async {
                let error=app.activate_team_work(secret,local.credentials().clone(),TeamWorkLaunch {
                    organization_id:"org".into(),team_id:"team".into(),work_id:"child".into(),information_context_id:"shared".into(),agent_key:"agent".into(),definition_digest:registered.identity.bound_definition_digest.clone(),execution_grant_id:"grant".into(),input:None,recovery_id:"child-bypass".into()
                },settings()).await.err().expect("an unparked child must not become an independent root run");
                assert!(matches!(error,AppError::PolicyDenied(ref reason) if reason.contains("Governed delegation")));
                assert!(resources.get_team_work_item(secret,"org".into(),"team".into(),"child".into()).await.unwrap().unwrap().run_id.is_none());
                // Force a stop after durable admission but before the work bind.
                // The fresh attempt must be canceled, releasing this agent's slot
                // so the unrelated root submission below can still proceed.
                resources.create_team_work_item(secret, crate::resources::CreateTeamWorkItem { org: "org".into(), team: "team".into(), work_id: "racing".into(), title: "Ship preview".into(), request_id: "racing-request".into(), goal_id: None }).await.unwrap();
                let paused = Arc::new(tokio::sync::Notify::new());
                let resume = Arc::new(tokio::sync::Notify::new());
                app.run_manager.managed().set_post_admission_hook(paused.clone(), resume.clone());
                let racing = app.activate_team_work(secret, local.credentials().clone(), TeamWorkLaunch {
                    organization_id: "org".into(), team_id: "team".into(), work_id: "racing".into(),
                    information_context_id: "shared".into(), agent_key: "agent".into(),
                    definition_digest: registered.identity.bound_definition_digest.clone(),
                    execution_grant_id: "grant".into(), input: None, recovery_id: "job".into(),
                }, settings());
                let stop = async {
                    paused.notified().await;
                    resources.request_control_stop(secret, "org".into(), "work".into(),
                        "racing".into(), "cancel".into(), "Stop before binding".into()).await.unwrap();
                    resume.notify_one();
                };
                let (raced, ()) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
                    tokio::join!(racing, stop)
                }).await.expect("admission/stop race must finish");
                assert!(raced.is_err());
                let parked = resources.get_team_work_item(secret, "org".into(), "team".into(), "racing".into()).await.unwrap().unwrap();
                assert_eq!(parked.status, "parked");
                assert!(parked.run_id.is_none());
                resume.notify_one(); // Let the normal root admission pass the test hook.
                app.activate_team_work(
                    secret,
                    local.credentials().clone(),
                    TeamWorkLaunch {
                        organization_id: "org".into(),
                        team_id: "team".into(),
                        work_id: "w1".into(),
                        information_context_id: "shared".into(),
                        agent_key: "agent".into(),
                        definition_digest: registered.identity.bound_definition_digest.clone(),
                        execution_grant_id: "grant".into(),
                        input: None,
                        recovery_id: "job".into(),
                    },
                    settings(),
                )
                .await
                .unwrap()
            })
            .await;
        assert_eq!(bound.status, "running");
        assert!(bound.attempt_id.is_some());
        assert_eq!(bound.run_id.as_deref(), Some(submission.run_id.0.as_str()));
        resources
            .request_control_stop(
                secret,
                "org".into(),
                "team".into(),
                "team".into(),
                "estop".into(),
                "halt".into(),
            )
            .await
            .unwrap();
        assert!(
            app.activate_team_work(
                secret,
                local.credentials().clone(),
                TeamWorkLaunch {
                    organization_id: "org".into(),
                    team_id: "team".into(),
                    work_id: "child".into(),
                    information_context_id: "shared".into(),
                    agent_key: "agent".into(),
                    definition_digest: registered.identity.bound_definition_digest.clone(),
                    execution_grant_id: "grant".into(),
                    input: None,
                    recovery_id: "job-child-2".into(),
                },
                settings(),
            )
            .await
            .is_err(),
            "team estop must block new activation"
        );
        drop(server);
    }
}
