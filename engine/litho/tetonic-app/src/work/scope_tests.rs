use super::*;
use crate::resources::OrganizationRole;

#[tokio::test]
async fn non_default_scope_uses_existing_managed_execution_and_idempotency() {
    let dir = tempfile::tempdir().unwrap();
    let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        "finish",
        serde_json::json!({"summary":"Scoped result"}),
        false,
    )
    .await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let base =
                LocalWorkspace::open(dir.path().join("scope.db"), "qwen3.5:latest".into(), url)
                    .await
                    .unwrap();
            let (alice, _) = scoped(&base, "alice", "research").await;
            // Execution grants require existing organization authority. Binding a
            // team scope must not invent that permission for an ordinary member.
            base.services
                .local
                .resources()
                .set_organization_member(
                    &base.services.host.credential,
                    ORG.into(),
                    "alice".into(),
                    Some(OrganizationRole::Administrator),
                )
                .await
                .unwrap();
            let id = uuid::Uuid::new_v4().to_string();
            let admitted = alice
                .submit(id.clone(), "Answer briefly.".into())
                .await
                .unwrap();
            let result = tokio::time::timeout(std::time::Duration::from_secs(20), async {
                loop {
                    let task = alice.task(&id).await.unwrap();
                    if task.state == "completed" || task.state == "failed" {
                        break task;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            assert_eq!(
                result.state,
                "completed",
                "{}",
                serde_json::to_string(&result).unwrap()
            );
            assert_eq!(result.run_id, admitted.run_id);
            alice
                .set_local_work_data(
                    &id,
                    None,
                    Some("blocked".into()),
                    Some("presentation-lead".into()),
                    None,
                )
                .await
                .unwrap();
            let listed = alice.work_items().await.unwrap();
            let listed = listed.iter().find(|item| item.id == id).unwrap();
            assert_eq!(listed.status, result.state);
            assert_eq!(listed.agent_key.as_deref(), Some(result.agent_key.as_str()));
            assert_eq!(listed.lead_id.as_deref(), Some("presentation-lead"));
            assert_eq!(
                alice
                    .get_local_work_data(&id)
                    .await
                    .unwrap()
                    .status
                    .as_deref(),
                Some("blocked")
            );
            let retried = alice
                .submit(id.clone(), "Answer briefly.".into())
                .await
                .unwrap();
            assert_eq!(retried.run_id, result.run_id);
            assert_eq!(calls.lock().unwrap().len(), 1);
            assert!(base.task(&id).await.is_err());
            assert!(base.work_items().await.unwrap().is_empty());
            let grant = alice
                .services
                .local
                .resources()
                .get_execution_grant(
                    &alice.services.host.credential,
                    ORG.into(),
                    format!("local-ui-{id}"),
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(grant.scope.principal_id, "alice");
            assert_eq!(
                grant.scope.information_context_id,
                alice.services.scope.context()
            );
        })
        .await;
    server.abort();
}

async fn scoped(base: &WorkService, principal: &str, team: &str) -> (WorkService, String) {
    let local = base.services.local.clone();
    local.register_principal(principal.into()).await.unwrap();
    local
        .resources()
        .set_organization_member(
            &base.services.host.credential,
            ORG.into(),
            principal.into(),
            Some(OrganizationRole::TeamCreator),
        )
        .await
        .unwrap();
    let credential = local
        .credentials()
        .issue(principal.into(), 3600)
        .await
        .unwrap();
    local
        .resources()
        .create_team(
            credential.expose_secret(),
            ORG.into(),
            team.into(),
            team.into(),
        )
        .await
        .unwrap();
    let scope = local
        .application_scope(credential.expose_secret(), ORG.into(), team.into())
        .await
        .unwrap();
    let mut host = base.services.host.clone();
    host.credential = credential.expose_secret().into();
    let services = WorkspaceServices::bind(
        local,
        host,
        scope,
        base.services.keys.clone(),
        base.services.execution.clone(),
        base.services.folders.clone(),
        base.services.protected_folders.clone(),
    )
    .await
    .unwrap();
    (WorkService { services }, credential.credential_id)
}

#[tokio::test]
async fn scopes_isolate_work_metadata_and_reject_foreign_and_revoked_access() {
    let dir = tempfile::tempdir().unwrap();
    let base = LocalWorkspace::open(
        dir.path().join("scope.db"),
        "fixture-model".into(),
        "http://127.0.0.1:9".into(),
    )
    .await
    .unwrap();
    let (alice, alice_credential) = scoped(&base, "alice", "research").await;
    let (bob, _) = scoped(&base, "bob", "operations").await;
    assert_eq!(alice.services.scope.principal(), "alice");
    assert_ne!(alice.services.scope.context(), bob.services.scope.context());
    // The same work ID is valid in different security teams. Metadata must
    // stay scoped just like plans, grants, budgets and agent activation.
    let id = uuid::Uuid::new_v4().to_string();
    for (service, title) in [(&alice, "Alice's research"), (&bob, "Bob's operations")] {
        service
            .create_work_item(CreateWorkItemRequest {
                id: id.clone(),
                title: title.into(),
                goal_id: None,
                agent_key: None,
                lead_id: None,
                agent_ids: None,
            })
            .await
            .unwrap();
        service
            .set_work_item_notes(&id, vec![title.into()])
            .await
            .unwrap();
    }
    assert_eq!(
        alice.get_work_item_notes(&id).await.unwrap(),
        ["Alice's research"]
    );
    assert_eq!(
        bob.get_work_item_notes(&id).await.unwrap(),
        ["Bob's operations"]
    );
    let items = alice.work_items().await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, "Alice's research");
    assert!(base.get_work_item_notes(&id).await.is_err());
    assert!(base
        .set_work_item_notes(&id, vec!["must not write".into()])
        .await
        .is_err());
    assert!(base
        .services
        .local
        .application_scope(&bob.services.host.credential, ORG.into(), "research".into())
        .await
        .is_err());
    // A bound identifier bundle cannot be reused with another caller's credential.
    let mut mismatched = alice.clone();
    mismatched.services.host.credential = base.services.host.credential.clone();
    assert!(mismatched.work_items().await.is_err());
    alice
        .services
        .local
        .credentials()
        .revoke(alice_credential)
        .await
        .unwrap();
    assert!(alice.work_items().await.is_err());
    assert!(alice
        .set_work_item_notes(&id, vec!["revoked".into()])
        .await
        .is_err());
    assert_eq!(
        bob.get_work_item_notes(&id).await.unwrap(),
        ["Bob's operations"]
    );
}

#[tokio::test]
async fn scoped_work_services_recheck_membership_and_keep_records_after_rebinding() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scope.db");
    let base = LocalWorkspace::open(
        path.clone(),
        "fixture-model".into(),
        "http://127.0.0.1:9".into(),
    )
    .await
    .unwrap();
    let (alice, _) = scoped(&base, "alice", "research").await;
    let id = uuid::Uuid::new_v4().to_string();
    alice
        .create_work_item(CreateWorkItemRequest {
            id: id.clone(),
            title: "A scoped investigation".into(),
            goal_id: None,
            agent_key: None,
            lead_id: None,
            agent_ids: None,
        })
        .await
        .unwrap();
    alice
        .set_work_item_notes(&id, vec!["durable".into()])
        .await
        .unwrap();
    let reopened = LocalWorkspace::open(path, "fixture-model".into(), "http://127.0.0.1:9".into())
        .await
        .unwrap();
    let (again, _) = scoped(&reopened, "alice", "research").await;
    assert_eq!(again.get_work_item_notes(&id).await.unwrap(), ["durable"]);
    base.services
        .local
        .resources()
        .set_organization_member(
            &base.services.host.credential,
            ORG.into(),
            "alice".into(),
            None,
        )
        .await
        .unwrap();
    assert!(again.work_items().await.is_err());
    assert!(again.get_work_item_notes(&id).await.is_err());
}
