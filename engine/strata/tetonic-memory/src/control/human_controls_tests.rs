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
fn parent_stop_blocks_descendants_and_parks_siblings_independently() {
    let db = primed();
    db.create_team_goal("alice", "org", "team", "g1", "Goal")
        .unwrap();
    db.create_team_work_item(crate::CreateTeamWorkItem {
        actor: "alice",
        org: "org",
        team: "team",
        work_id: "a",
        title: "A",
        request_id: "ra",
        goal_id: Some("g1"),
    })
    .unwrap();
    db.create_team_work_item(crate::CreateTeamWorkItem {
        actor: "alice",
        org: "org",
        team: "team",
        work_id: "b",
        title: "B",
        request_id: "rb",
        goal_id: None,
    })
    .unwrap();
    let stop = db
        .request_control_stop("alice", "org", "goal", "g1", "pause", "wait")
        .unwrap();
    assert_eq!(stop.mode, "pause");
    assert!(db
        .activation_blocked_by_stop("org", "team", "a", Some("g1"), None)
        .unwrap()
        .is_some());
    assert!(db
        .activation_blocked_by_stop("org", "team", "b", None, None)
        .unwrap()
        .is_none());
    let listed = db.list_team_work_items("alice", "org", "team").unwrap();
    assert_eq!(
        listed.iter().find(|w| w.work_id == "a").unwrap().status,
        "parked"
    );
    assert_eq!(
        listed.iter().find(|w| w.work_id == "b").unwrap().status,
        "open"
    );
    db.clear_control_stop("alice", "org", "goal", "g1").unwrap();
    assert!(db
        .activation_blocked_by_stop("org", "team", "a", Some("g1"), None)
        .unwrap()
        .is_none());
}

#[test]
fn rejected_expired_and_changed_approvals_never_dispatch() {
    let db = primed();
    let proposed = db
        .propose_effect_approval(crate::ProposeEffectApproval {
            proposal: None,
            actor: "alice",
            org: "org",
            team: "team",
            approval_id: "ap1",
            proposal_digest: "digest-a",
            request_id: "req-a",
            expires_at: 2_000_000_000,
            work_id: None,
        })
        .unwrap();
    assert_eq!(proposed.status, "pending");
    assert!(!db
        .effect_approval_allows_dispatch("org", "team", "ap1", "digest-a", 1_000)
        .unwrap());
    db.resolve_effect_approval(crate::ResolveEffectApproval {
        actor: "bob",
        org: "org",
        team: "team",
        approval_id: "ap1",
        proposal_digest: "digest-a",
        allow: true,
        now_unix: 1_000,
    })
    .unwrap();
    assert!(db
        .effect_approval_allows_dispatch("org", "team", "ap1", "digest-a", 1_000)
        .unwrap());
    assert!(!db
        .effect_approval_allows_dispatch("org", "team", "ap1", "digest-b", 1_000)
        .unwrap());
    assert!(db
        .resolve_effect_approval(crate::ResolveEffectApproval {
            actor: "bob",
            org: "org",
            team: "team",
            approval_id: "ap1",
            proposal_digest: "digest-b",
            allow: true,
            now_unix: 1_000
        })
        .is_err());
    db.propose_effect_approval(crate::ProposeEffectApproval {
        proposal: None,
        actor: "alice",
        org: "org",
        team: "team",
        approval_id: "ap2",
        proposal_digest: "digest-c",
        request_id: "req-c",
        expires_at: 2_000_000_000,
        work_id: None,
    })
    .unwrap();
    db.resolve_effect_approval(crate::ResolveEffectApproval {
        actor: "alice",
        org: "org",
        team: "team",
        approval_id: "ap2",
        proposal_digest: "digest-c",
        allow: false,
        now_unix: 1_000,
    })
    .unwrap();
    assert!(!db
        .effect_approval_allows_dispatch("org", "team", "ap2", "digest-c", 1_000)
        .unwrap());
    db.propose_effect_approval(crate::ProposeEffectApproval {
        proposal: None,
        actor: "alice",
        org: "org",
        team: "team",
        approval_id: "ap3",
        proposal_digest: "digest-d",
        request_id: "req-d",
        expires_at: 50,
        work_id: None,
    })
    .unwrap();
    assert!(db
        .resolve_effect_approval(crate::ResolveEffectApproval {
            actor: "alice",
            org: "org",
            team: "team",
            approval_id: "ap3",
            proposal_digest: "digest-d",
            allow: true,
            now_unix: 100
        })
        .is_err());
    assert!(!db
        .effect_approval_allows_dispatch("org", "team", "ap3", "digest-d", 100)
        .unwrap());
}

#[test]
fn effort_unknown_is_not_zero_and_team_inspection_has_no_private_bodies() {
    let db = primed();
    let unknown = db
        .record_team_effort(crate::RecordTeamEffort {
            actor: "alice",
            org: "org",
            team: "team",
            entry_id: "e1",
            request_id: "er1",
            measured_tokens: None,
            goal_id: None,
            work_id: None,
        })
        .unwrap();
    assert_eq!(unknown.status, "unknown");
    assert!(unknown.measured_tokens.is_none());
    let measured = db
        .record_team_effort(crate::RecordTeamEffort {
            actor: "alice",
            org: "org",
            team: "team",
            entry_id: "e2",
            request_id: "er2",
            measured_tokens: Some(40),
            goal_id: None,
            work_id: None,
        })
        .unwrap();
    assert_eq!(measured.measured_tokens, Some(40));
    let view = db.inspect_team_work("bob", "org", "team").unwrap();
    assert_eq!(view.effort.len(), 2);
    let encoded = serde_json::to_string(&view).unwrap();
    assert!(!encoded.contains("PRIVATE"));
    assert!(!encoded.contains("secret"));
}
