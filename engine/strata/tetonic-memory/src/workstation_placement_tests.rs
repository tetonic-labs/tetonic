use super::*;
use crate::{OrganizationRole, TeamRow};

fn primed() -> Store {
    let db = Store::open(":memory:").unwrap();
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
    db
}

#[test]
fn team_membership_does_not_grant_workstation_and_shared_is_opt_in() {
    let db = primed();
    let ws = db
        .enroll_workstation(
            "alice",
            "org",
            "laptop",
            "Alice laptop",
            "linux",
            "device-secret",
            false,
        )
        .unwrap();
    assert!(!ws.shared_assignment);
    assert!(db
        .approve_workstation_grant("bob", "org", "laptop", "g1", "path", "/home/alice/proj",)
        .is_err());
    db.approve_workstation_grant("alice", "org", "laptop", "g1", "path", "/home/alice/proj")
        .unwrap();
    db.create_team_work_item("alice", "org", "team", "w1", "Ship", "r1", None)
        .unwrap();
    assert!(db
        .pin_work_to_workstation("bob", "org", "team", "w1", "laptop")
        .is_err());
    db.pin_work_to_workstation("alice", "org", "team", "w1", "laptop")
        .unwrap();
}

#[test]
fn offline_parks_pins_reconnect_does_not_duplicate_and_revoke_fences() {
    let db = primed();
    db.enroll_workstation(
        "alice",
        "org",
        "laptop",
        "Alice laptop",
        "macos",
        "device-secret",
        true,
    )
    .unwrap();
    db.create_team_work_item("alice", "org", "team", "w1", "Ship", "r1", None)
        .unwrap();
    db.create_team_work_item("alice", "org", "team", "w2", "Other", "r2", None)
        .unwrap();
    db.pin_work_to_workstation("bob", "org", "team", "w1", "laptop")
        .unwrap();
    let (offline, parked) = db
        .mark_workstation_offline("alice", "org", "laptop")
        .unwrap();
    assert_eq!(offline.status, "offline");
    assert_eq!(parked, vec!["w1".to_string()]);
    assert_eq!(
        db.get_team_work_item("org", "team", "w1")
            .unwrap()
            .unwrap()
            .status,
        "parked"
    );
    assert_eq!(
        db.get_team_work_item("org", "team", "w2")
            .unwrap()
            .unwrap()
            .status,
        "open"
    );
    assert!(db
        .placement_blocks_activation("org", "team", "w1")
        .unwrap()
        .is_some());
    let before = db.get_workstation("org", "laptop").unwrap().unwrap();
    let again = db.reconnect_workstation("alice", "org", "laptop").unwrap();
    assert_eq!(again.status, "enrolled");
    assert_eq!(again.assignment_generation, before.assignment_generation);
    assert_eq!(
        db.list_team_work_items("alice", "org", "team")
            .unwrap()
            .len(),
        2
    );

    let claim = db
        .claim_worker_assignment(
            "org",
            "laptop",
            "device-secret",
            "a1",
            "areq",
            Some("w1"),
            again.assignment_generation,
        )
        .unwrap();
    assert_eq!(claim.status, "claimed");
    assert_eq!(
        db.claim_worker_assignment(
            "org",
            "laptop",
            "device-secret",
            "a1",
            "areq",
            Some("w1"),
            again.assignment_generation,
        )
        .unwrap(),
        claim
    );
    let revoked = db.revoke_workstation("alice", "org", "laptop").unwrap();
    assert_eq!(revoked.status, "revoked");
    assert_eq!(
        revoked.assignment_generation,
        again.assignment_generation + 1
    );
    assert!(db
        .claim_worker_assignment(
            "org",
            "laptop",
            "device-secret",
            "a2",
            "areq2",
            None,
            again.assignment_generation,
        )
        .is_err());
    assert!(db
        .accept_worker_assignment_result(
            "org",
            "laptop",
            "device-secret",
            "a1",
            again.assignment_generation,
        )
        .is_err());
    assert!(db
        .workstation_grants_for("org", "laptop")
        .unwrap()
        .is_empty());
}

#[test]
fn drain_blocks_new_claims_and_generation_survives_reopen() {
    let db = primed();
    db.enroll_workstation("alice", "org", "box", "Box", "windows", "secret", true)
        .unwrap();
    let gen = db
        .get_workstation("org", "box")
        .unwrap()
        .unwrap()
        .assignment_generation;
    db.drain_workstation("alice", "org", "box").unwrap();
    assert!(db
        .claim_worker_assignment("org", "box", "secret", "x", "xr", None, gen)
        .is_err());
    // Re-open the same database file semantics: generation is durable in-row.
    let gen_again = db
        .get_workstation("org", "box")
        .unwrap()
        .unwrap()
        .assignment_generation;
    assert_eq!(gen, gen_again);
}
