use super::*;

#[tokio::test]
async fn foreground_message_preempts_an_outstanding_background_model_call() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("priority.db");
    tokio::task::LocalSet::new()
        .run_until(async {
            let (url, _, server) = crate::tui_mvp_tests::inference_server_with_behavior(
                false,
                "structured",
                json!({"summary":"Worker result"}),
                false,
            )
            .await;
            let workspace = LocalWorkspace::open(database.clone(), "qwen3.5:latest".into(), url)
                .await
                .unwrap();
            let work = uuid::Uuid::new_v4().to_string();
            workspace
                .submit(work.clone(), "A finished independent assignment".into())
                .await
                .unwrap();
            done(&workspace, &work).await;
            workspace
                .save_guide_attention(crate::work::SaveGuideAttention {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    settings: tetonic_memory::GuideAttentionSettings {
                        revision: 0,
                        enabled: true,
                        daily_tokens: 16000,
                        assessment_tokens: 8000,
                    },
                })
                .await
                .unwrap();
            drop(workspace);
            server.abort();
            let (url, _, server) = crate::tui_mvp_tests::inference_server_with_behavior(
                false,
                "structured",
                json!({"summary":"unused"}),
                true,
            )
            .await;
            let workspace = LocalWorkspace::open(database, "qwen3.5:latest".into(), url)
                .await
                .unwrap();
            workspace.reconcile_guide_attention().await.unwrap();
            let background = workspace.guide_attention().await.unwrap().items[0]
                .assessment_id
                .clone()
                .unwrap();
            let foreground = uuid::Uuid::new_v4().to_string();
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                workspace.submit_with_purpose(
                    foreground.clone(),
                    "An unrelated question from the human".into(),
                    shaping::GUIDE.into(),
                    None,
                    WorkPurpose::Explore,
                ),
            )
            .await
            .expect("Foreground must not wait for background inference")
            .unwrap();
            assert!(matches!(
                workspace.task(&background).await.unwrap().state.as_str(),
                "canceled" | "canceling"
            ));
            assert!(workspace.outstanding_attention().await.unwrap().is_empty());
            assert_eq!(workspace.task(&work).await.unwrap().state, "completed");
            workspace.cancel(&foreground).await.unwrap();
            server.abort();
        })
        .await;
}

#[tokio::test]
async fn long_discussion_retrieves_originals_and_links_subjects_without_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("partner.db");
    tokio::task::LocalSet::new()
        .run_until(async {
            let (url, _, server) = crate::tui_mvp_tests::inference_server_with_behavior(
                false,
                "structured",
                json!({"summary":"Useful discussion only."}),
                false,
            )
            .await;
            // Exercise the full retained-history ceiling with an explicitly
            // funded Guide. Tiny fake usage reports do not make its input free.
            let mut configuration = crate::host::HostConfiguration::default();
            configuration.workspace_execution.max_tokens = 32_768;
            let workspace = LocalWorkspace::open_with_configuration(
                database.clone(),
                "qwen3.5:latest".into(),
                url.clone(),
                None,
                configuration,
            )
            .await
            .unwrap();
            let guide = workspace.guide_for_coordination().await.unwrap();
            workspace
                .update_agent(UpdateLocalAgent {
                    agent_key: guide.key,
                    expected_definition_digest: guide.definition_digest,
                    configuration: CreateLocalAgent {
                        tool_pack_selection: None,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        name: guide.name,
                        purpose: guide.purpose,
                        provider: guide.provider,
                        model: guide.model,
                        harness: "general".into(),
                        max_steps: guide.max_steps,
                        max_seconds: guide.max_seconds,
                        max_tokens: 32_768,
                        tools: Some(vec![]),
                        workspace_root: None,
                        expected_workspace_root: None,
                        hosted_consent: false,
                        hosted_tools_consent: false,
                    },
                })
                .await
                .unwrap();
            let mut ids = vec![];
            for i in 0..70 {
                let id = uuid::Uuid::new_v4().to_string();
                let input = format!(
                    "Turn {i}: {}. {}",
                    "資料と証拠".repeat(32),
                    if i == 0 {
                        "Exact reservation: AZ-4937. Tentative museum visit."
                    } else {
                        "Continue comparing options; do not delegate."
                    }
                );
                workspace
                    .submit_with_purpose(
                        id.clone(),
                        input,
                        shaping::GUIDE.into(),
                        ids.last().cloned(),
                        WorkPurpose::Explore,
                    )
                    .await
                    .unwrap();
                done(&workspace, &id).await;
                ids.push(id);
            }
            let prompt = workspace
                .conversation_input(
                    &uuid::Uuid::new_v4().to_string(),
                    shaping::GUIDE,
                    ids.last().map(String::as_str),
                    "Correction: we rejected the museum. What was our reservation?",
                )
                .await
                .unwrap();
            assert!(prompt.len() <= conversations::GUIDE_CONTEXT_LIMIT);
            assert!(prompt.contains("AZ-4937") && prompt.contains("rejected the museum"));
            let (_, session) = workspace.bind_director(ids.last().unwrap()).await.unwrap();
            let original = workspace
                .director_history(&session.binding, Some(&ids[0]), None, 0)
                .await
                .unwrap();
            assert!(original["content"].as_str().unwrap().contains("AZ-4937"));
            let other = uuid::Uuid::new_v4().to_string();
            workspace
                .submit_with_purpose(
                    other.clone(),
                    "Unrelated garden idea".into(),
                    shaping::GUIDE.into(),
                    None,
                    WorkPurpose::Explore,
                )
                .await
                .unwrap();
            done(&workspace, &other).await;
            let (_, other_session) = workspace.bind_director(&other).await.unwrap();
            assert!(workspace
                .director_history(&other_session.binding, Some(&ids[0]), None, 0)
                .await
                .is_err());
            let subject = tetonic_memory::GuideSubjectDraft {
                id: uuid::Uuid::new_v4().to_string(),
                expected_revision: 0,
                title: "Trip".into(),
                understanding: "Museum rejected; keep the reservation.".into(),
                source_turns: vec![ids[0].clone(), ids[69].clone()],
                work_ids: vec![],
            };
            workspace
                .update_guide_context(
                    &ids[0],
                    crate::work::GuideContextMutation {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        command: crate::work::GuideContextCommand::Save {
                            subject: subject.clone(),
                        },
                    },
                )
                .await
                .unwrap();
            workspace
                .update_guide_context(
                    &other,
                    crate::work::GuideContextMutation {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        command: crate::work::GuideContextCommand::Focus {
                            subject_id: Some(subject.id.clone()),
                            expected_revision: 0,
                        },
                    },
                )
                .await
                .unwrap();
            assert!(workspace
                .director_history(&other_session.binding, Some(&ids[0]), None, 0)
                .await
                .unwrap()["content"]
                .as_str()
                .unwrap()
                .contains("AZ-4937"));
            assert!(workspace
                .snapshot()
                .await
                .unwrap()
                .tasks
                .iter()
                .all(|t| t.purpose == WorkPurpose::Explore));
            drop(workspace);
            let reopened = LocalWorkspace::open(database, "qwen3.5:latest".into(), url)
                .await
                .unwrap();
            let context = reopened.guide_context_view(&other).await.unwrap();
            assert_eq!(context["subject"]["id"], subject.id);
            assert_eq!(context["subject"]["understanding"], subject.understanding);
            server.abort();
        })
        .await;
}

#[tokio::test]
async fn background_assessment_uses_managed_budget_and_cannot_mutate_or_multiply_work() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("attention.db");
    tokio::task::LocalSet::new()
        .run_until(async {
            let (url, _, server) = crate::tui_mvp_tests::inference_server_with_behavior(
                false,
                "structured",
                json!({"summary":"Result available; inspect the evidence."}),
                false,
            )
            .await;
            let workspace = LocalWorkspace::open(database, "qwen3.5:latest".into(), url)
                .await
                .unwrap();
            let work = uuid::Uuid::new_v4().to_string();
            workspace
                .submit(work.clone(), "Compare these two choices".into())
                .await
                .unwrap();
            done(&workspace, &work).await;
            workspace.reconcile_guide_attention().await.unwrap();
            assert_eq!(workspace.guide_attention().await.unwrap().items.len(), 1);
            assert_eq!(workspace.snapshot().await.unwrap().tasks.len(), 1);
            workspace
                .save_guide_attention(crate::work::SaveGuideAttention {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    settings: tetonic_memory::GuideAttentionSettings {
                        revision: 0,
                        enabled: true,
                        daily_tokens: 16_000,
                        assessment_tokens: 8_000,
                    },
                })
                .await
                .unwrap();
            workspace.reconcile_guide_attention().await.unwrap();
            let id = workspace.guide_attention().await.unwrap().items[0]
                .assessment_id
                .clone()
                .unwrap();
            done(&workspace, &id).await;
            let task = workspace.task(&id).await.unwrap();
            assert!(task.guide_background);
            let (dispatch, session) = workspace.bind_director(&id).await.unwrap();
            let schema = dispatch.director.unwrap().advertisement();
            assert!(!schema.parameters["properties"]["operation"]["enum"]
                .to_string()
                .contains("propose"));
            assert!(workspace
                .director_command(
                    &session.binding,
                    "late",
                    Command::Remember {
                        direction: "No authority".into()
                    }
                )
                .await
                .is_err());
            workspace.reconcile_guide_attention().await.unwrap();
            let view = workspace.guide_attention().await.unwrap();
            assert_eq!(view.items[0].state, "reviewed");
            assert!(view.items[0].update.is_some());
            assert_eq!(view.reserved_today, 8_000);
            let mut result = workspace.task(&work).await.unwrap();
            let signature = crate::work::director::attention::work_signature(&result);
            // Other assignments advance the shared run sequence without
            // changing this assignment's evidence or consuming another check.
            result.sequence += 20;
            assert_eq!(
                crate::work::director::attention::work_signature(&result),
                signature
            );
            result
                .messages
                .last_mut()
                .unwrap()
                .content
                .push_str(" A corrected result.");
            assert_ne!(
                crate::work::director::attention::work_signature(&result),
                signature
            );
            for _ in 0..3 {
                workspace.reconcile_guide_attention().await.unwrap();
            }
            assert_eq!(workspace.snapshot().await.unwrap().tasks.len(), 2);
            let usage = workspace
                .snapshot()
                .await
                .unwrap()
                .usage
                .into_iter()
                .find(|u| u.work_id == id)
                .unwrap();
            assert_eq!(usage.budget.unwrap().token_limit, 8_000);
            assert!(usage.input_tokens + usage.output_tokens > 0);
            workspace
                .see_guide_attention(view.items[0].signal.key.clone())
                .await
                .unwrap();
            assert_eq!(
                workspace.guide_attention().await.unwrap().items[0].state,
                "reviewed"
            );
            server.abort();
        })
        .await;
}
