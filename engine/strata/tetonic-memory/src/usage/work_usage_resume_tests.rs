use super::*;
use tetonic_domain::{
    ArtifactRef, AttemptSuspension, CommandEnvelope, ResumeAttempt, RunCommand, TaskState,
};

fn saved_wait(db: &Store) -> (RunSnapshot, tetonic_domain::RunEventEnvelope) {
    let mut old = db.load_run_snapshot("run").unwrap().unwrap();
    let checkpoint = ArtifactRef {
        artifact_id: "wait".into(),
        digest: "a".repeat(64),
    };
    let a = old.attempts.get_mut(&AttemptId::new("attempt")).unwrap();
    a.state = AttemptState::Suspended;
    a.execution_quiesced = true;
    a.suspension = Some(AttemptSuspension {
        checkpoint: checkpoint.clone(),
        reason: tetonic_domain::SuspensionReason::HumanInput,
        suspended_at: 100,
        remaining_seconds: 100,
    });
    let t = old.tasks.get_mut(&TaskId::new("task")).unwrap();
    t.state = TaskState::Parked;
    t.binding.deadline = Some(200);
    db.persist_run_projection(&old).unwrap();
    let old_lease = old.attempts[&AttemptId::new("attempt")]
        .lease
        .as_ref()
        .unwrap();
    let command = RunCommand::ResumeAttempt(ResumeAttempt {
        envelope: CommandEnvelope {
            command_id: "resume".into(),
            expected_sequence: Some(old.sequence),
            trace: Default::default(),
            actor: "host".into(),
            timestamp: 150,
            workspace_version: None,
            idempotency_key: None,
        },
        run_id: old.run_id.clone(),
        attempt_id: AttemptId::new("attempt"),
        checkpoint,
        lease_proof: tetonic_domain::LeaseProof {
            lease_id: old_lease.lease_id.clone(),
            lease_epoch: old_lease.lease_epoch,
            holder: old_lease.holder.clone(),
        },
        holder: tetonic_domain::ExecutionTargetId::worker("reconstructed"),
    });
    let mut next = old.clone();
    next.sequence += 1;
    let a = next.attempts.get_mut(&AttemptId::new("attempt")).unwrap();
    a.state = AttemptState::Starting;
    a.execution_claimed = false;
    a.execution_quiesced = false;
    a.suspension = None;
    let lease = a.lease.as_mut().unwrap();
    lease.lease_epoch += 1;
    lease.holder = tetonic_domain::ExecutionTargetId::worker("reconstructed");
    let t = next.tasks.get_mut(&TaskId::new("task")).unwrap();
    t.state = TaskState::Leased;
    t.binding.deadline = Some(250);
    let payload = serde_json::to_value(command).unwrap();
    let event = tetonic_domain::RunEventEnvelope {
        event_id: tetonic_domain::ids::EventId::new("resume-event"),
        run_id: next.run_id.clone(),
        sequence: next.sequence,
        event_type: tetonic_domain::EventType::Other("attempt.resumed".into()),
        schema_version: 1,
        command_id: Some(tetonic_domain::ids::CommandId::new("resume")),
        causation_id: None,
        correlation_id: None,
        actor: tetonic_domain::EventActor {
            name: "host".into(),
        },
        occurred_at: chrono::Utc::now(),
        recorded_at: chrono::Utc::now(),
        data_class: tetonic_domain::DataClass::Secret,
        payload_digest: crate::payload_digest::digest_event_payload(&payload).unwrap(),
        payload,
    };
    (next, event)
}

fn ledger(db: &Store) -> (String, i64, i64) {
    db.conn.query_row("SELECT fence,allowance,released_tokens FROM work_budget_executions WHERE attempt_id='attempt'", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap()
}

#[test]
fn resume_transfers_usage_fence_atomically_without_resetting_allowance_or_unknown_spend() {
    for variant in [
        "reported",
        "unknown",
        "rollback",
        "changed-fence",
        "released",
    ] {
        let db = Store::open(":memory:").unwrap();
        seed(&db);
        assert_eq!(limited(&db, "before", 60).unwrap(), Some(60));
        if variant != "unknown" {
            db.finish_work_inference("before", Some(10), Some(20))
                .unwrap();
        }
        let (mut next, mut event) = saved_wait(&db);
        if variant == "changed-fence" {
            db.conn
                .execute(
                    "UPDATE work_budget_executions SET fence='another-owner'",
                    [],
                )
                .unwrap();
        } else if variant == "released" {
            db.conn
                .execute("UPDATE work_budget_executions SET released_tokens=1", [])
                .unwrap();
        }
        let before = ledger(&db);
        if variant == "rollback" {
            event.payload_digest = tetonic_domain::ContentDigest("invalid".into());
            // Fails after the lease/accounting update.
        }
        let result = db.commit_run_command(&next, &event, None);
        if matches!(variant, "rollback" | "changed-fence" | "released") {
            assert!(result.is_err(), "{variant}");
            assert_eq!(ledger(&db), before, "all accounting changes roll back");
            assert_eq!(
                db.load_run_snapshot("run").unwrap().unwrap().attempts[&AttemptId::new("attempt")]
                    .state,
                AttemptState::Suspended
            );
            assert!(db.list_run_events("run").unwrap().is_empty());
            continue;
        }
        result.unwrap();
        let after = ledger(&db);
        assert_ne!(after.0, before.0);
        assert_eq!((after.1, after.2), (before.1, before.2));
        assert!(
            db.commit_run_command(&next, &event, None).is_err(),
            "no second reservation transfer"
        );
        // Model the separate execution claim. Projection-only lease replacement
        // still cannot move accounting; only the journaled resume above did.
        let a = next.attempts.get_mut(&AttemptId::new("attempt")).unwrap();
        a.state = AttemptState::Running;
        a.execution_claimed = true;
        next.tasks.get_mut(&TaskId::new("task")).unwrap().state = TaskState::Running;
        db.persist_run_projection(&next).unwrap();
        if variant == "unknown" {
            assert!(
                limited(&db, "after", 100).is_err(),
                "unknown spend remains blocking"
            );
        } else {
            assert_eq!(
                limited(&db, "after", 100).unwrap(),
                Some(30),
                "no top-up after resume"
            );
        }
    }
}
