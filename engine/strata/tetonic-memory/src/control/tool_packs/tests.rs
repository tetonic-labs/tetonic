use super::*;
use crate::{OrganizationRole, TeamRow};

#[test]
fn revisions_survive_restart_and_are_scoped_idempotent_and_immutable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("packs.db");
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
    let input = SaveToolPack {
        request_id: uuid::Uuid::new_v4().to_string(),
        id: uuid::Uuid::new_v4().to_string(),
        expected_revision: 0,
        name: "Research".into(),
        description: "Find evidence".into(),
        tools: vec!["grep".into(), "read_file".into(), "grep".into()],
    };
    let one = db.save_tool_pack("alice", "org", "team", &input).unwrap();
    assert_eq!(one.tools, vec!["grep", "read_file"]);
    assert_eq!(
        one,
        db.save_tool_pack("alice", "org", "team", &input).unwrap()
    );
    assert!(db.save_tool_pack("bob", "org", "team", &input).is_err());
    assert!(db.tool_packs("bob", "org", "team").is_err());
    assert!(db
        .tool_pack_revision("alice", "org", "other", &input.id, 1)
        .unwrap()
        .is_none());
    let mut edit = input.clone();
    edit.request_id = uuid::Uuid::new_v4().to_string();
    edit.expected_revision = 1;
    edit.tools.push("run_shell".into());
    let two = db.save_tool_pack("alice", "org", "team", &edit).unwrap();
    assert_eq!(two.revision, 2);
    assert_eq!(
        one,
        db.save_tool_pack("alice", "org", "team", &input).unwrap()
    );
    edit.request_id = uuid::Uuid::new_v4().to_string();
    assert!(matches!(
        db.save_tool_pack("alice", "org", "team", &edit),
        Err(StoreError::ControlResourceConflict)
    ));
    edit.request_id = input.request_id.clone();
    edit.expected_revision = 0;
    assert!(db.save_tool_pack("alice", "org", "team", &edit).is_err());
    assert!(db
        .conn
        .execute("DELETE FROM tool_pack_versions", [])
        .is_err());
    drop(db);
    let db = Store::open(path).unwrap();
    assert_eq!(db.tool_packs("alice", "org", "team").unwrap(), vec![two]);
    assert_eq!(
        db.tool_pack_revision("alice", "org", "team", &input.id, 1)
            .unwrap(),
        Some(one)
    );
}
