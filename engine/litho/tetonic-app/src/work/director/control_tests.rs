use super::*;
use serde_json::json;

fn plan(title: &str) -> tetonic_memory::PlanContent {
    serde_json::from_value(json!({"title":title,"summary":"Compare the supplied options.","token_budget":3000,"open_questions":[],"assignments":[
        {"key":"compare","title":"Compare formats","instructions":"Use only the supplied workshop information.","agent_key":AGENT,"depends_on":[],"tools":[],"deliverable":"A short comparison","token_budget":1000}
    ]})).unwrap()
}

async fn done(workspace: &LocalWorkspace, id: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let task = workspace.task(id).await.unwrap();
            if task.state == "completed" {
                break;
            }
            assert!(
                !matches!(task.state.as_str(), "failed" | "canceled"),
                "{:?}",
                task.error
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn guide_tool_creates_and_revises_the_same_durable_proposal_without_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("guide.db");
    let source = uuid::Uuid::new_v4().to_string();
    let followup = uuid::Uuid::new_v4().to_string();
    tokio::task::LocalSet::new()
        .run_until(async {
            for (index, (turn, parent, title)) in [
                (source.clone(), None, "Workshop options"),
                (followup.clone(), Some(source.clone()), "A smaller workshop"),
            ]
            .into_iter()
            .enumerate()
            {
                let expected = plan(title);
                let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(
                    index == 0,
                    crate::resources::work_director::CONTROL,
                    json!({"operation":"propose","direction":title,"plan":expected}),
                    false,
                )
                .await;
                let workspace =
                    LocalWorkspace::open(database.clone(), "qwen3.5:latest".into(), url)
                        .await
                        .unwrap();
                let agents = workspace.services.agents().await.unwrap();
                assert_eq!(
                    agents
                        .iter()
                        .find(|a| a.key == shaping::GUIDE)
                        .unwrap()
                        .max_tokens,
                    LOCAL_TOKEN_CEILING
                );
                assert_eq!(
                    agents.iter().find(|a| a.key == AGENT).unwrap().max_tokens,
                    DEFAULT_WORK_TOKENS
                );
                if index == 1 {
                    workspace
                        .set_budget_settings(BudgetSettingsRequest {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            expected_revision: 0,
                            token_limit: Some(1024),
                        })
                        .await
                        .unwrap();
                }
                workspace
                    .submit_with_purpose(
                        turn.clone(),
                        format!("Plan {title}. PRIVATE_NOTE_NOT_FOR_TEAM"),
                        shaping::GUIDE.into(),
                        parent,
                        WorkPurpose::Explore,
                    )
                    .await
                    .unwrap();
                done(&workspace, &turn).await;
                let view = workspace.plan_view(&source).await.unwrap();
                assert_eq!(view.plans.len(), index + 1);
                assert_eq!(view.plans[0].content.as_ref(), Some(&expected));
                assert_eq!(view.plans[0].status, "draft");
                assert_eq!(view.plans[0].revision, (index + 1) as i64);
                assert!(view.execution.is_none() && view.generation.is_none());
                let inspected = workspace.director_state(&source).await.unwrap();
                assert_eq!(inspected["plan"]["revision"], json!(index + 1));
                assert_eq!(inspected["conversation_id"], json!(source));
                assert!(inspected["execution_state"].is_null());
                assert!(
                    inspected.get("work").is_none(),
                    "Inspection must not repeat unrelated workspace work"
                );
                let snapshot = workspace.snapshot().await.unwrap();
                let mut noisy = workspace.snapshot().await.unwrap();
                let mut unrelated = workspace.task(&turn).await.unwrap();
                unrelated.id = "unrelated-work".into();
                unrelated.input = "UNRELATED_COMPLETED_WORK".into();
                unrelated.purpose = WorkPurpose::Work;
                unrelated.state = "completed".into();
                noisy.tasks.push(unrelated);
                let focused = workspace.director_observation(&noisy, &turn).await.unwrap();
                assert_eq!(focused["focus"]["kind"], "this_proposal");
                assert_eq!(focused["plan_started"], false);
                assert_eq!(focused["focus"]["completed_worker_assignments"], 0);
                assert_eq!(focused["saved_plan"]["revision"], json!(index + 1));
                assert_eq!(focused["saved_plan"]["title"], title);
                assert_eq!(focused["work"], json!([]));
                assert!(!focused.to_string().contains("UNRELATED_COMPLETED_WORK"));
                assert_eq!(
                    snapshot.tasks.len(),
                    index + 1,
                    "The Guide conversation must stay visible"
                );
                assert!(
                    snapshot.planning_tasks.is_empty(),
                    "No second planning run or worker is launched"
                );
                assert!(snapshot
                    .tasks
                    .iter()
                    .all(|t| t.purpose == WorkPurpose::Explore));
                assert_eq!(
                    snapshot
                        .usage
                        .iter()
                        .find(|u| u.work_id == turn)
                        .unwrap()
                        .budget
                        .as_ref()
                        .unwrap()
                        .token_limit,
                    if index == 1 {
                        1024
                    } else {
                        LOCAL_TOKEN_CEILING as i64
                    }
                );
                assert_eq!(workspace.work_briefs(&source).await.unwrap()[0].body, title);
                let requests = calls.lock().unwrap().clone();
                assert_eq!(requests.len(), 2);
                assert!(requests[0]["tools"].to_string().contains("work_plan"));
                let tool = requests[0]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|t| t["function"]["name"] == "work_plan")
                    .unwrap();
                assert_eq!(
                    tool["function"]["parameters"]["required"],
                    json!(["operation", "direction", "plan", "work_id"])
                );
                assert_eq!(
                    tool["function"]["parameters"]["properties"]["plan"]["anyOf"][0]["properties"]
                        ["assignments"]["items"]["properties"]["agent_key"]["enum"],
                    json!([AGENT])
                );
                assert!(requests[1]["messages"]
                    .to_string()
                    .contains("work_launched"));
                assert!(requests[1]["messages"].to_string().contains(title));
                // A replay of the accepted send never executes its planning tool again.
                workspace
                    .submit_with_purpose(
                        turn.clone(),
                        format!("Plan {title}. PRIVATE_NOTE_NOT_FOR_TEAM"),
                        shaping::GUIDE.into(),
                        if index == 1 {
                            Some(source.clone())
                        } else {
                            None
                        },
                        WorkPurpose::Explore,
                    )
                    .await
                    .unwrap();
                assert_eq!(calls.lock().unwrap().len(), 2);
                let (_, session) = workspace.bind_director(&turn).await.unwrap();
                assert!(
                    workspace
                        .director_command(&session.binding, Command::Inspect {})
                        .await
                        .is_err(),
                    "A completed reply cannot issue a late control operation"
                );
                server.abort();
            }
        })
        .await;
}

#[tokio::test]
async fn guide_answers_a_small_question_naturally_without_creating_a_plan() {
    let dir = tempfile::tempdir().unwrap();
    let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        false,
        "structured",
        json!({"summary":"A shorter workshop can be easier to schedule."}),
        false,
    )
    .await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace =
                LocalWorkspace::open(dir.path().join("answer.db"), "qwen3.5:latest".into(), url)
                    .await
                    .unwrap();
            let id = uuid::Uuid::new_v4().to_string();
            workspace
                .submit_with_purpose(
                    id.clone(),
                    "What is one benefit of a shorter workshop?".into(),
                    shaping::GUIDE.into(),
                    None,
                    WorkPurpose::Explore,
                )
                .await
                .unwrap();
            done(&workspace, &id).await;
            assert_eq!(calls.lock().unwrap().len(), 1);
            let view = workspace.plan_view(&id).await.unwrap();
            assert!(view.plans.is_empty() && view.generation.is_none() && view.execution.is_none());
            assert!(workspace.work_briefs(&id).await.unwrap().is_empty());
        })
        .await;
    server.abort();
}

#[tokio::test]
async fn proposal_preserves_owner_edits_and_rejects_invalid_agents_and_repeated_changes() {
    let dir = tempfile::tempdir().unwrap();
    let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        "finish",
        json!({"summary":"Okay"}),
        false,
    )
    .await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace =
                LocalWorkspace::open(dir.path().join("conflict.db"), "qwen3.5:latest".into(), url)
                    .await
                    .unwrap();
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
                        title: "Discuss".into(),
                        request_id: format!("{source}@{}", shaping::GUIDE),
                        goal_id: None,
                    },
                    Some("Discuss".into()),
                    WorkPurpose::Explore,
                )
                .await
                .unwrap();
            let (_, session) = workspace.bind_director(&source).await.unwrap();
            let binding = session.binding;
            let mut worker = workspace
                .services
                .agents()
                .await
                .unwrap()
                .into_iter()
                .find(|a| a.key == AGENT)
                .unwrap();
            worker.name = "Reviewer".into();
            let mut named = plan("Human names");
            named.assignments[0].agent_key = "Reviewer".into();
            resolve_agents(&mut named, &[worker.clone()]).unwrap();
            assert_eq!(named.assignments[0].agent_key, AGENT);
            let mut duplicate = worker.clone();
            duplicate.key = "another-reviewer".into();
            named.assignments[0].agent_key = "Reviewer".into();
            assert!(resolve_agents(&mut named, &[worker.clone(), duplicate.clone()]).is_err());
            named.assignments[0].agent_key = AGENT.into();
            resolve_agents(&mut named, &[worker, duplicate]).unwrap();
            let mut invalid = plan("Invalid");
            invalid.assignments[0].agent_key = "imaginary-agent".into();
            assert!(workspace
                .director_propose(&binding, "Shared".into(), invalid)
                .await
                .is_err());
            assert!(workspace.work_briefs(&source).await.unwrap().is_empty());
            workspace
                .save_work_brief(
                    &source,
                    SaveWorkBrief {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        expected_revision: 0,
                        body: "Owner's edit".into(),
                    },
                )
                .await
                .unwrap();
            assert!(workspace
                .director_propose(&binding, "Overwrite".into(), plan("Stale"))
                .await
                .is_err());
            assert_eq!(
                workspace.work_briefs(&source).await.unwrap()[0].body,
                "Owner's edit"
            );
            let (_, session) = workspace.bind_director(&source).await.unwrap();
            workspace
                .director_propose(&session.binding, "Reconciled".into(), plan("Reconciled"))
                .await
                .unwrap();
            workspace
                .director_propose(&session.binding, "Reconciled".into(), plan("Reconciled"))
                .await
                .unwrap();
            assert!(workspace
                .director_propose(&session.binding, "Another edit".into(), plan("Changed"))
                .await
                .is_err());
            assert_eq!(workspace.plan_view(&source).await.unwrap().plans.len(), 1);
            assert_eq!(workspace.work_briefs(&source).await.unwrap().len(), 2);
            assert!(calls.lock().unwrap().is_empty());
        })
        .await;
    server.abort();
}

#[test]
fn model_commands_cannot_smuggle_launch_authority_or_select_another_conversation() {
    use crate::resources::work_director::command;
    assert!(command(json!({"operation":"inspect"})).is_ok());
    assert!(command(json!({"operation":"inspect","direction":null,"plan":null})).is_ok());
    assert!(
        command(json!({"operation":"resources","direction":null,"plan":null,"work_id":null}))
            .is_ok()
    );
    assert!(command(json!({"operation":"work","direction":null,"plan":null,"work_id":uuid::Uuid::new_v4().to_string()})).is_ok());
    let missing_direction = command(json!({"operation":"propose","plan":plan("Plan")}))
        .err()
        .unwrap();
    assert!(missing_direction.contains("missing field `direction`"));
    for value in [
        json!({"operation":"start"}),
        json!({"operation":"agree"}),
        json!({"operation":"resources","organization":"other-org"}),
        json!({"operation":"resources","grant":"admin"}),
        json!({"operation":"work","work_id":"../private"}),
        json!({"operation":"inspect","work_id":uuid::Uuid::new_v4().to_string()}),
        json!({"operation":"inspect","source":"someone-else"}),
        json!({"operation":"inspect","direction":"A hidden write","plan":null}),
        json!({"operation":"propose","direction":"Shared","plan":plan("Plan"),"grant":"admin"}),
    ] {
        assert!(
            command(value.clone()).is_err(),
            "Accepted unexpected fields: {value}"
        );
    }
}
