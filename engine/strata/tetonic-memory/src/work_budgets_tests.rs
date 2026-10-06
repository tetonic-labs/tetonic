use super::*;
use crate::{OrganizationRole, TeamRow};

fn seed(db: &Store) {
    db.bootstrap_control("alice", "org", "Org").unwrap();
    db.register_control_principal("bob").unwrap();
    db.set_organization_member("org", "bob", OrganizationRole::Member)
        .unwrap();
    db.create_team(&TeamRow {
        org_id: "org".into(),
        team_id: "team".into(),
        name: "Team".into(),
        owner_principal_id: "alice".into(),
    })
    .unwrap();
    db.add_team_member("org", "team", "bob").unwrap();
    db.create_team_work_item(crate::CreateTeamWorkItem {
        actor: "alice",
        org: "org",
        team: "team",
        work_id: "root",
        title: "Root",
        request_id: "root-request",
        goal_id: None,
    })
    .unwrap();
}

fn primed() -> Store {
    let db = Store::open(":memory:").unwrap();
    seed(&db);
    db.authorize_work_budget("alice", "org", "team", "root", "fund", 100)
        .unwrap();
    db
}

fn child(db: &Store, id: &str, tokens: i64) -> Result<crate::WorkDelegation> {
    db.create_work_delegation(crate::CreateWorkDelegation {
        actor: "alice",
        org: "org",
        team: "team",
        delegation_id: id,
        parent_work_id: "root",
        child_work_id: &format!("{id}-work"),
        child_title: "Help",
        request_id: &format!("{id}-request"),
        parent_budget_tokens: 100,
        child_budget_tokens: tokens,
        stop_scope: "inherit",
        peer_org: None,
        peer_team: None,
    })
}

#[test]
fn allowance_is_explicit_immutable_and_requires_current_manager() {
    let db = Store::open(":memory:").unwrap();
    seed(&db);
    assert!(child(&db, "no-authority", 40).is_err());
    assert!(db
        .authorize_work_budget("bob", "org", "team", "root", "fund", 100)
        .is_err());
    let first = db
        .authorize_work_budget("alice", "org", "team", "root", "fund", 100)
        .unwrap();
    assert_eq!(first.available_tokens, 100);
    assert_eq!(
        first,
        db.authorize_work_budget("alice", "org", "team", "root", "fund", 100)
            .unwrap()
    );
    for (request, tokens) in [("fund", 101), ("second-fund", 100), ("fund", 0)] {
        assert!(db
            .authorize_work_budget("alice", "org", "team", "root", request, tokens)
            .is_err());
    }
    assert!(db
        .create_work_delegation(crate::CreateWorkDelegation {
            actor: "alice",
            org: "org",
            team: "team",
            delegation_id: "inflated",
            parent_work_id: "root",
            child_work_id: "c",
            child_title: "Help",
            request_id: "inflated",
            parent_budget_tokens: 1000,
            child_budget_tokens: 40,
            stop_scope: "inherit",
            peer_org: None,
            peer_team: None
        })
        .is_err());
    assert!(db
        .reserve_work_budget("bob", "org", "team", "root", "spend", 1)
        .is_err());
    assert!(db.work_budget("bob", "org", "team", "root").is_ok());
    db.conn
        .execute(
            "UPDATE control_principals SET enabled=0 WHERE principal_id='alice'",
            [],
        )
        .unwrap();
    assert!(db
        .authorize_work_budget("alice", "org", "team", "root", "fund", 100)
        .is_err());
    assert!(child(&db, "disabled", 1).is_err());
}

#[test]
fn sibling_allocations_and_parent_effort_share_a_balance_without_double_charging_retries() {
    let db = primed();
    let receipt = db
        .reserve_work_budget("alice", "org", "team", "root", "planning", 20)
        .unwrap();
    let first = child(&db, "first", 60).unwrap();
    assert_eq!(first, child(&db, "first", 60).unwrap());
    assert_eq!(
        receipt,
        db.reserve_work_budget("alice", "org", "team", "root", "planning", 20)
            .unwrap()
    );
    assert!(child(&db, "second", 21).is_err());
    assert!(db
        .reserve_work_budget("alice", "org", "team", "root", "planning", 19)
        .is_err());
    assert!(db
        .reserve_work_budget("alice", "org", "team", "root", "more", 21)
        .is_err());
    child(&db, "second", 20).unwrap();
    let budget = db.work_budget("alice", "org", "team", "root").unwrap();
    assert_eq!(
        (
            budget.token_limit,
            budget.reserved_tokens,
            budget.delegated_tokens,
            budget.available_tokens
        ),
        (100, 20, 80, 0)
    );
    assert!(child(&db, "third", 1).is_err());
    assert!(db
        .get_team_work_item("org", "team", "third-work")
        .unwrap()
        .is_none());
}

#[test]
fn nested_delegation_inherits_payer_and_root_stop_and_cannot_fund_itself() {
    let db = primed();
    child(&db, "first", 60).unwrap();
    db.set_organization_member("org", "bob", OrganizationRole::Administrator)
        .unwrap();
    let nested = db
        .create_work_delegation(crate::CreateWorkDelegation {
            actor: "bob",
            org: "org",
            team: "team",
            delegation_id: "nested",
            parent_work_id: "first-work",
            child_work_id: "grandchild",
            child_title: "Investigate",
            request_id: "nested",
            parent_budget_tokens: 60,
            child_budget_tokens: 40,
            stop_scope: "inherit",
            peer_org: None,
            peer_team: None,
        })
        .unwrap();
    assert_eq!(nested.payer_principal_id, "alice");
    assert_eq!(nested.created_by, "bob");
    assert_eq!(nested.stop_scope, "work/root");
    assert!(db
        .authorize_work_budget("bob", "org", "team", "grandchild", "new-payer", 100)
        .is_err());
    assert!(db
        .create_work_delegation(crate::CreateWorkDelegation {
            actor: "bob",
            org: "org",
            team: "team",
            delegation_id: "overflow",
            parent_work_id: "first-work",
            child_work_id: "extra",
            child_title: "Investigate",
            request_id: "overflow",
            parent_budget_tokens: 60,
            child_budget_tokens: 21,
            stop_scope: "inherit",
            peer_org: None,
            peer_team: None
        })
        .is_err());
    let budget = db.work_budget("bob", "org", "team", "grandchild").unwrap();
    assert_eq!(budget.token_limit, 40);
    assert_eq!(budget.root_work_id, "root");
    assert_eq!(budget.payer_principal_id, "alice");
    assert_eq!(
        db.work_budget("alice", "org", "team", "root")
            .unwrap()
            .available_tokens,
        40
    );
    db.park_team_work_item("bob", "org", "team", "first-work")
        .unwrap();
    assert!(db
        .reserve_work_budget("bob", "org", "team", "grandchild", "after-park", 1)
        .is_err());
    assert_eq!(
        db.work_budget("alice", "org", "team", "root")
            .unwrap()
            .available_tokens,
        40
    );
}

#[test]
fn retries_are_exact_and_ids_cannot_attach_existing_work_or_spoof_stop_scope() {
    let db = primed();
    child(&db, "first", 40).unwrap();
    for (id, parent, target, title, request, stop, peer_org, peer_team) in [
        (
            "first",
            "root",
            "first-work",
            "Changed",
            "first-request",
            "inherit",
            None,
            None,
        ),
        (
            "different",
            "root",
            "first-work",
            "Help",
            "different",
            "inherit",
            None,
            None,
        ),
        (
            "self", "root", "root", "Root", "self", "inherit", None, None,
        ),
        (
            "prefix",
            "root",
            "prefix-work",
            "Help",
            "prefix",
            "work/root-elsewhere",
            None,
            None,
        ),
        (
            "peer",
            "root",
            "peer-work",
            "Help",
            "peer",
            "inherit",
            Some("other"),
            None,
        ),
        (
            "peer",
            "root",
            "peer-work",
            "Help",
            "peer",
            "inherit",
            None,
            Some("team"),
        ),
    ] {
        assert!(
            db.create_work_delegation(crate::CreateWorkDelegation {
                actor: "alice",
                org: "org",
                team: "team",
                delegation_id: id,
                parent_work_id: parent,
                child_work_id: target,
                child_title: title,
                request_id: request,
                parent_budget_tokens: 100,
                child_budget_tokens: 40,
                stop_scope: stop,
                peer_org,
                peer_team
            })
            .is_err(),
            "{id}"
        );
    }
    assert_eq!(
        db.work_budget("alice", "org", "team", "root")
            .unwrap()
            .available_tokens,
        60
    );
    assert_eq!(
        db.list_team_work_items("alice", "org", "team")
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn parallel_writers_cannot_oversubscribe_siblings_or_parent_effort() {
    for parent_effort in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budgets.db");
        let db = Store::open(&path).unwrap();
        seed(&db);
        db.authorize_work_budget("alice", "org", "team", "root", "fund", 100)
            .unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let threads: Vec<_> = (0..2)
            .map(|index| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let db = Store::open(path).unwrap();
                    barrier.wait();
                    if parent_effort && index == 1 {
                        db.reserve_work_budget("alice", "org", "team", "root", "own-effort", 60)
                            .is_ok()
                    } else {
                        child(&db, &format!("parallel-{index}"), 60).is_ok()
                    }
                })
            })
            .collect();
        let successes = threads
            .into_iter()
            .map(|t| t.join().unwrap())
            .filter(|ok| *ok)
            .count();
        assert_eq!(successes, 1);
        let budget = db.work_budget("alice", "org", "team", "root").unwrap();
        assert_eq!(budget.available_tokens, 40);
        assert_eq!(budget.delegated_tokens + budget.reserved_tokens, 60);
    }
}

#[test]
fn reservations_survive_restart_stop_and_unknown_usage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("budgets.db");
    {
        let db = Store::open(&path).unwrap();
        seed(&db);
        db.authorize_work_budget("alice", "org", "team", "root", "fund", 100)
            .unwrap();
        db.reserve_work_budget("alice", "org", "team", "root", "before-call", 30)
            .unwrap();
        child(&db, "first", 60).unwrap();
        db.record_team_effort(crate::RecordTeamEffort {
            actor: "alice",
            org: "org",
            team: "team",
            entry_id: "unknown",
            request_id: "usage",
            measured_tokens: None,
            goal_id: None,
            work_id: Some("root"),
        })
        .unwrap();
        db.request_control_stop("alice", "org", "work", "root", "estop", "Stop")
            .unwrap();
    }
    let db = Store::open(&path).unwrap();
    let budget = db.work_budget("alice", "org", "team", "root").unwrap();
    assert_eq!(budget.available_tokens, 10);
    assert_eq!(budget.reserved_tokens, 30);
    assert!(child(&db, "after-stop", 1).is_err());
    // Recovering a receipt is read-only; a new allocation after stop is denied.
    assert!(child(&db, "first", 60).is_ok());
    assert!(db
        .reserve_work_budget("alice", "org", "team", "root", "before-call", 30)
        .is_ok());
    assert!(db
        .reserve_work_budget("alice", "org", "team", "root", "after-stop", 1)
        .is_err());
    assert!(matches!(
        db.work_budget("outsider", "org", "team", "root"),
        Err(StoreError::ControlAccessDenied)
    ));
}

#[test]
fn work_stop_reaches_descendants_and_does_not_park_unrelated_work() {
    let db = primed();
    child(&db, "first", 60).unwrap();
    db.create_work_delegation(crate::CreateWorkDelegation {
        actor: "alice",
        org: "org",
        team: "team",
        delegation_id: "nested",
        parent_work_id: "first-work",
        child_work_id: "grandchild",
        child_title: "Investigate",
        request_id: "nested",
        parent_budget_tokens: 60,
        child_budget_tokens: 40,
        stop_scope: "inherit",
        peer_org: None,
        peer_team: None,
    })
    .unwrap();
    db.create_team_work_item(crate::CreateTeamWorkItem {
        actor: "alice",
        org: "org",
        team: "team",
        work_id: "other",
        title: "Other",
        request_id: "other",
        goal_id: None,
    })
    .unwrap();
    for work in ["root", "first-work", "grandchild", "other"] {
        db.activate_team_work_item(
            "alice",
            "org",
            "team",
            work,
            &format!("attempt-{work}"),
            &format!("run-{work}"),
        )
        .unwrap();
    }
    let mut runs = db.run_ids_under_stop("org", "work", "root").unwrap();
    runs.sort();
    assert_eq!(runs, vec!["run-first-work", "run-grandchild", "run-root"]);
    let (_, mut captured) = db
        .request_control_stop_with_runs("alice", "org", "work", "root", "cancel", "Stop")
        .unwrap();
    captured.sort();
    assert_eq!(captured, runs);
    for work in ["root", "first-work", "grandchild"] {
        assert_eq!(
            db.get_team_work_item("org", "team", work)
                .unwrap()
                .unwrap()
                .status,
            "parked"
        );
        assert_eq!(
            db.activation_blocked_by_stop("org", "team", work, None, None)
                .unwrap()
                .unwrap()
                .scope_id,
            "root"
        );
        assert!(db
            .activate_team_work_item("alice", "org", "team", work, "late-attempt", "late-run")
            .is_err());
        assert!(db
            .resume_team_work_item("alice", "org", "team", work)
            .is_err());
    }
    assert_eq!(
        db.get_team_work_item("org", "team", "other")
            .unwrap()
            .unwrap()
            .status,
        "running"
    );
    assert!(db
        .activation_blocked_by_stop("org", "team", "other", None, None)
        .unwrap()
        .is_none());
}

#[test]
fn duplicate_delivery_from_two_connections_returns_one_allocation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("duplicate.db");
    let db = Store::open(&path).unwrap();
    seed(&db);
    db.authorize_work_budget("alice", "org", "team", "root", "fund", 100)
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let db = Store::open(path).unwrap();
                barrier.wait();
                child(&db, "same", 60).unwrap()
            })
        })
        .collect();
    let receipts: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(receipts[0], receipts[1]);
    assert_eq!(
        db.work_budget("alice", "org", "team", "root")
            .unwrap()
            .delegated_tokens,
        60
    );
    assert_eq!(
        db.list_team_work_items("alice", "org", "team")
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn stop_and_binding_race_either_captures_the_run_or_denies_its_binding() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stop.db");
    let db = Store::open(&path).unwrap();
    seed(&db);
    db.authorize_work_budget("alice", "org", "team", "root", "fund", 100)
        .unwrap();
    child(&db, "first", 60).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let bind_path = path.clone();
    let bind_barrier = barrier.clone();
    let bind = std::thread::spawn(move || {
        let db = Store::open(bind_path).unwrap();
        bind_barrier.wait();
        db.activate_team_work_item(
            "alice",
            "org",
            "team",
            "first-work",
            "child-attempt",
            "child-run",
        )
        .is_ok()
    });
    let stop = std::thread::spawn(move || {
        let db = Store::open(path).unwrap();
        barrier.wait();
        db.request_control_stop_with_runs("alice", "org", "work", "root", "cancel", "Stop")
            .unwrap()
            .1
    });
    let bound = bind.join().unwrap();
    let targets = stop.join().unwrap();
    assert_eq!(bound, targets.contains(&"child-run".into()));
    assert_eq!(
        db.get_team_work_item("org", "team", "first-work")
            .unwrap()
            .unwrap()
            .status,
        "parked"
    );
    assert!(db
        .activate_team_work_item("alice", "org", "team", "first-work", "late", "late")
        .is_err());
}

#[test]
fn corrupt_or_ambiguous_legacy_lineage_fails_closed() {
    let db = primed();
    child(&db, "first", 40).unwrap();
    db.conn
        .execute(
            "UPDATE work_delegations SET parent_work_id=child_work_id",
            [],
        )
        .unwrap();
    assert!(db
        .work_budget("alice", "org", "team", "first-work")
        .is_err());
    assert!(db
        .activation_blocked_by_stop("org", "team", "first-work", None, None)
        .is_err());
}

#[test]
fn migration_does_not_turn_legacy_budget_claims_into_authority() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    {
        let db = primed();
        // Ensure migration reentry is harmless with current records.
        child(&db, "first", 40).unwrap();
        db.migrate_work_budgets_v55().unwrap();
        assert_eq!(
            db.work_budget("alice", "org", "team", "root")
                .unwrap()
                .available_tokens,
            60
        );
    }
    {
        let db = Store::open(&path).unwrap();
        seed(&db);
        db.authorize_work_budget("alice", "org", "team", "root", "fund", 100)
            .unwrap();
        child(&db, "first", 40).unwrap();
        db.conn
            .execute_batch(
                "DROP TABLE work_budget_reservations; DROP TABLE work_budget_envelopes;
            DELETE FROM schema_versions WHERE version>=55;",
            )
            .unwrap();
    }
    let db = Store::open(&path).unwrap();
    assert!(db.work_budget("alice", "org", "team", "root").is_err());
    assert!(db
        .authorize_work_budget("alice", "org", "team", "root", "fund-new", 100)
        .is_err());
    assert!(child(&db, "legacy-new", 1).is_err());
    assert_eq!(
        db.list_team_work_items("alice", "org", "team")
            .unwrap()
            .len(),
        2
    );
}
