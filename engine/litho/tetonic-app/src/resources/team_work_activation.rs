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
    /// onto the work item. Child delegations inherit a reported-token ceiling from
    /// the stored child budget and cannot activate while the parent is parked.
    pub async fn activate_team_work(
        &self,
        credential: &str,
        verifier: Arc<dyn CredentialVerifier>,
        launch: TeamWorkLaunch,
        mut settings: RegisteredExecutionSettings,
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
        if work.status == "running" {
            if let (Some(_attempt), Some(run)) = (&work.attempt_id, &work.run_id) {
                return Ok((
                    work.clone(),
                    RegisteredAgentSubmission {
                        run_id: tetonic_domain::RunId::new(run.clone()),
                        task_id: tetonic_domain::TaskId::new(format!("task_root_{run}")),
                        audit_session_id: String::new(),
                        execution: None,
                    },
                ));
            }
        }
        if work.status != "open" && work.status != "parked" && work.status != "running" {
            return Err(AppError::InvalidRequest(
                "team work item is not activatable".into(),
            ));
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
        if let Some(delegation) = &delegation {
            let child_ceiling = u64::try_from(delegation.child_budget_tokens).unwrap_or(0);
            settings.reported_token_ceiling = Some(match settings.reported_token_ceiling {
                Some(host) => host.min(child_ceiling),
                None => child_ceiling,
            });
        }
        // Durable retries share the work request id with the managed activation key.
        let request_id = sanitize_activation_request_id(&work.request_id)?;
        let input = launch
            .input
            .unwrap_or_else(|| work.title.clone());
        let submission = self
            .submit_registered_job(
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
            )
            .await?;
        let attempt_id = match &submission.execution {
            Some(execution) => execution.attempt_id.0.clone(),
            None => {
                // Existing activation: prefer a prior bind, else the root attempt
                // naming convention used by managed activation.
                work.attempt_id.clone().unwrap_or_else(|| {
                    format!("attempt_root_{}", submission.run_id)
                })
            }
        };
        let bound = resources
            .activate_team_work_item(
                credential,
                launch.organization_id,
                launch.team_id,
                launch.work_id,
                attempt_id,
                submission.run_id.0.clone(),
            )
            .await
            .map_err(resource_error)?;
        Ok((bound, submission))
    }
}

fn sanitize_activation_request_id(request_id: &str) -> Result<String, AppError> {
    if request_id.is_empty()
        || request_id.len() > 128
        || !request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        // Work request ids may include '/', which activation keys reject.
        let digest = {
            use sha2::Digest;
            format!(
                "tw_{:x}",
                sha2::Sha256::digest(request_id.as_bytes())
            )
        };
        return Ok(digest.chars().take(128).collect());
    }
    Ok(request_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::{
        HarnessPreparationLimits, LocalControl, RegisteredExecutionSettings,
    };
    use crate::{Application, ComputePlaneRequest};
    use tetonic_domain::ExecutionScope;
    use tetonic_memory::ContextOwner;

    #[tokio::test]
    async fn activate_team_work_binds_managed_run_and_respects_delegation_ceiling() {
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
                "org".into(),
                "team".into(),
                "w1".into(),
                "Ship preview".into(),
                "work-req-1".into(),
                None,
            )
            .await
            .unwrap();
        resources
            .create_work_delegation(
                secret,
                "org".into(),
                "team".into(),
                "d1".into(),
                "w1".into(),
                "child".into(),
                "Help".into(),
                "del-1".into(),
                100,
                25,
                "inherit".into(),
                None,
                None,
            )
            .await
            .unwrap();
        resources
            .park_team_work_item(secret, "org".into(), "team".into(), "w1".into())
            .await
            .unwrap();
        let store = tetonic_memory::SharedStore::open(database, 1).unwrap();
        let (sink, _) = crate::events::RecordingEventSink::new();
        let app =
            Application::bootstrap_mock_with_store(dir.path(), Some(store), sink, vec![]);
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
        drop(server);
    }
}

