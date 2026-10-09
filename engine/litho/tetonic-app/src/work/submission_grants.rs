//! Refresh preparation only before admission; a retry never changes an admitted job.
use super::*;

impl WorkService {
    pub(super) async fn submission_grant(
        &self,
        id: &str,
        request: &str,
        job: tetonic_domain::AgentJobSpec,
    ) -> Result<String, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        let scope = ExecutionScope {
            principal_id: app_scope.principal().into(),
            organization_id: app_scope.organization().into(),
            information_context_id: self.services.scope.context().to_owned(),
        };
        let resources = self.services.local.resources();
        let base_id = format!("local-ui-{id}");
        let previous = resources
            .get_execution_grant(
                &self.services.host.credential,
                app_scope.organization().into(),
                base_id.clone(),
            )
            .await
            .map_err(resource)?;
        let now = chrono::Utc::now().timestamp();
        let grant_id = match previous {
            Some(previous)
                if previous.scope == scope && previous.job == job && previous.expires_at > now =>
            {
                return Ok(base_id)
            }
            Some(previous) => {
                if previous.scope != scope {
                    return Err(AppError::PolicyDenied(
                        "This request belongs to a different execution scope.".into(),
                    ));
                }
                let run = tetonic_run::managed::activation::activation_run_id(
                    &scope,
                    &tetonic_memory::work_activation_request_id(request),
                )?;
                let admitted = self
                    .services
                    .local
                    .store()
                    .read(move |db| db.load_run_snapshot(&run.0))
                    .await
                    .map_err(|_| AppError::InferenceUnavailable)?
                    .map_err(|e| resource(e.into()))?
                    .is_some();
                if admitted {
                    return Err(AppError::Conflict("This message already has an admitted execution. Inspect its recorded state before retrying; it has not been started again.".into()));
                }
                // Failed admission saved a grant for an older observation/model.
                // This explicit owner retry rechecks current authority and grants
                // the newly prepared input. Never overwrite the immutable grant.
                format!("{base_id}/{}", uuid::Uuid::new_v4())
            }
            // A revoked grant is hidden by the read; issuing the same ID below
            // fails closed instead of silently replacing revoked permission.
            None => base_id,
        };
        resources
            .issue_execution_grant(
                &self.services.host.credential,
                tetonic_memory::ExecutionGrant {
                    grant_id: grant_id.clone(),
                    scope,
                    job,
                    expires_at: now + 3600,
                },
            )
            .await
            .map_err(resource)?;
        Ok(grant_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn guide_retry_refreshes_unadmitted_observation_but_never_replays_admitted_work() {
        let dir = tempfile::tempdir().unwrap();
        let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(
            true,
            "finish",
            serde_json::json!({"summary":"Ready to discuss."}),
            false,
        )
        .await;
        tokio::task::LocalSet::new()
            .run_until(async {
                let workspace =
                    LocalWorkspace::open(dir.path().join("retry.db"), "qwen3.5:latest".into(), url)
                        .await
                        .unwrap();
                let id = uuid::Uuid::new_v4().to_string();
                let request = format!("{id}@{}", shaping::GUIDE);
                let registered = workspace
                    .services
                    .registered_agent(shaping::GUIDE)
                    .await
                    .unwrap();
                let prepared = workspace
                    .services
                    .local
                    .resources()
                    .prepare_general_revision(
                        &workspace.services.host.credential,
                        ORG.into(),
                        shaping::GUIDE.into(),
                        registered.identity.bound_definition_digest,
                        "Earlier workspace observation".into(),
                        crate::resources::HarnessPreparationLimits {
                            work_director: true,
                            human_handoff: false,
                            max_steps: 32,
                            max_input_bytes: 30_000,
                        },
                    )
                    .await
                    .unwrap();
                let old_job = prepared.start_command(id.clone()).unwrap().job_spec;
                let base = workspace
                    .submission_grant(&id, &request, old_job.clone())
                    .await
                    .unwrap();
                let started = workspace
                    .submit_with_purpose(
                        id.clone(),
                        "Help me think.".into(),
                        shaping::GUIDE.into(),
                        None,
                        WorkPurpose::Explore,
                    )
                    .await
                    .unwrap();
                assert!(started.run_id.is_some());
                tokio::time::timeout(std::time::Duration::from_secs(20), async {
                    while workspace.task(&id).await.unwrap().state != "completed" {
                        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    }
                })
                .await
                .unwrap();
                let old = workspace
                    .services
                    .local
                    .resources()
                    .get_execution_grant(&workspace.services.host.credential, ORG.into(), base)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(old.job, old_job, "immutable authority is not overwritten");
                let mut changed = old_job;
                changed.input_digest = "changed-again".into();
                assert!(
                    workspace
                        .submission_grant(&id, &request, changed)
                        .await
                        .is_err(),
                    "a durable admission may never be replaced"
                );
                let retry = workspace
                    .submit_with_purpose(
                        id.clone(),
                        "Help me think.".into(),
                        shaping::GUIDE.into(),
                        None,
                        WorkPurpose::Explore,
                    )
                    .await
                    .unwrap();
                assert_eq!(retry.run_id, started.run_id);
                assert_eq!(calls.lock().unwrap().len(), 1);

                let revoked_id = uuid::Uuid::new_v4().to_string();
                let mut revoked_job = old.job;
                revoked_job.recovery_id = revoked_id.clone();
                let revoked_request = format!("{revoked_id}@{}", shaping::GUIDE);
                let grant = workspace
                    .submission_grant(&revoked_id, &revoked_request, revoked_job.clone())
                    .await
                    .unwrap();
                workspace
                    .services
                    .local
                    .resources()
                    .revoke_execution_grant(&workspace.services.host.credential, ORG.into(), grant)
                    .await
                    .unwrap();
                revoked_job.input_digest = "new-observation".into();
                assert!(
                    workspace
                        .submission_grant(&revoked_id, &revoked_request, revoked_job)
                        .await
                        .is_err(),
                    "revocation is not a retryable preparation failure"
                );
            })
            .await;
        server.abort();
    }
}
