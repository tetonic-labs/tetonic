use super::*;
use crate::{OrganizationRole, TeamRow};

#[test]
fn skills_survive_restart_are_scoped_and_import_retries_do_not_undo_revocation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("skills.db");
    let db = Store::open(&path).unwrap();
    db.bootstrap_control("alice", "org", "Org").unwrap();
    db.register_control_principal("bob").unwrap();
    db.set_organization_member("org", "bob", OrganizationRole::Member)
        .unwrap();
    for team in ["team", "other"] {
        db.create_team(&TeamRow {
            org_id: "org".into(),
            team_id: team.into(),
            name: team.into(),
            owner_principal_id: "alice".into(),
        })
        .unwrap();
    }
    let import = |db: &Store| {
        db.import_workspace_skill(
            "alice",
            "org",
            "team",
            "research",
            "Find evidence",
            "Written here",
            "Reviewed instructions",
        )
    };
    let id = import(&db).unwrap();
    assert!(id.len() <= 64);
    assert!(db
        .workspace_skill_content("bob", "org", "team", &id, true)
        .is_err());
    assert!(db
        .workspace_skill_content("alice", "org", "other", &id, true)
        .unwrap()
        .is_none());
    assert!(db
        .import_workspace_skill("bob", "org", "team", "x", "x", "", "x")
        .is_err());
    drop(db);
    let db = Store::open(&path).unwrap();
    assert_eq!(
        db.workspace_skill_content("alice", "org", "team", &id, true)
            .unwrap()
            .as_deref(),
        Some("Reviewed instructions")
    );
    db.revoke_workspace_skill("alice", "org", "team", &id)
        .unwrap();
    assert_eq!(id, import(&db).unwrap());
    assert!(db
        .workspace_skill_content("alice", "org", "team", &id, true)
        .unwrap()
        .is_none());
    assert!(db
        .workspace_skill_content("alice", "org", "team", &id, false)
        .unwrap()
        .is_some());
    assert!(!db.workspace_skills("alice", "org", "team").unwrap()[0].enabled);
    let changed = db
        .import_workspace_skill(
            "alice",
            "org",
            "team",
            "research",
            "Find evidence",
            "Written here",
            "Different instructions",
        )
        .unwrap();
    assert_ne!(id, changed);
}
