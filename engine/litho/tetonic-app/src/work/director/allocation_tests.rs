use super::*;
use crate::resources::work_director::Allocation;

#[tokio::test]
async fn managed_allocation_patch_retains_the_entire_proposal_and_is_visible_as_repair() {
    let dir = tempfile::tempdir().unwrap();
    let mut original = plan("Compare options");
    original.token_budget = 6000;
    original.assignments[0].token_budget = 5000;
    let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(true, "sequence", json!([
        {"name":"work_plan","arguments":{"operation":"propose","direction":"Compare supplied options","plan":original}},
        {"name":"work_plan","arguments":{"operation":"resources"}},
        {"name":"work_plan","arguments":{"operation":"rebalance","direction":null,"plan":null,"work_id":null,"allocations":[{"assignment_key":"compare","token_budget":1904}]}}
    ]), false).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace =
                LocalWorkspace::open(dir.path().join("guide.db"), "qwen3.5:latest".into(), url)
                    .await
                    .unwrap();
            let source = uuid::Uuid::new_v4().to_string();
            workspace
                .submit_with_purpose(
                    source.clone(),
                    "Plan a comparison".into(),
                    shaping::GUIDE.into(),
                    None,
                    WorkPurpose::Explore,
                )
                .await
                .unwrap();
            done(&workspace, &source).await;
            let view = workspace.plan_view(&source).await.unwrap();
            let mut expected = original;
            expected.assignments[0].token_budget = 1904;
            assert_eq!(view.plans.len(), 1);
            assert_eq!(view.plans[0].content.as_ref(), Some(&expected));
            assert!(view.readiness.is_empty() && view.execution.is_none());
            let activity = workspace.task(&source).await.unwrap().guide_activity;
            assert!(activity
                .iter()
                .any(|a| a.operation == "repair" && a.state == "completed"));
            let requests = calls.lock().unwrap();
            assert_eq!(requests.len(), 4);
            assert!(requests[1]["messages"]
                .to_string()
                .contains("estimated minimum"));
        })
        .await;
    server.abort();
}

#[tokio::test]
async fn allocation_patch_is_bound_to_original_keys_turn_and_existing_retry_limit() {
    let dir = tempfile::tempdir().unwrap();
    let (url, _, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        "finish",
        json!({"summary":"Unused"}),
        false,
    )
    .await;
    let workspace = LocalWorkspace::open(dir.path().join("guide.db"), "qwen3.5:latest".into(), url)
        .await
        .unwrap();
    let source = proposal_validation::discussion(&workspace).await;
    let (_, session) = workspace.bind_director(&source).await.unwrap();
    let mut original = plan("Original promise");
    original.token_budget = 6000;
    original.assignments[0].token_budget = 5000;
    workspace
        .director_propose(&session.binding, "Original direction".into(), original)
        .await
        .unwrap_err();
    let other = proposal_validation::discussion(&workspace).await;
    let (_, other_session) = workspace.bind_director(&other).await.unwrap();
    let patch = || {
        vec![Allocation {
            assignment_key: "compare".into(),
            token_budget: 1904,
        }]
    };
    assert!(workspace
        .director_rebalance(&other_session.binding, "cross-turn", patch())
        .await
        .is_err());
    let result = workspace
        .director_rebalance(
            &session.binding,
            "bad-keys",
            vec![Allocation {
                assignment_key: "other".into(),
                token_budget: 1904,
            }],
        )
        .await
        .unwrap();
    assert_eq!(result["proposal_state"], "repair_failed");
    let exhausted = workspace
        .director_rebalance(&session.binding, "third", patch())
        .await
        .unwrap();
    assert_eq!(exhausted["proposal_state"], "repair_failed");
    assert!(workspace.plan_view(&source).await.unwrap().plans.is_empty());
    assert!(workspace.work_briefs(&source).await.unwrap().is_empty());
    server.abort();
}
