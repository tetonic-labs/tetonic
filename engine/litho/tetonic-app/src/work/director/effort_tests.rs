use super::*;

#[tokio::test]
async fn a_conversation_can_propose_another_effort_without_replacing_started_work() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("efforts.db");
    let (url, _, server) = crate::work::plan_execution::tests::scripted_server(0).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = std::rc::Rc::new(
                LocalWorkspace::open(database.clone(), "qwen3.5:latest".into(), url.clone())
                    .await
                    .unwrap(),
            );
            let source =
                crate::work::plan_execution::tests::seed_options(&workspace, false, true).await;
            let started = workspace
                .start_plan(
                    &source,
                    StartPlan {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        revision: 1,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            let old_plan = workspace.plan_view(&source).await.unwrap().plans.remove(0);
            let mut parent = source.clone();
            let mut second_source = String::new();
            for title in ["Build the requested game files", "Add a short README"] {
                let turn = uuid::Uuid::new_v4().to_string();
                workspace
                    .services
                    .local
                    .resources()
                    .create_work_with_roster(
                        &workspace.services.host.credential,
                        crate::resources::CreateTeamWorkItem {
                            org: ORG.into(),
                            team: TEAM.into(),
                            work_id: turn.clone(),
                            title: title.into(),
                            request_id: format!("{turn}@{}#{parent}", shaping::GUIDE),
                            goal_id: None,
                        },
                        Some(title.into()),
                        WorkPurpose::Explore,
                        Some((None, Some(parent.clone()))),
                    )
                    .await
                    .unwrap();
                let (_, session) = workspace.bind_director(&turn).await.unwrap();
                assert_eq!(session.binding.conversation, source);
                if second_source.is_empty() {
                    second_source = turn.clone();
                    assert_eq!(
                        session.binding.previous_source.as_deref(),
                        Some(source.as_str())
                    );
                } else {
                    assert!(session.binding.previous_source.is_none());
                }
                assert_eq!(session.binding.source, second_source);
                let mut content = plan(title);
                content.assignments[0].instructions =
                    "Prepare the new deliverable using only supplied information.".into();
                workspace
                    .director_propose(&session.binding, title.into(), content.clone())
                    .await
                    .unwrap();
                // Lost acknowledgements cannot produce another plan or execution.
                workspace
                    .director_propose(&session.binding, title.into(), content)
                    .await
                    .unwrap();
                let new_view = workspace.plan_view(&second_source).await.unwrap();
                assert!(new_view.execution.is_none());
                assert_eq!(new_view.plans[0].content.as_ref().unwrap().title, title);
                assert_eq!(new_view.plans[0].work_id, second_source);
                assert_eq!(
                    workspace.work_team(&second_source).await.unwrap(),
                    workspace.work_team(&source).await.unwrap()
                );
                let unchanged = workspace.plan_view(&source).await.unwrap();
                assert_eq!(unchanged.plans[0].content, old_plan.content);
                assert_eq!(unchanged.plans[0].revision, old_plan.revision);
                assert_eq!(unchanged.execution.unwrap().receipt, started.receipt);
                let snapshot = workspace.snapshot().await.unwrap();
                assert_eq!(proposal_source(&snapshot, &turn), second_source);
                assert_eq!(conversation_root(&snapshot, &turn), source);
                assert!(snapshot.guide_plan_sources.contains(&second_source));
                let observation = workspace
                    .director_observation(&snapshot, &turn)
                    .await
                    .unwrap();
                assert_eq!(observation["saved_plan"]["title"], title);
                assert_eq!(observation["plan_started"], false);
                parent = turn;
            }
            assert_eq!(
                workspace
                    .plan_view(&second_source)
                    .await
                    .unwrap()
                    .plans
                    .len(),
                2
            );
            // Persisted proposal ownership, not a mutable browser pointer, restores focus.
            let reopened = LocalWorkspace::open(database, "qwen3.5:latest".into(), url)
                .await
                .unwrap();
            let snapshot = reopened.snapshot().await.unwrap();
            assert_eq!(proposal_source(&snapshot, &parent), second_source);
            assert_eq!(
                reopened.plan_view(&source).await.unwrap().plans[0].content,
                old_plan.content
            );
        })
        .await;
    server.abort();
}
