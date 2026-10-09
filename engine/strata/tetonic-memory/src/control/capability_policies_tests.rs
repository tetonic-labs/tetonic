use super::capability_policies::*;
use crate::{CreateTeamWorkItem, SaveWorkTeam, Store, TeamRow, WorkPurpose, WorkTeamSelection};
use tetonic_domain::{ActionKind, ApprovalRequirement};
use tetonic_policy::capabilities::{
    capability_outcome, AutonomyTier, Capability, CapabilityDecision, CapabilityPolicy,
};

fn save(
    scope: CapabilityScope,
    id: &str,
    revision: i64,
    tier: AutonomyTier,
) -> SaveCapabilityPolicy {
    SaveCapabilityPolicy {
        scope,
        scope_id: id.into(),
        expected_revision: revision,
        request_id: uuid::Uuid::new_v4().to_string(),
        policy: Some(CapabilityPolicy {
            tier,
            ..Default::default()
        }),
    }
}
#[test]
fn capability_policies_are_durable_scoped_and_compare_and_swap() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("permissions.db");
    let db = Store::open(&path).unwrap();
    db.bootstrap_control("owner", "org", "Org").unwrap();
    for team in ["workspace", "other"] {
        db.create_team(&TeamRow {
            org_id: "org".into(),
            team_id: team.into(),
            name: team.into(),
            owner_principal_id: "owner".into(),
        })
        .unwrap();
    }
    let agent = db
        .register_organization_agent(
            "owner",
            "org",
            "robin",
            "general",
            &serde_json::json!({"tools":["finish"]}),
        )
        .unwrap();
    let team = db
        .save_work_team(
            "owner",
            "org",
            "workspace",
            &SaveWorkTeam {
                id: "research".into(),
                request_id: "create".into(),
                expected_revision: 0,
                name: "Research".into(),
                purpose: "Research".into(),
                agent_keys: vec!["robin".into()],
            },
        )
        .unwrap();
    for (id, roster) in [
        (
            "team-work",
            Some(WorkTeamSelection {
                id: team.id.clone(),
                revision: team.revision,
            }),
        ),
        ("solo-work", None),
    ] {
        db.create_rostered_work(
            CreateTeamWorkItem {
                actor: "owner",
                org: "org",
                team: "workspace",
                work_id: id,
                title: id,
                request_id: id,
                goal_id: None,
            },
            Some("Research"),
            WorkPurpose::Explore,
            roster.as_ref(),
            None,
        )
        .unwrap();
    }
    let workspace = save(
        CapabilityScope::Workspace,
        "",
        0,
        AutonomyTier::ReviewChanges,
    );
    let first = db
        .save_capability_policy("owner", "org", "workspace", &workspace)
        .unwrap();
    assert_eq!(
        db.save_capability_policy("owner", "org", "workspace", &workspace)
            .unwrap(),
        first
    );
    assert!(db
        .save_capability_policy(
            "owner",
            "org",
            "workspace",
            &save(CapabilityScope::Workspace, "", 0, AutonomyTier::Automatic)
        )
        .is_err());
    let mut changed_retry = workspace.clone();
    changed_retry.policy = None;
    assert!(db
        .save_capability_policy("owner", "org", "workspace", &changed_retry)
        .is_err());
    db.save_capability_policy(
        "owner",
        "org",
        "workspace",
        &save(
            CapabilityScope::Agent,
            &agent.identity.identity_id,
            0,
            AutonomyTier::Automatic,
        ),
    )
    .unwrap();
    db.save_capability_policy(
        "owner",
        "org",
        "workspace",
        &save(CapabilityScope::Team, &team.id, 0, AutonomyTier::ReadOnly),
    )
    .unwrap();
    let evaluate = |db: &Store, work| {
        capability_outcome(
            &db.work_capability_policies(
                "owner",
                "org",
                "workspace",
                work,
                &agent.identity.identity_id,
            )
            .unwrap(),
            &ActionKind::WriteFile,
        )
    };
    assert!(!evaluate(&db, "team-work").decision.allowed());
    assert_eq!(
        evaluate(&db, "solo-work").approval,
        ApprovalRequirement::Interactive
    );
    assert!(db
        .capability_policies("owner", "org", "other")
        .unwrap()
        .is_empty());
    db.register_control_principal("outsider").unwrap();
    assert!(db
        .capability_policies("outsider", "org", "workspace")
        .is_err());
    assert!(db
        .save_capability_policy("outsider", "org", "workspace", &workspace)
        .is_err());
    assert!(db
        .save_capability_policy(
            "owner",
            "org",
            "workspace",
            &save(
                CapabilityScope::Agent,
                "invented",
                0,
                AutonomyTier::Automatic
            )
        )
        .is_err());
    drop(db);
    let db = Store::open(&path).unwrap();
    assert!(!evaluate(&db, "team-work").decision.allowed());
    let mut restrict = save(
        CapabilityScope::Agent,
        &agent.identity.identity_id,
        1,
        AutonomyTier::Automatic,
    );
    restrict
        .policy
        .as_mut()
        .unwrap()
        .overrides
        .insert(Capability::FileRead, CapabilityDecision::Deny);
    db.save_capability_policy("owner", "org", "workspace", &restrict)
        .unwrap();
    assert!(!capability_outcome(
        &db.work_capability_policies(
            "owner",
            "org",
            "workspace",
            "solo-work",
            &agent.identity.identity_id
        )
        .unwrap(),
        &ActionKind::ReadFile
    )
    .decision
    .allowed());
    restrict.expected_revision = 2;
    restrict.request_id = uuid::Uuid::new_v4().to_string();
    restrict.policy = None;
    db.save_capability_policy("owner", "org", "workspace", &restrict)
        .unwrap();
    assert!(capability_outcome(
        &db.work_capability_policies(
            "owner",
            "org",
            "workspace",
            "solo-work",
            &agent.identity.identity_id
        )
        .unwrap(),
        &ActionKind::ReadFile
    )
    .decision
    .allowed());
}
