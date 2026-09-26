//! Lokai recovery inspection and explicit abandonment; manager owns transitions.
use crate::{errors::AppError, Application};
use tetonic_domain::{RunId, RunState};

fn run_carries_execution_scope(snapshot: &tetonic_domain::RunSnapshot) -> bool {
    snapshot
        .tasks
        .values()
        .any(|task| task.binding.execution_scope.is_some())
}

impl Application {
    /// The local daemon has no employee credential. Scoped runs stay on the
    /// credentialed control path. Missing and scoped ids use the same text.
    pub async fn unscoped_daemon_inspect(
        &self,
        run_id: &str,
    ) -> Result<tetonic_domain::RunSnapshot, AppError> {
        let snapshot = self
            .runs
            .inspect_run(crate::commands::InspectRunCommand {
                run_id: run_id.to_string(),
            })
            .await?;
        if run_carries_execution_scope(&snapshot) {
            return Err(AppError::InvalidRequest(format!("run not found: {run_id}")));
        }
        Ok(snapshot)
    }

    pub async fn unscoped_daemon_resume(
        &self,
        run_id: &str,
        after_sequence: u64,
        limit: Option<u32>,
    ) -> Result<
        Result<Vec<tetonic_domain::RunEventEnvelope>, tetonic_domain::ReplayGap>,
        AppError,
    > {
        self.unscoped_daemon_inspect(run_id).await?;
        let replay = self
            .runs
            .resume_events(crate::commands::ResumeRunEventsCommand {
                run_id: run_id.to_string(),
                after_sequence,
                limit,
            })
            .await?;
        self.unscoped_daemon_inspect(run_id).await?;
        Ok(replay)
    }

    pub async fn unscoped_daemon_cancel(&self, run_id: &str) -> Result<(), AppError> {
        self.unscoped_daemon_inspect(run_id).await?;
        self.runs
            .cancel_run(crate::commands::CancelByRunCommand {
                run_id: run_id.to_string(),
            })
            .await
    }

    pub async fn recovery_report(&self) -> Result<String, AppError> {
        let Some(store) = &self.turn.store else {
            return Ok("No persistent recovery state.\n".into());
        };
        let ids = store
            .read(|db| db.list_all_run_ids())
            .await
            .map_err(|e| AppError::PersistenceFailed(e.to_string()))?
            .map_err(|e| AppError::PersistenceFailed(e.to_string()))?;
        let mut report = String::new();
        if self.supervisor.is_safe_mode() {
            report.push_str(
                "Storage/migration Safe Mode is active. Abandoning a run cannot clear it.\n",
            );
        }
        for id in ids {
            let snapshot = self
                .supervisor
                .snapshot(RunId::new(&id))
                .await
                .map_err(|e| AppError::PersistenceFailed(e.to_string()))?;
            if snapshot.state != RunState::RecoveryRequired {
                continue;
            }
            // The local chat has no employee credential. A scoped hold stays
            // off this report; credentialed recovery is a separate door.
            if run_carries_execution_scope(&snapshot) {
                continue;
            }
            let claims = snapshot
                .tasks
                .values()
                .filter(|t| t.finalization_claim.is_some())
                .count();
            report.push_str(&format!("Run {id}, revision {}, session {}\n  Finalization claims: {claims}; recorded effects: {}\n",
                snapshot.sequence, snapshot.session_id.as_ref().map(|s| s.0.as_str()).unwrap_or("none"), snapshot.side_effect_commits.len()));
            for attempt in snapshot.attempts.values() {
                report.push_str(&format!("  {}: {:?}\n", attempt.attempt_id, attempt.state));
            }
            report.push_str(&format!(
                "  After reviewing workspace effects: /recovery abandon {id} {}\n",
                snapshot.sequence
            ));
        }
        if report.is_empty() {
            report.push_str("No runs require recovery.\n");
        } else {
            report.push_str("Abandon cancels further execution and preserves history. It does not undo or verify workspace changes.\n");
        }
        Ok(report)
    }

    pub async fn abandon_recovery_run(
        &self,
        run_id: &str,
        expected_sequence: u64,
    ) -> Result<String, AppError> {
        if self.supervisor.is_safe_mode() {
            return Err(AppError::InvalidRequest(
                "storage/migration Safe Mode must be resolved before abandoning a run".into(),
            ));
        }
        let id = RunId::new(run_id);
        let snapshot = self
            .supervisor
            .snapshot(id.clone())
            .await
            .map_err(|e| AppError::PersistenceFailed(e.to_string()))?;
        if run_carries_execution_scope(&snapshot) {
            return Err(AppError::InvalidRequest(format!("run not found: {run_id}")));
        }
        if snapshot.state != RunState::RecoveryRequired {
            return Err(AppError::InvalidRequest(
                "run does not require recovery; refresh /recovery".into(),
            ));
        }
        self.run_manager
            .managed
            .abandon_recovery_run(&id, expected_sequence)
            .await
            .map_err(|e| AppError::PersistenceFailed(e.to_string()))?;
        Ok(format!(
            "Abandoned {run_id}. History and workspace changes were preserved.\n"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::CreateRunCommand;
    use crate::events::NoopEventSink;
    use std::sync::Arc;
    use tetonic_domain::{CancelRun, RunCommand};

    fn create() -> CreateRunCommand {
        CreateRunCommand {
            session_id: None,
            root_task_id: None,
            identity: None,
            job_spec: None,
        }
    }

    #[tokio::test]
    async fn interrupted_run_is_quarantined_and_explicit_abandonment_is_durable() {
        let dir = tempfile::tempdir().unwrap();
        let store = tetonic_memory::SharedStore::open(dir.path().join("audit.db"), 1).unwrap();
        let app = Application::bootstrap_mock_with_store(
            dir.path(),
            Some(store.clone()),
            Arc::new(NoopEventSink),
            vec![],
        );
        let id = app.runs.create_run(create()).await.unwrap();
        let mut snapshot = app.supervisor.snapshot(id.clone()).await.unwrap();
        snapshot.state = RunState::RecoveryRequired;
        let revision = snapshot.sequence;
        store
            .write_sync(move |db| db.persist_run_projection(&snapshot))
            .unwrap()
            .unwrap();
        drop(app);
        let app = Application::bootstrap_mock_with_store(
            dir.path(),
            Some(store.clone()),
            Arc::new(NoopEventSink),
            vec![],
        );
        assert!(!app.supervisor.is_safe_mode());
        assert!(app.recovery_report().await.unwrap().contains(&id.0));
        // An unrelated new run is accepted, but the original remains frozen.
        app.runs.create_run(create()).await.unwrap();
        assert!(app
            .supervisor
            .handle(RunCommand::CancelRun(CancelRun {
                envelope: tetonic_run::command_envelope("unreviewed", None, "test"),
                run_id: id.clone(),
            }))
            .await
            .is_err());
        assert!(app.abandon_recovery_run(&id.0, revision + 1).await.is_err());
        assert_eq!(
            app.supervisor.snapshot(id.clone()).await.unwrap().state,
            RunState::RecoveryRequired
        );
        app.abandon_recovery_run(&id.0, revision).await.unwrap();
        let canceled = app.supervisor.snapshot(id.clone()).await.unwrap();
        assert_eq!(canceled.state, RunState::Canceled);
        assert!(canceled.cancellation.run_canceled);
        assert_eq!(canceled.sequence, revision + 1);
        drop(app);
        let app = Application::bootstrap_mock_with_store(
            dir.path(),
            Some(store),
            Arc::new(NoopEventSink),
            vec![],
        );
        assert_eq!(
            app.supervisor.snapshot(id).await.unwrap().state,
            RunState::Canceled
        );
        assert!(app
            .recovery_report()
            .await
            .unwrap()
            .contains("No runs require recovery"));
    }

    #[tokio::test]
    async fn storage_failure_still_blocks_new_runs_and_abandonment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.db");
        let store = tetonic_memory::SharedStore::open(&path, 1).unwrap();
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch("DROP TABLE run_projections")
            .unwrap();
        let supervisor = crate::build_supervisor(Some(store));
        assert!(supervisor.is_safe_mode());
        assert!(supervisor
            .handle(RunCommand::CancelRun(CancelRun {
                envelope: tetonic_run::command_envelope("abandon", Some(1), "test"),
                run_id: RunId::new("missing"),
            }))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn unscoped_daemon_does_not_read_or_cancel_a_scoped_run() {
        let dir = tempfile::tempdir().unwrap();
        let store = tetonic_memory::SharedStore::open(dir.path().join("audit.db"), 1).unwrap();
        let app = Application::bootstrap_mock_with_store(
            dir.path(),
            Some(store),
            Arc::new(NoopEventSink),
            vec![],
        );
        let scoped = RunId::new("scoped-run");
        app.supervisor
            .handle(RunCommand::CreateRun(tetonic_domain::CreateRun {
                envelope: tetonic_run::command_envelope("create-scoped", None, "test"),
                session_id: None,
                run_id: scoped.clone(),
                root_task_id: tetonic_domain::TaskId::new("root"),
                root_binding: tetonic_domain::TaskInputBinding {
                    execution_scope: Some(tetonic_domain::ExecutionScope {
                        principal_id: "alice".into(),
                        organization_id: "org".into(),
                        information_context_id: "private-context".into(),
                    }),
                    ..Default::default()
                },
                speculation: None,
                job_spec: None,
            }))
            .await
            .unwrap();
        for err in [
            app.unscoped_daemon_inspect(&scoped.0).await.unwrap_err(),
            app.unscoped_daemon_resume(&scoped.0, 0, Some(10))
                .await
                .unwrap_err(),
            app.unscoped_daemon_cancel(&scoped.0).await.unwrap_err(),
        ] {
            let text = err.to_string();
            assert!(text.contains("run not found"), "{text}");
            assert!(!text.contains("private-context"), "{text}");
            assert!(!text.contains("alice"), "{text}");
        }
        assert_ne!(
            app.supervisor.snapshot(scoped.clone()).await.unwrap().state,
            RunState::Canceled
        );
        let mut held = app.supervisor.snapshot(scoped.clone()).await.unwrap();
        held.state = RunState::RecoveryRequired;
        held.session_id = Some(tetonic_domain::SessionId::new("PRIVATECANARY-session"));
        let revision = held.sequence;
        app.turn
            .store
            .as_ref()
            .unwrap()
            .write_sync(move |db| db.persist_run_projection(&held))
            .unwrap()
            .unwrap();
        drop(app);
        let app = Application::bootstrap_mock_with_store(
            dir.path(),
            Some(
                tetonic_memory::SharedStore::open(dir.path().join("audit.db"), 1).unwrap(),
            ),
            Arc::new(NoopEventSink),
            vec![],
        );
        let report = app.recovery_report().await.unwrap();
        assert!(
            !report.contains("PRIVATECANARY"),
            "{report}"
        );
        assert!(!report.contains(&scoped.0), "{report}");
        assert!(!report.contains("private-context"), "{report}");
        let abandoned = app.abandon_recovery_run(&scoped.0, revision).await.unwrap_err();
        let text = abandoned.to_string();
        assert!(text.contains("run not found"), "{text}");
        assert!(!text.contains("PRIVATECANARY"), "{text}");
        assert!(!text.contains("private-context"), "{text}");
        assert_eq!(
            app.supervisor.snapshot(scoped).await.unwrap().state,
            RunState::RecoveryRequired
        );
        let open = app.runs.create_run(create()).await.unwrap();
        app.unscoped_daemon_inspect(&open.0).await.unwrap();
    }
}
