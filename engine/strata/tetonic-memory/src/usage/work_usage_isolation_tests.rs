use super::*;

fn branch(db: &Store, parent: &str, id: &str, parent_limit: i64, tokens: i64) {
    db.create_work_delegation(crate::CreateWorkDelegation {
        actor: "owner",
        org: "org",
        team: "team",
        delegation_id: id,
        parent_work_id: parent,
        child_work_id: id,
        child_title: id,
        request_id: id,
        parent_budget_tokens: parent_limit,
        child_budget_tokens: tokens,
        stop_scope: "inherit",
        peer_org: None,
        peer_team: None,
    })
    .unwrap();
    let mut run = db.load_run_snapshot("run").unwrap().unwrap();
    let mut task = run.tasks[&TaskId::new("task")].clone();
    task.task_id = TaskId::new(id);
    task.active_attempt = Some(AttemptId::new(id));
    task.binding.job_spec.as_mut().unwrap().identity_id =
        tetonic_domain::IdentityId::new(format!("agent-{id}"));
    task.binding.execution_grant_id = Some(format!("grant-{id}"));
    task.binding.delegation = Some(tetonic_domain::DelegatedTaskBinding {
        parent_lease: None,
        parent_attempt: AttemptId::new(if parent == "work" { "attempt" } else { parent }),
        activation: ActivationBinding {
            request_id: work_activation_request_id(&format!("{id}/child")),
            request_digest: format!("digest-{id}"),
            audit_session_id: format!("audit-{id}"),
        },
    });
    task.binding.activation = None;
    let mut attempt = run.attempts[&AttemptId::new("attempt")].clone();
    attempt.attempt_id = AttemptId::new(id);
    attempt.task_id = task.task_id.clone();
    attempt.lease.as_mut().unwrap().attempt_id = attempt.attempt_id.clone();
    run.tasks.insert(task.task_id.clone(), task);
    run.attempts.insert(attempt.attempt_id.clone(), attempt);
    db.persist_run_projection(&run).unwrap();
}

fn call(db: &Store, work: &str, call: &str, minimum: i64) -> Result<Option<i64>> {
    db.begin_work_inference_with_headroom(
        crate::BeginWorkInference {
            actor: "owner",
            org: "org",
            team: "team",
            work,
            run: "run",
            task: if work == "work" { "task" } else { work },
            attempt: if work == "work" { "attempt" } else { work },
            call,
            model: "test",
            now: 100,
        },
        None,
        minimum,
    )
}

#[test]
fn rejected_input_has_no_pending_call_or_reservation_and_does_not_poison_retry() {
    let db = Store::open(":memory:").unwrap();
    seed(&db);
    assert!(call(&db, "work", "too-large", 101).is_err());
    let usage = db.team_work_usage("owner", "org", "team").unwrap();
    assert_eq!(usage[0].calls, 0);
    assert_eq!(usage[0].budget.as_ref().unwrap().available_tokens, 100);
    assert_eq!(call(&db, "work", "fits", 90).unwrap(), Some(100));
    db.finish_work_inference("fits", Some(10), Some(10))
        .unwrap();
    assert!(call(&db, "work", "too-large-again", 81).is_err());
    let usage = db.team_work_usage("owner", "org", "team").unwrap();
    assert_eq!(usage[0].calls, 1);
    assert_eq!(usage[0].unknown_calls + usage[0].pending_calls, 0);
    assert_eq!(call(&db, "work", "fits-again", 80).unwrap(), Some(80));
}

#[test]
fn sibling_survives_an_overrun_and_unknown_usage_within_shared_capacity_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("budget.db");
    {
        let db = Store::open(&path).unwrap();
        seed(&db);
        limited(&db, "parent", 20).unwrap();
        branch(&db, "work", "failed", 100, 30);
        branch(&db, "work", "healthy", 100, 30);
        call(&db, "failed", "overrun", 1).unwrap();
        call(&db, "healthy", "healthy-1", 1).unwrap();
        db.finish_work_inference("overrun", Some(30), Some(6))
            .unwrap();
        db.finish_work_inference("healthy-1", Some(2), Some(3))
            .unwrap();
        // The parent remains unconfirmed. Its full twenty tokens stay reserved.
        db.finish_work_inference("parent", None, None).unwrap();
    }
    let db = Store::open(&path).unwrap();
    assert!(call(&db, "failed", "no-more", 1).is_err());
    assert!(call(&db, "work", "unknown-retry", 1).is_err());
    assert_eq!(call(&db, "healthy", "healthy-2", 1).unwrap(), Some(25));
    let root = db.work_budget("owner", "org", "team", "work").unwrap();
    assert_eq!(root.reserved_tokens, 26);
    assert_eq!(root.available_tokens, 14);
    // The reported extra six tokens cannot be spent again by a new allocation.
    assert!(db
        .reserve_work_budget("owner", "org", "team", "work", "too-much", 15)
        .is_err());
    db.reserve_work_budget("owner", "org", "team", "work", "fits", 14)
        .unwrap();
}

#[test]
fn funding_previously_unbounded_work_cannot_turn_unknown_usage_into_free_capacity() {
    let db = Store::open(":memory:").unwrap();
    seed(&db);
    db.conn
        .execute("DELETE FROM work_budget_envelopes", [])
        .unwrap();
    assert_eq!(begin(&db, "unbounded").unwrap(), None);
    db.finish_work_inference("unbounded", None, None).unwrap();
    db.authorize_work_budget("owner", "org", "team", "work", "new-fund", 100)
        .unwrap();
    assert!(call(&db, "work", "must-not-send", 1).is_err());
    let usage = db.team_work_usage("owner", "org", "team").unwrap();
    assert_eq!(usage[0].unknown_calls, 1);
    assert_eq!(usage[0].calls, 1);
}

#[test]
fn fully_allocated_project_caps_calls_to_actual_shared_headroom_instead_of_killing_peers() {
    let db = Store::open(":memory:").unwrap();
    seed(&db);
    limited(&db, "parent", 40).unwrap();
    branch(&db, "work", "failed", 100, 30);
    branch(&db, "work", "healthy", 100, 30);
    call(&db, "failed", "overrun", 1).unwrap();
    call(&db, "healthy", "healthy-1", 1).unwrap();
    db.finish_work_inference("parent", Some(2), Some(3))
        .unwrap();
    db.finish_work_inference("overrun", Some(30), Some(6))
        .unwrap();
    db.finish_work_inference("healthy-1", Some(2), Some(3))
        .unwrap();
    assert_eq!(call(&db, "healthy", "healthy-2", 1).unwrap(), Some(25));
    // 46 spent + 25 reserved by the unresolved peer leaves 29 for this call,
    // even though the coordinator's personal allowance still has 35 left.
    assert_eq!(limited(&db, "parent-2", 40).unwrap(), Some(29));
    assert!(
        !db.team_work_usage("owner", "org", "team")
            .unwrap()
            .iter()
            .find(|r| r.work_id == "work")
            .unwrap()
            .over_limit
    );
    db.finish_work_inference("healthy-2", Some(20), Some(5))
        .unwrap();
    db.finish_work_inference("parent-2", Some(20), Some(9))
        .unwrap();
    assert!(limited(&db, "no-more-parent", 40).is_err());
    assert_eq!(
        db.team_work_usage("owner", "org", "team")
            .unwrap()
            .iter()
            .map(|r| r.input_tokens + r.output_tokens)
            .sum::<i64>(),
        100
    );
}

#[test]
fn parallel_calls_cannot_spend_the_same_shared_headroom_after_a_sibling_overrun() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.db");
    let db = Store::open(&path).unwrap();
    seed(&db);
    limited(&db, "parent", 40).unwrap();
    branch(&db, "work", "failed", 100, 30);
    branch(&db, "work", "healthy", 100, 30);
    call(&db, "failed", "overrun", 1).unwrap();
    call(&db, "healthy", "healthy-1", 1).unwrap();
    db.finish_work_inference("parent", Some(2), Some(3))
        .unwrap();
    db.finish_work_inference("overrun", Some(30), Some(6))
        .unwrap();
    db.finish_work_inference("healthy-1", Some(2), Some(3))
        .unwrap();
    drop(db);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let writers: Vec<_> = ["work", "healthy"]
        .into_iter()
        .map(|work| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let db = Store::open(path).unwrap();
                barrier.wait();
                call(&db, work, &format!("{work}-next"), 1)
                    .unwrap()
                    .unwrap()
            })
        })
        .collect();
    let admitted: i64 = writers.into_iter().map(|w| w.join().unwrap()).sum();
    assert_eq!(
        admitted, 54,
        "46 already spent; concurrent reservations must fit the shared 100"
    );
}

#[test]
fn shared_overrun_blocks_every_branch_but_keeps_inflight_reports() {
    let db = Store::open(":memory:").unwrap();
    seed(&db);
    limited(&db, "parent", 20).unwrap();
    branch(&db, "work", "failed", 100, 30);
    branch(&db, "work", "healthy", 100, 30);
    call(&db, "failed", "overrun", 1).unwrap();
    call(&db, "healthy", "inflight", 1).unwrap();
    db.finish_work_inference("overrun", Some(80), Some(11))
        .unwrap();
    db.finish_work_inference("parent", Some(2), Some(3))
        .unwrap();
    db.finish_work_inference("inflight", Some(2), Some(3))
        .unwrap();
    assert!(call(&db, "healthy", "stop", 1).is_err());
    assert!(call(&db, "work", "stop-root", 1).is_err());
    let usage = db.team_work_usage("owner", "org", "team").unwrap();
    assert!(
        usage
            .iter()
            .find(|r| r.work_id == "work")
            .unwrap()
            .over_limit
    );
    assert_eq!(
        usage
            .iter()
            .map(|r| r.input_tokens + r.output_tokens)
            .sum::<i64>(),
        101
    );
}

#[test]
fn nested_overrun_uses_nearest_allocated_headroom_before_shared_root() {
    let db = Store::open(":memory:").unwrap();
    seed(&db);
    limited(&db, "parent", 20).unwrap();
    branch(&db, "work", "branch", 100, 50);
    branch(&db, "branch", "leaf", 50, 10);
    branch(&db, "work", "peer", 100, 30);
    call(&db, "leaf", "overrun", 1).unwrap();
    db.finish_work_inference("overrun", Some(10), Some(5))
        .unwrap();
    assert!(call(&db, "leaf", "stop-leaf", 1).is_err());
    assert_eq!(
        db.work_budget("owner", "org", "team", "branch")
            .unwrap()
            .available_tokens,
        35
    );
    assert_eq!(
        db.work_budget("owner", "org", "team", "work")
            .unwrap()
            .available_tokens,
        0
    );
    assert_eq!(call(&db, "peer", "continues", 1).unwrap(), Some(30));
    // The ancestor can use only the thirty-five tokens not already consumed.
    assert_eq!(
        call(&db, "branch", "continues-parent", 1).unwrap(),
        Some(35)
    );
}
