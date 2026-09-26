//! Indexed admission projection of the existing run journal. There is no
//! independent reservation lifetime: only recorded worker quiescence releases
//! a terminal registered run. Crashes and ambiguous admissions retain capacity.

use crate::{Result, StoreError};

use tetonic_domain::{RunSnapshot, RunState};

pub(crate) fn registered_capacity(snapshot: &RunSnapshot) -> Result<(Option<&str>, bool)> {
    let mut bindings = snapshot
        .tasks
        .values()
        .filter(|t| t.binding.activation.is_some());
    let Some(root) = bindings.next() else {
        return Ok((None, false));
    };
    let job = root.binding.job_spec.as_ref().ok_or_else(|| {
        StoreError::InvalidControlResource("registered run has no identity binding".into())
    })?;
    if bindings.next().is_some()
        || root.binding.execution_scope.is_none()
        || snapshot.job_spec.as_ref() != Some(job)
    {
        return Err(StoreError::InvalidControlResource(
            "invalid registered run binding".into(),
        ));
    }
    let released = matches!(
        snapshot.state,
        RunState::Succeeded | RunState::Failed | RunState::Canceled
    ) && !snapshot.attempts.is_empty()
        && snapshot.attempts.values().all(|a| {
            a.execution_quiesced
                && matches!(
                    a.state,
                    tetonic_domain::AttemptState::Succeeded
                        | tetonic_domain::AttemptState::Failed
                        | tetonic_domain::AttemptState::TimedOut
                        | tetonic_domain::AttemptState::LeaseExpired
                        | tetonic_domain::AttemptState::Canceled
                        | tetonic_domain::AttemptState::Superseded
                )
        });
    Ok((Some(&job.identity_id.0), !released))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use tetonic_domain::{
        ActivationBinding, AgentJobSpec, ExecutionScope, IdentityId, TaskInputBinding,
    };

    fn legacy_terminal(run: &str) -> RunSnapshot {
        let job = AgentJobSpec {
            identity_id: IdentityId::new("agent"),
            definition_digest: "definition".into(),
            input_digest: "input".into(),
            capability_bindings: vec![],
            artifact_bindings: vec![],
            recovery_id: "recovery".into(),
        };
        let binding = TaskInputBinding {
            job_spec: Some(job.clone()),
            activation: Some(ActivationBinding {
                request_id: run.into(),
                request_digest: "digest".into(),
                audit_session_id: "audit".into(),
            }),
            execution_scope: Some(ExecutionScope {
                organization_id: "org".into(),
                principal_id: "alice".into(),
                information_context_id: "private".into(),
            }),
            ..Default::default()
        };
        serde_json::from_value(serde_json::json!({
            "run_id":run,"session_id":null,"state":"succeeded","sequence":9,"workspace_version":null,
            "tasks":{"root":{"task_id":"root","state":"succeeded","binding":binding,"accepted_artifact":null,"active_attempt":null}},
            "attempts":{"attempt":{"attempt_id":"attempt","task_id":"root","state":"succeeded","task_version":1,
                "workspace_version":null,"input_digest":"input","result_digest":"result","delivery_key":null,"lease":null,
                "failure_class":null,"failure_reason":null}},
            "dependencies":{},"events":[],"job_spec":job
        })).unwrap()
    }

    #[test]
    fn v40_upgrade_preserves_multiple_ambiguous_terminal_holds_and_legacy_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v40.db");
        {
            let db = Store::open(&path).unwrap();
            // The pre-limit journal could contain several runs for one identity.
            for run in ["first", "second"] {
                db.persist_run_projection(&legacy_terminal(run)).unwrap();
            }
            let mut legacy = legacy_terminal("unregistered");
            legacy.tasks.values_mut().next().unwrap().binding.activation = None;
            db.persist_run_projection(&legacy).unwrap();
            db.conn
                .execute_batch(
                    "DROP INDEX idx_run_execution_capacity;
                ALTER TABLE run_projections DROP COLUMN registered_identity_id;
                ALTER TABLE run_projections DROP COLUMN execution_held;
                DELETE FROM schema_versions WHERE version=41;",
                )
                .unwrap();
        }
        let db = Store::open(&path).unwrap();
        let held: i64 = db.conn.query_row("SELECT count(*) FROM run_projections WHERE execution_held=1 AND registered_identity_id='agent'", [], |r| r.get(0)).unwrap();
        assert_eq!(held, 2, "terminal status cannot invent worker quiescence");
        assert_eq!(
            db.load_run_snapshot("first").unwrap().unwrap(),
            legacy_terminal("first")
        );
        let unregistered: bool = db
            .conn
            .query_row(
                "SELECT execution_held FROM run_projections WHERE run_id='unregistered'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!unregistered);
        drop(db);
        Store::open(&path).unwrap();
    }

    #[test]
    fn capacity_requires_terminal_run_and_every_attempt_to_be_quiescent() {
        let mut snapshot = legacy_terminal("run");
        assert_eq!(
            registered_capacity(&snapshot).unwrap(),
            (Some("agent"), true)
        );
        snapshot
            .attempts
            .values_mut()
            .next()
            .unwrap()
            .execution_quiesced = true;
        assert_eq!(
            registered_capacity(&snapshot).unwrap(),
            (Some("agent"), false)
        );
        let mut second = snapshot.attempts.values().next().unwrap().clone();
        second.execution_quiesced = false;
        second.attempt_id = tetonic_domain::AttemptId::new("second");
        snapshot.attempts.insert(second.attempt_id.clone(), second);
        assert!(registered_capacity(&snapshot).unwrap().1);
        snapshot
            .attempts
            .remove(&tetonic_domain::AttemptId::new("second"));
        snapshot.state = RunState::RecoveryRequired;
        assert!(registered_capacity(&snapshot).unwrap().1);
        snapshot.state = RunState::Canceled;
        snapshot.attempts.clear();
        assert!(
            registered_capacity(&snapshot).unwrap().1,
            "partial admission keeps its hold"
        );
    }
}
