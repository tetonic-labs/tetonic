use super::*;

#[tokio::test]
async fn managed_guide_repairs_malformed_assignment_keys_without_extra_work() {
    let dir = tempfile::tempdir().unwrap();
    let valid = plan("Compare workshop options");
    let mut malformed = valid.clone();
    malformed.assignments[0].key = "Format analyst".into();
    let propose = |content| json!({"name":"work_plan","arguments":{"operation":"propose","direction":"Compare the supplied workshop options","plan":content,"work_id":null}});
    let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        "sequence",
        json!([propose(malformed), propose(valid.clone())]),
        false,
    )
    .await;
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
            let requests = calls.lock().unwrap();
            assert_eq!(requests.len(), 3);
            let feedback = requests[1]["messages"].to_string();
            assert!(feedback.contains("assignments[0].key") && feedback.contains("no spaces"));
            drop(requests);
            let view = workspace.plan_view(&source).await.unwrap();
            assert_eq!(view.plans.len(), 1);
            assert_eq!(view.plans[0].content.as_ref(), Some(&valid));
            assert!(view.execution.is_none() && view.readiness.is_empty());
            assert_eq!(workspace.work_briefs(&source).await.unwrap().len(), 1);
            assert!(workspace
                .snapshot()
                .await
                .unwrap()
                .tasks
                .iter()
                .all(|t| t.purpose == WorkPurpose::Explore));
        })
        .await;
    server.abort();
}

#[tokio::test]
async fn managed_guide_corrects_once_and_preserves_scope_without_starting_work() {
    for changes_scope in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut original = plan("Compare workshop options");
        original.token_budget = 1000;
        let mut corrected = original.clone();
        corrected.assignments[0].token_budget = 744;
        if changes_scope {
            corrected.assignments[0].deliverable = "A smaller promise".into();
        }
        let propose = |plan: &tetonic_memory::PlanContent| json!({"name":"work_plan","arguments":{"operation":"propose","direction":"Compare the workshop options","plan":plan}});
        let mut sequence = vec![propose(&original), propose(&corrected)];
        if changes_scope {
            let mut third = original.clone();
            third.assignments[0].token_budget = 744;
            sequence.push(propose(&third));
        }
        let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(
            true,
            "sequence",
            json!(sequence),
            false,
        )
        .await;
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
                assert!(view.execution.is_none());
                assert_eq!(view.plans.len(), usize::from(!changes_scope));
                assert_eq!(
                    workspace.work_briefs(&source).await.unwrap().len(),
                    usize::from(!changes_scope)
                );
                let requests = calls.lock().unwrap().clone();
                assert_eq!(requests.len(), if changes_scope { 4 } else { 3 });
                assert!(requests[1]["messages"]
                    .to_string()
                    .contains("repair_needed"));
                let activity = workspace.task(&source).await.unwrap().guide_activity;
                assert!(activity.iter().any(|a| a.operation == "repair"
                    && a.state
                        == if changes_scope {
                            "repair_failed"
                        } else {
                            "completed"
                        }));
                if !changes_scope {
                    assert_eq!(view.plans[0].content.as_ref(), Some(&corrected));
                    assert!(view.readiness.is_empty());
                } else {
                    assert!(requests.last().unwrap()["messages"]
                        .to_string()
                        .contains("Stop proposing"));
                }
                assert!(workspace
                    .snapshot()
                    .await
                    .unwrap()
                    .tasks
                    .iter()
                    .all(|t| t.purpose == WorkPurpose::Explore));
            })
            .await;
        server.abort();
    }
}

async fn discussion(workspace: &LocalWorkspace) -> String {
    let source = uuid::Uuid::new_v4().to_string();
    workspace
        .services
        .local
        .resources()
        .create_team_work_item_for_purpose(
            &workspace.services.host.credential,
            crate::resources::CreateTeamWorkItem {
                org: ORG.into(),
                team: TEAM.into(),
                work_id: source.clone(),
                title: "Compare options".into(),
                request_id: format!("{source}@{}", shaping::GUIDE),
                goal_id: None,
            },
            Some("Compare options".into()),
            WorkPurpose::Explore,
        )
        .await
        .unwrap();
    source
}

#[tokio::test]
async fn invalid_allocations_leave_no_brief_or_proposal_and_same_total_correction_is_reviewable() {
    let dir = tempfile::tempdir().unwrap();
    // Discovery is served by the fixture; these operations never run inference.
    let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        "finish",
        json!({"summary":"Unused"}),
        false,
    )
    .await;
    let workspace = LocalWorkspace::open(
        dir.path().join("validation.db"),
        "qwen3.5:latest".into(),
        url,
    )
    .await
    .unwrap();
    let source = discussion(&workspace).await;
    let (_, session) = workspace.bind_director(&source).await.unwrap();
    let mut candidate = plan("Workshop options");
    candidate.token_budget = 1000;
    for coordination in [
        0,
        255,
        workspace.services.execution.coordination_tokens() + 1,
    ] {
        let mut invalid = candidate.clone();
        invalid.token_budget = 1000 + coordination;
        let error = workspace
            .validate_plan_proposal(&source, &invalid)
            .await
            .unwrap_err();
        assert!(error.employee_message().contains("coordination"));
        assert!(workspace.work_briefs(&source).await.unwrap().is_empty());
        assert!(workspace.plan_view(&source).await.unwrap().plans.is_empty());
    }
    let worker = workspace
        .services
        .agents()
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.key == AGENT)
        .unwrap();
    let mut over_worker = candidate.clone();
    over_worker.assignments[0].token_budget = worker.max_tokens + 1;
    over_worker.token_budget = worker.max_tokens + 257;
    assert!(workspace
        .validate_plan_proposal(&source, &over_worker)
        .await
        .unwrap_err()
        .employee_message()
        .contains("per-run allowance"));
    workspace
        .set_budget_settings(BudgetSettingsRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_revision: 0,
            token_limit: Some(1000),
        })
        .await
        .unwrap();
    assert!(workspace
        .validate_plan_proposal(&source, &plan("Too much"))
        .await
        .unwrap_err()
        .employee_message()
        .contains("workspace allowance"));
    assert!(workspace.work_briefs(&source).await.unwrap().is_empty());

    // Rebalance only the worker allowance, preserving the total, contributor,
    // instructions and deliverable. The host never silently rewrites a plan.
    candidate.assignments[0].token_budget = 744;
    let receipt = workspace
        .director_propose(&session.binding, "Shared".into(), candidate.clone())
        .await
        .unwrap();
    assert_eq!(
        receipt["allowance"],
        json!({"total_tokens":1000,"worker_tokens":744,"coordination_tokens":256,"reserved":false})
    );
    assert_eq!(receipt["work_launched"], false);
    let view = workspace.plan_view(&source).await.unwrap();
    assert_eq!(view.plans[0].content.as_ref(), Some(&candidate));
    assert!(view.readiness.is_empty());
    assert!(!view.execution_available && view.execution.is_none());

    let revise_id = uuid::Uuid::new_v4().to_string();
    let mut revised = candidate;
    revised.summary = "A reviewed approach".into();
    let command = || PlanCommand::Revise {
        request_id: revise_id.clone(),
        expected_revision: 1,
        brief_revision: 1,
        content: revised.clone(),
    };
    let saved = workspace.update_plan(&source, command()).await.unwrap();
    let agreement = uuid::Uuid::new_v4().to_string();
    let agree = || PlanCommand::Agree {
        request_id: agreement.clone(),
        revision: 2,
    };
    let agreed = workspace.update_plan(&source, agree()).await.unwrap();
    workspace
        .set_budget_settings(BudgetSettingsRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_revision: 1,
            token_limit: Some(999),
        })
        .await
        .unwrap();
    // Settings changed after the writes; exact lost replies still replay while
    // current launch readiness is blocked. No second revision or agreement.
    assert_eq!(
        workspace
            .update_plan(&source, command())
            .await
            .unwrap()
            .content,
        saved.content
    );
    assert_eq!(
        workspace.update_plan(&source, agree()).await.unwrap(),
        agreed
    );
    let view = workspace.plan_view(&source).await.unwrap();
    assert_eq!(view.plans.len(), 2);
    assert!(!view.execution_available);
    assert!(view
        .readiness
        .iter()
        .any(|r| r.contains("workspace allowance")));
    assert_eq!(
        workspace
            .director_propose(
                &session.binding,
                "Shared".into(),
                view.plans[1].content.clone().unwrap(),
            )
            .await
            .unwrap(),
        receipt
    );
    assert!(workspace
        .update_plan(
            &source,
            PlanCommand::Revise {
                request_id: revise_id,
                expected_revision: 1,
                brief_revision: 1,
                content: plan("Changed retry"),
            }
        )
        .await
        .is_err());
    assert!(calls.lock().unwrap().is_empty());
    server.abort();
}

#[tokio::test]
async fn managed_guide_receives_allocation_error_without_saving_or_dispatching() {
    let dir = tempfile::tempdir().unwrap();
    let mut invalid = plan("No coordination");
    invalid.token_budget = 1000;
    let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        crate::resources::work_director::CONTROL,
        json!({"operation":"propose","direction":"Compare the options","plan":invalid}),
        false,
    )
    .await;
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
            let requests = calls.lock().unwrap().clone();
            assert_eq!(requests.len(), 2);
            let messages = requests[1]["messages"].to_string();
            assert!(messages.contains("leaves 0 tokens for coordination"));
            assert!(messages.contains("without increasing the total"));
            assert!(workspace.work_briefs(&source).await.unwrap().is_empty());
            let view = workspace.plan_view(&source).await.unwrap();
            assert!(view.plans.is_empty() && view.execution.is_none());
            assert!(workspace
                .snapshot()
                .await
                .unwrap()
                .tasks
                .iter()
                .all(|t| t.purpose == WorkPurpose::Explore));
            let activity = workspace.task(&source).await.unwrap().guide_activity;
            assert!(activity
                .iter()
                .any(|a| a.operation == "propose" && a.state == "repair_needed"));
        })
        .await;
    server.abort();
}

#[tokio::test]
async fn structured_generation_cannot_capture_a_zero_coordination_proposal() {
    let dir = tempfile::tempdir().unwrap();
    let mut invalid = plan("No coordination");
    invalid.token_budget = 1000;
    let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        false,
        "structured",
        json!({"summary":serde_json::to_string(&invalid).unwrap()}),
        false,
    )
    .await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace =
                LocalWorkspace::open(dir.path().join("capture.db"), "qwen3.5:latest".into(), url)
                    .await
                    .unwrap();
            let source = discussion(&workspace).await;
            workspace
                .set_budget_settings(BudgetSettingsRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    expected_revision: 0,
                    token_limit: Some(2000),
                })
                .await
                .unwrap();
            let proposal = workspace
                .update_plan(
                    &source,
                    PlanCommand::Prepare {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        expected_revision: 0,
                        expected_brief_revision: 0,
                        body: "Compare the supplied options".into(),
                    },
                )
                .await
                .unwrap();
            done(&workspace, &proposal.generation_id).await;
            assert!(calls.lock().unwrap()[0]["messages"]
                .to_string()
                .contains("must not exceed 2000 tokens"));
            for _ in 0..2 {
                let error = workspace
                    .update_plan(&source, PlanCommand::Capture { revision: 1 })
                    .await
                    .unwrap_err();
                assert!(error
                    .employee_message()
                    .contains("leaves 0 tokens for coordination"));
            }
            let view = workspace.plan_view(&source).await.unwrap();
            assert_eq!(view.plans[0].status, "drafting");
            assert!(
                view.plans[0].content.is_none()
                    && !view.execution_available
                    && view.execution.is_none()
            );
            assert_eq!(
                calls.lock().unwrap().len(),
                1,
                "Capture retries never start extra inference"
            );
            assert_eq!(
                view.generation.unwrap().messages.last().unwrap().content,
                serde_json::to_string(&invalid).unwrap()
            );
        })
        .await;
    server.abort();
}

#[tokio::test]
async fn selected_roster_limits_schema_and_rejection_does_not_save_a_brief() {
    let dir = tempfile::tempdir().unwrap();
    let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        "finish",
        json!({"summary":"Unused"}),
        false,
    )
    .await;
    let workspace =
        LocalWorkspace::open(dir.path().join("roster.db"), "qwen3.5:latest".into(), url)
            .await
            .unwrap();
    let outsider = workspace
        .create_agent(CreateLocalAgent {
            workspace_root: None,
            provider: "ollama".into(),
            hosted_consent: false,
            hosted_tools_consent: false,
            expected_workspace_root: None,
            request_id: uuid::Uuid::new_v4().to_string(),
            name: "Other reviewer".into(),
            purpose: "Review supplied evidence".into(),
            model: "qwen3.5:latest".into(),
            harness: "general".into(),
            max_steps: 4,
            max_seconds: 120,
            max_tokens: 4096,
            tools: Some(vec![]),
        })
        .await
        .unwrap();
    let team = workspace
        .save_work_team(SaveWorkTeam {
            id: uuid::Uuid::new_v4().to_string(),
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_revision: 0,
            name: "Chosen team".into(),
            purpose: "Compare options".into(),
            agent_keys: vec![AGENT.into()],
        })
        .await
        .unwrap();
    let source = uuid::Uuid::new_v4().to_string();
    workspace
        .services
        .local
        .resources()
        .create_work_with_roster(
            &workspace.services.host.credential,
            crate::resources::CreateTeamWorkItem {
                org: ORG.into(),
                team: TEAM.into(),
                work_id: source.clone(),
                title: "Compare options".into(),
                request_id: format!("{source}@{}", shaping::GUIDE),
                goal_id: None,
            },
            Some("Compare options".into()),
            WorkPurpose::Explore,
            Some((
                Some(WorkTeamSelection {
                    id: team.id,
                    revision: team.revision,
                }),
                None,
            )),
        )
        .await
        .unwrap();
    let (_, session) = workspace.bind_director(&source).await.unwrap();
    assert_eq!(
        session.binding.plan_schema["properties"]["assignments"]["items"]["properties"]
            ["agent_key"]["enum"],
        json!([AGENT])
    );
    for name in [outsider.key, outsider.name] {
        let mut invalid = plan("Wrong team");
        invalid.assignments[0].agent_key = name;
        assert!(workspace
            .director_propose(&session.binding, "Wrong shared brief".into(), invalid)
            .await
            .is_err());
    }
    assert!(workspace.work_briefs(&source).await.unwrap().is_empty());
    assert!(workspace.plan_view(&source).await.unwrap().plans.is_empty());
    workspace
        .director_propose(&session.binding, "Right team".into(), plan("Valid"))
        .await
        .unwrap();
    assert_eq!(workspace.work_briefs(&source).await.unwrap().len(), 1);
    assert!(calls.lock().unwrap().is_empty());
    server.abort();
}
