//! Authorized snapshot inspection over the existing durable run supervisor.
use super::*;
use tetonic_domain::{RunId, RunSnapshot};
use tetonic_run::RunSupervisor;

pub enum RunPoll {
    Events(Vec<tetonic_domain::RunEventEnvelope>),
    Gap(tetonic_domain::ReplayGap),
    CaughtUp,
}

impl ContextService {
    /// Content membership permits inspection, independently of execution grants.
    /// Legacy and mixed-context runs are not exposed through this employee door.
    pub async fn inspect_run(
        &self,
        credential: &str,
        organization: String,
        context: String,
        run: String,
    ) -> Result<RunSnapshot, ResourceError> {
        let principal = self.verifier.verify(credential).await?;
        let actor = principal.principal_id;
        self.authorize_run_context(&actor, &organization, &context)
            .await?;
        let supervisor = tetonic_run::DurableRunSupervisor::new(Some(self.store.clone()));
        let snapshot = supervisor
            .snapshot(RunId::new(run))
            .await
            .map_err(|_| ResourceError::Denied)?;
        if snapshot.tasks.is_empty()
            || snapshot.tasks.values().any(|task| {
                !task.binding.execution_scope.as_ref().is_some_and(|scope| {
                    scope.organization_id == organization && scope.information_context_id == context
                })
            })
        {
            return Err(ResourceError::Denied);
        }
        // Recheck after storage access before releasing content. These are
        // snapshot checks, not an atomic revocation fence over response delivery.
        if self.verifier.verify(credential).await?.principal_id != actor {
            return Err(ResourceError::Denied);
        }
        self.authorize_run_context(&actor, &organization, &context)
            .await?;
        Ok(snapshot)
    }

    /// Replay the existing durable lifecycle journal; this is not model-token streaming.
    /// Returns the supervisor's retention gap unchanged, never inventing missing events.
    pub async fn replay_run(
        &self,
        credential: &str,
        organization: String,
        context: String,
        run: String,
        after: u64,
        limit: u32,
    ) -> Result<
        Result<Vec<tetonic_domain::RunEventEnvelope>, tetonic_domain::ReplayGap>,
        ResourceError,
    > {
        if !(1..=1000).contains(&limit) {
            return Err(ResourceError::Invalid);
        }
        self.inspect_run(
            credential,
            organization.clone(),
            context.clone(),
            run.clone(),
        )
        .await?;
        let supervisor = tetonic_run::DurableRunSupervisor::new(Some(self.store.clone()));
        let replay = supervisor
            .resume_from_sequence(RunId::new(run.clone()), after, limit)
            .await
            .map_err(|_| ResourceError::Denied)?;
        // A task added during the read may introduce another scope. Revalidate
        // the entire run before returning any event or retention metadata.
        self.inspect_run(credential, organization, context, run)
            .await?;
        Ok(replay)
    }

    /// One authorized read of new durable events. Membership is checked by
    /// `replay_run` before any event is returned. An empty batch on a terminal
    /// run is caught up. This is not an unrestricted live fanout.
    pub async fn poll_run(
        &self,
        credential: &str,
        organization: String,
        context: String,
        run: String,
        after: u64,
        limit: u32,
    ) -> Result<RunPoll, ResourceError> {
        let snapshot = self
            .inspect_run(
                credential,
                organization.clone(),
                context.clone(),
                run.clone(),
            )
            .await?;
        let replay = self
            .replay_run(credential, organization, context, run, after, limit)
            .await?;
        match replay {
            Err(gap) => Ok(RunPoll::Gap(gap)),
            Ok(events)
                if events.is_empty()
                    && matches!(
                        snapshot.state,
                        tetonic_domain::RunState::Succeeded
                            | tetonic_domain::RunState::Failed
                            | tetonic_domain::RunState::Canceled
                    ) =>
            {
                Ok(RunPoll::CaughtUp)
            }
            Ok(events) => Ok(RunPoll::Events(events)),
        }
    }

    async fn authorize_run_context(
        &self,
        actor: &str,
        organization: &str,
        context: &str,
    ) -> Result<(), ResourceError> {
        let (actor, organization, context) = (
            actor.to_owned(),
            organization.to_owned(),
            context.to_owned(),
        );
        let allowed = self
            .store
            .read(move |db| db.context_access_in_organization(&actor, &context, &organization))
            .await
            .map_err(|_| ResourceError::Denied)?
            .map_err(|_| ResourceError::Denied)?;
        if allowed {
            Ok(())
        } else {
            Err(ResourceError::Denied)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn inspection_denies_legacy_and_mixed_context_runs() {
        let dir = tempfile::tempdir().unwrap();
        let local = LocalControl::open(dir.path().join("db"), "test".into())
            .await
            .unwrap();
        local
            .bootstrap("alice".into(), "org".into(), "Org".into())
            .await
            .unwrap();
        let credential = local
            .credentials()
            .issue("alice".into(), 3600)
            .await
            .unwrap();
        let contexts = local.contexts();
        contexts
            .create(
                credential.expose_secret(),
                "private".into(),
                ContextOwner::Private {
                    org_id: "org".into(),
                },
            )
            .await
            .unwrap();
        let supervisor = tetonic_run::DurableRunSupervisor::new(Some(contexts.store.clone()));
        for scoped in [false, true] {
            let run = RunId::new(if scoped { "mixed" } else { "legacy" });
            let scope = scoped.then(|| tetonic_domain::ExecutionScope {
                principal_id: "alice".into(),
                organization_id: "org".into(),
                information_context_id: "private".into(),
            });
            supervisor
                .handle(tetonic_domain::RunCommand::CreateRun(
                    tetonic_domain::CreateRun {
                        envelope: tetonic_run::command_envelope("create", None, "test"),
                        session_id: None,
                        run_id: run.clone(),
                        root_task_id: tetonic_domain::TaskId::new("root"),
                        root_binding: tetonic_domain::TaskInputBinding {
                            execution_scope: scope,
                            ..Default::default()
                        },
                        speculation: None,
                        job_spec: None,
                    },
                ))
                .await
                .unwrap();
            if scoped {
                assert!(contexts
                    .inspect_run(
                        credential.expose_secret(),
                        "org".into(),
                        "private".into(),
                        run.0.clone()
                    )
                    .await
                    .is_ok());
                supervisor
                    .handle(tetonic_domain::RunCommand::AddTask(
                        tetonic_domain::AddTask {
                            envelope: tetonic_run::command_envelope("add", None, "test"),
                            run_id: run.clone(),
                            task_id: tetonic_domain::TaskId::new("foreign"),
                            binding: Default::default(),
                        },
                    ))
                    .await
                    .unwrap();
            }
            assert!(contexts
                .inspect_run(
                    credential.expose_secret(),
                    "org".into(),
                    "private".into(),
                    run.0
                )
            .await
            .is_err());
        }
    }

    #[tokio::test]
    async fn poll_run_stops_when_the_credential_is_revoked() {
        let dir = tempfile::tempdir().unwrap();
        let local = LocalControl::open(dir.path().join("db"), "test".into())
            .await
            .unwrap();
        local
            .bootstrap("alice".into(), "org".into(), "Org".into())
            .await
            .unwrap();
        let credential = local
            .credentials()
            .issue("alice".into(), 3600)
            .await
            .unwrap();
        let contexts = local.contexts();
        contexts
            .create(
                credential.expose_secret(),
                "private".into(),
                ContextOwner::Private {
                    org_id: "org".into(),
                },
            )
            .await
            .unwrap();
        let supervisor = tetonic_run::DurableRunSupervisor::new(Some(contexts.store.clone()));
        supervisor
            .handle(tetonic_domain::RunCommand::CreateRun(
                tetonic_domain::CreateRun {
                    envelope: tetonic_run::command_envelope("create", None, "test"),
                    session_id: None,
                    run_id: RunId::new("followed"),
                    root_task_id: tetonic_domain::TaskId::new("root"),
                    root_binding: tetonic_domain::TaskInputBinding {
                        execution_scope: Some(tetonic_domain::ExecutionScope {
                            principal_id: "alice".into(),
                            organization_id: "org".into(),
                            information_context_id: "private".into(),
                        }),
                        ..Default::default()
                    },
                    speculation: None,
                    job_spec: None,
                },
            ))
            .await
            .unwrap();
        let first = contexts
            .poll_run(
                credential.expose_secret(),
                "org".into(),
                "private".into(),
                "followed".into(),
                0,
                20,
            )
            .await
            .unwrap();
        assert!(matches!(first, RunPoll::Events(events) if !events.is_empty()));
        local
            .credentials()
            .revoke(credential.credential_id.clone())
            .await
            .unwrap();
        assert!(matches!(
            contexts
                .poll_run(
                    credential.expose_secret(),
                    "org".into(),
                    "private".into(),
                    "followed".into(),
                    0,
                    20,
                )
                .await,
            Err(ResourceError::Denied)
        ));
    }
}
