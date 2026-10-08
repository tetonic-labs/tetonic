use super::*;
#[test]
fn scoped_connections_survive_restart_and_reject_stale_or_redirected_edits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mcp.db");
    let db = Store::open(&path).unwrap();
    db.bootstrap_control("alice", "org", "Org").unwrap();
    for team in ["team", "other"] {
        db.create_team(&crate::TeamRow {
            org_id: "org".into(),
            team_id: team.into(),
            name: team.into(),
            owner_principal_id: "alice".into(),
        })
        .unwrap();
    }
    let mut record = WorkspaceMcpConnection {
        id: "calendar".into(),
        name: "Calendar".into(),
        endpoint: "https://example.com/mcp".into(),
        auth: "bearer".into(),
        secret_ref: Some("opaque-vault-reference".into()),
        revision: 1,
        binding_epoch: 1,
        enabled: true,
        approved_manifests: vec![],
    };
    db.save_workspace_mcp("alice", "org", "team", &record, 0)
        .unwrap();
    assert!(db.workspace_mcp_connections("bob", "org", "team").is_err());
    assert!(db
        .workspace_mcp_connections("alice", "org", "other")
        .unwrap()
        .is_empty());
    assert!(db
        .save_workspace_mcp("alice", "org", "team", &record, 0)
        .is_err());
    assert!(!serde_json::to_string(&record)
        .unwrap()
        .contains("opaque-vault"));
    drop(db);
    let db = Store::open(&path).unwrap();
    assert_eq!(
        db.workspace_mcp_connections("alice", "org", "team")
            .unwrap()[0]
            .secret_ref,
        record.secret_ref
    );
    record.revision = 2;
    record.endpoint = "https://other.example/mcp".into();
    assert!(db
        .save_workspace_mcp("alice", "org", "team", &record, 1)
        .is_err());
    record.endpoint = "https://example.com/mcp".into();
    record.secret_ref = Some("other-account".into());
    assert!(db
        .save_workspace_mcp("alice", "org", "team", &record, 1)
        .is_err());
    record.binding_epoch = 2;
    db.save_workspace_mcp("alice", "org", "team", &record, 1)
        .unwrap();
    record.revision = 3;
    record.binding_epoch = 3;
    record.enabled = false;
    record.secret_ref = None;
    db.save_workspace_mcp("alice", "org", "team", &record, 2)
        .unwrap();
    assert!(
        !db.workspace_mcp_connections("alice", "org", "team")
            .unwrap()[0]
            .enabled
    );
}
