use super::*;

#[test]
fn folders_are_owner_only_versioned_durable_and_retry_safe() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("folders.db");
    let db = Store::open(&path).unwrap();
    db.bootstrap_control("owner", "org", "Org").unwrap();
    for team in ["workspace", "other"] {
        db.create_team(&crate::TeamRow {
            org_id: "org".into(),
            team_id: team.into(),
            name: team.into(),
            owner_principal_id: "owner".into(),
        })
        .unwrap();
    }
    let mut input = SaveWorkspaceFolders {
        request_id: uuid::Uuid::new_v4().to_string(),
        expected_revision: 0,
        paths: vec!["/work/research".into()],
    };
    db.register_control_principal("member").unwrap();
    db.set_organization_member("org", "member", crate::OrganizationRole::Member)
        .unwrap();
    db.add_team_member("org", "workspace", "member").unwrap();
    assert!(matches!(
        db.save_workspace_folders("member", "org", "workspace", &input),
        Err(StoreError::ControlAccessDenied)
    ));
    assert!(db.workspace_folders("member", "org", "workspace").is_err());
    assert!(matches!(
        db.save_workspace_folders("outsider", "org", "workspace", &input),
        Err(StoreError::ControlAccessDenied)
    ));
    let first = db
        .save_workspace_folders("owner", "org", "workspace", &input)
        .unwrap();
    assert_eq!(first.revision, 1);
    assert_eq!(
        db.save_workspace_folders("owner", "org", "workspace", &input)
            .unwrap(),
        first
    );
    let first_request = input.clone();
    input.paths.push("/work/website".into());
    assert!(db
        .save_workspace_folders("owner", "org", "workspace", &input)
        .is_err());
    input.request_id = uuid::Uuid::new_v4().to_string();
    assert!(db
        .save_workspace_folders("owner", "org", "workspace", &input)
        .is_err());
    input.expected_revision = 1;
    let second = db
        .save_workspace_folders("owner", "org", "workspace", &input)
        .unwrap();
    assert_eq!(
        db.save_workspace_folders("owner", "org", "workspace", &first_request)
            .unwrap(),
        first
    );
    assert_eq!(
        db.workspace_folders("owner", "org", "workspace").unwrap(),
        second
    );
    assert!(db
        .workspace_folders("owner", "org", "other")
        .unwrap()
        .paths
        .is_empty());
    assert!(db
        .workspace_folders("outsider", "org", "workspace")
        .is_err());
    drop(db);
    let db = Store::open(path).unwrap();
    assert_eq!(
        db.workspace_folders("owner", "org", "workspace").unwrap(),
        second
    );
}
