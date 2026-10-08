use crate::{AgentEdit, OrganizationRole, Store};

#[test]
fn edits_preserve_identity_revisions_and_registration_with_atomic_cas_and_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edits.db");
    let db = Store::open(&path).unwrap();
    db.bootstrap_control("admin", "org", "Org").unwrap();
    db.register_control_principal("member").unwrap();
    db.set_organization_member("org", "member", OrganizationRole::Member)
        .unwrap();
    let first_config = serde_json::json!({"instructions":"first","requested_tools":[]});
    let first = db
        .register_organization_agent("admin", "org", "agent", "general", &first_config)
        .unwrap();
    let config = serde_json::json!({"instructions":"second","requested_tools":["read_file"]});
    let edit = |actor, request, expected, configuration| AgentEdit {
        actor,
        org: "org",
        key: "agent",
        request,
        expected,
        harness: "general",
        configuration,
    };
    let digest = &first.identity.bound_definition_digest;
    assert!(db
        .edit_organization_agent(edit("member", "edit", digest, &config))
        .is_err());
    assert!(db
        .edit_organization_agent(edit("admin", "stale", "stale", &config))
        .is_err());
    db.conn.execute_batch("CREATE TRIGGER reject_edit BEFORE INSERT ON organization_agent_edits BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert!(db
        .edit_organization_agent(edit("admin", "edit", digest, &config))
        .is_err());
    assert_eq!(
        db.get_organization_agent("admin", "org", "agent").unwrap(),
        Some(first.clone())
    );
    let count: i64 = db
        .conn
        .query_row("SELECT count(*) FROM agent_definition_revisions", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
    db.conn.execute_batch("DROP TRIGGER reject_edit;").unwrap();
    let second = db
        .edit_organization_agent(edit("admin", "edit", digest, &config))
        .unwrap();
    assert_eq!(second.identity.identity_id, first.identity.identity_id);
    assert_ne!(second.identity.bound_definition_digest, *digest);
    assert_eq!(second.identity.toolset_subscriptions_json, "[]");
    assert_eq!(
        db.get_organization_agent_revision("admin", "org", "agent", digest)
            .unwrap(),
        Some(first.clone())
    );
    assert_eq!(
        db.edit_organization_agent(edit("admin", "edit", digest, &config))
            .unwrap(),
        second
    );
    assert!(db
        .edit_organization_agent(edit("admin", "edit", digest, &first_config))
        .is_err());
    assert!(db
        .edit_organization_agent(edit("admin", "other-tab", digest, &first_config))
        .is_err());
    // A creation retry or bootstrap must neither fail nor reset the selected revision.
    assert_eq!(
        db.register_organization_agent("admin", "org", "agent", "general", &first_config)
            .unwrap(),
        first
    );
    assert_eq!(
        db.list_organization_agents("admin", "org").unwrap()[0].1,
        second
    );
    drop(db);
    let db = Store::open(&path).unwrap();
    assert_eq!(
        db.get_organization_agent("admin", "org", "agent").unwrap(),
        Some(second.clone())
    );
    let third = db
        .edit_organization_agent(edit(
            "admin",
            "revert",
            &second.identity.bound_definition_digest,
            &first_config,
        ))
        .unwrap();
    assert_eq!(third, first);
    // A late retry returns its receipt, without reapplying the superseded configuration.
    assert_eq!(
        db.edit_organization_agent(edit("admin", "edit", digest, &config))
            .unwrap(),
        second
    );
    assert_eq!(
        db.get_organization_agent("admin", "org", "agent").unwrap(),
        Some(third)
    );
}

#[test]
fn v61_upgrades_preserve_existing_agents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("upgrade.db");
    let original;
    {
        let db = Store::open(&path).unwrap();
        db.bootstrap_control("admin", "org", "Org").unwrap();
        original = db
            .register_organization_agent(
                "admin",
                "org",
                "agent",
                "general",
                &serde_json::json!({"instructions":"keep me"}),
            )
            .unwrap();
        db.conn.execute_batch("DROP TRIGGER agent_edit_immutable; DROP TABLE organization_agent_edits; DROP TABLE organization_agent_heads; DROP TABLE plan_continuations; DELETE FROM schema_versions WHERE version>=62;").unwrap();
    }
    let db = Store::open(&path).unwrap();
    assert_eq!(
        db.get_organization_agent("admin", "org", "agent").unwrap(),
        Some(original)
    );
}
