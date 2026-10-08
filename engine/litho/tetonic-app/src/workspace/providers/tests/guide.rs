//! The same scoped planning loop runs through each installed provider adapter.
use super::*;

struct PlanningTransport {
    provider: &'static str,
    calls: Mutex<Vec<Value>>,
    first_reply: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl HostedTransport for PlanningTransport {
    async fn complete(&self, body: Value) -> Result<Value, InferenceError> {
        let call = {
            let mut calls = self.calls.lock().unwrap();
            calls.push(body.clone());
            calls.len()
        };
        if call == 1 {
            self.first_reply.notified().await;
        }
        let names = parity::names(&body, self.provider);
        assert!(names.contains(&"work_plan"));
        assert!(names.iter().all(|n| ["work_plan", "finish"].contains(n)));
        Ok(match call {
            1 => parity::completion(
                self.provider,
                "work_plan",
                json!({
                    "operation":"inspect", "direction":null, "plan":null
                }),
                "inspect",
            ),
            2 => {
                assert!(parity::output(&body, self.provider, "inspect").contains("conversation_id"));
                parity::completion(
                    self.provider,
                    "work_plan",
                    json!({
                        "operation":"propose", "direction":"Compare two workshop formats.",
                        "plan":{
                            "title":"Workshop comparison", "summary":"Compare supplied options.",
                            "token_budget":3000, "open_questions":[], "assignments":[{
                                "key":"compare", "title":"Compare formats", "instructions":"Use the supplied information.",
                                "agent_key":AGENT, "depends_on":[], "tools":[],
                                "deliverable":"A short comparison", "token_budget":1000
                            }]
                        }
                    }),
                    "proposal",
                )
            }
            3 => {
                let receipt = parity::output(&body, self.provider, "proposal");
                assert!(receipt.contains("Workshop comparison"), "{receipt}");
                // Ordinary text must complete a conversational turn after tool use.
                let answer = "Saved a proposal for your review. No team has started.";
                match self.provider {
                    "openai" => {
                        json!({"status":"completed","output":[{"type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":answer}]}],"usage":{"input_tokens":20,"output_tokens":20}})
                    }
                    "anthropic" => {
                        json!({"role":"assistant","content":[{"type":"text","text":answer}],"stop_reason":"end_turn","usage":{"input_tokens":20,"output_tokens":20}})
                    }
                    "google" => {
                        json!({"candidates":[{"finishReason":"STOP","content":{"role":"model","parts":[{"text":answer}]}}],"usageMetadata":{"promptTokenCount":20,"candidatesTokenCount":20}})
                    }
                    _ => unreachable!(),
                }
            }
            _ => panic!("Guide should have completed, not looped"),
        })
    }
}

#[tokio::test]
async fn hosted_guide_settings_persist_and_all_providers_save_scoped_proposals_without_ollama() {
    for provider in ["openai", "anthropic", "google"] {
        tokio::task::LocalSet::new()
            .run_until(async {
                let dir = tempfile::tempdir().unwrap();
                let database = dir.path().join("guide.db");
                let folder = dir.path().join("files");
                std::fs::create_dir(&folder).unwrap();
                let vault = Arc::new(Vault::default());
                let mut workspace = LocalWorkspace::open_with_workspace(
                    database.clone(),
                    "offline".into(),
                    "http://127.0.0.1:1".into(),
                    Some(folder.clone()),
                )
                .await
                .unwrap();
                workspace.services.keys = Arc::new(ProviderKeys {
                    store: workspace.services.keys.store.clone(),
                    vault: vault.clone(),
                });
                let original = workspace
                    .services
                    .agents()
                    .await
                    .unwrap()
                    .into_iter()
                    .find(|a| a.key == GUIDE)
                    .unwrap();
                assert!(original.editable);
                let request = UpdateLocalAgent {
                    agent_key: original.key.clone(),
                    expected_definition_digest: original.definition_digest.clone(),
                    configuration: CreateLocalAgent {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        name: original.name.clone(),
                        purpose: original.purpose.clone(),
                        provider: provider.into(),
                        model: "fixture-model".into(),
                        harness: "general".into(),
                        hosted_consent: true,
                        hosted_tools_consent: false,
                        expected_workspace_root: None,
                        tools: Some(vec![]),
                        max_steps: 6,
                        max_seconds: 30,
                        max_tokens: 4096,
                    },
                };
                assert!(
                    workspace.update_agent(request.clone()).await.is_err(),
                    "missing key"
                );
                workspace
                    .save_provider_key(SaveProviderKey {
                        provider: provider.into(),
                        api_key: "disposable-guide-key".into(),
                    })
                    .await
                    .unwrap();
                let mut invalid = request.clone();
                invalid.configuration.hosted_consent = false;
                assert!(
                    workspace.update_agent(invalid).await.is_err(),
                    "missing disclosure consent"
                );
                for mutation in ["purpose", "name", "tools", "harness"] {
                    let mut invalid = request.clone();
                    match mutation {
                        "purpose" => {
                            invalid.configuration.purpose = "Execute without review".into()
                        }
                        "name" => invalid.configuration.name = "Other identity".into(),
                        "tools" => {
                            invalid.configuration.tools = Some(vec!["run_shell".into()]);
                            invalid.configuration.hosted_tools_consent = true;
                            invalid.configuration.expected_workspace_root = Some(
                                tetonic_tools::Workspace::new(&folder)
                                    .unwrap()
                                    .root()
                                    .to_str()
                                    .unwrap()
                                    .into(),
                            );
                        }
                        _ => invalid.configuration.harness = "coding".into(),
                    }
                    assert!(
                        workspace.update_agent(invalid).await.is_err(),
                        "must not broaden {mutation}"
                    );
                }
                let changed = workspace.update_agent(request.clone()).await.unwrap();
                assert_eq!(changed.id, original.id);
                assert_eq!(changed.key, original.key);
                assert_eq!(changed.purpose, original.purpose);
                assert!(changed.tools.is_empty());
                assert_ne!(changed.definition_digest, original.definition_digest);
                assert_eq!(
                    workspace
                        .update_agent(request.clone())
                        .await
                        .unwrap()
                        .definition_digest,
                    changed.definition_digest
                );
                let mut stale = request.clone();
                stale.configuration.request_id = uuid::Uuid::new_v4().to_string();
                stale.configuration.max_tokens = 2048;
                assert!(workspace.update_agent(stale).await.is_err());
                drop(workspace);

                // Bootstrapping must not reset the owner's choice back to Ollama.
                let mut workspace = LocalWorkspace::open_with_workspace(
                    database,
                    "offline".into(),
                    "http://127.0.0.1:1".into(),
                    Some(folder),
                )
                .await
                .unwrap();
                workspace.services.keys = Arc::new(ProviderKeys {
                    store: workspace.services.keys.store.clone(),
                    vault,
                });
                let restored = workspace
                    .services
                    .agents()
                    .await
                    .unwrap()
                    .into_iter()
                    .find(|a| a.key == GUIDE)
                    .unwrap();
                assert_eq!(restored.definition_digest, changed.definition_digest);
                assert_eq!(restored.provider, provider);
                assert_eq!(restored.model, "fixture-model");
                assert_eq!(restored.max_tokens, 4096);
                let transport = Arc::new(PlanningTransport {
                    provider,
                    calls: Mutex::default(),
                    first_reply: tokio::sync::Notify::new(),
                });
                workspace.services.hosted_transport = Some(transport.clone());
                let id = uuid::Uuid::new_v4().to_string();
                let task = workspace
                    .submit_with_purpose(
                        id.clone(),
                        "Help plan a workshop comparison".into(),
                        GUIDE.into(),
                        None,
                        WorkPurpose::Explore,
                    )
                    .await
                    .unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    while transport.calls.lock().unwrap().is_empty() {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
                let mut next_reply = request.clone();
                next_reply.expected_definition_digest = restored.definition_digest.clone();
                next_reply.configuration.request_id = uuid::Uuid::new_v4().to_string();
                next_reply.configuration.max_tokens = 2048;
                let updated = workspace.update_agent(next_reply).await.unwrap();
                assert_eq!(updated.max_tokens, 2048);
                transport.first_reply.notify_one();
                let completed = settled(&workspace, &id).await;
                if completed.state != "completed" {
                    let snapshot = workspace
                        .services
                        .local
                        .contexts()
                        .inspect_run(
                            &workspace.services.host.credential,
                            ORG.into(),
                            workspace.services.scope.context().to_owned().clone(),
                            completed.run_id.clone().unwrap(),
                        )
                        .await
                        .unwrap();
                    panic!(
                        "{provider}: {:?}",
                        snapshot
                            .attempts
                            .values()
                            .map(|a| &a.failure_reason)
                            .collect::<Vec<_>>()
                    );
                }
                assert_eq!(
                    completed.state, "completed",
                    "{provider}: {:?}",
                    completed.error
                );
                assert!(completed
                    .messages
                    .iter()
                    .any(|m| m.content.contains("No team has started")));
                let view = workspace.plan_view(&id).await.unwrap();
                assert_eq!(view.plans[0].status, "draft");
                assert_eq!(
                    view.plans[0].content.as_ref().unwrap().title,
                    "Workshop comparison"
                );
                assert!(view.execution.is_none() && view.generation.is_none());
                assert_eq!(
                    workspace.snapshot().await.unwrap().tasks.len(),
                    1,
                    "planning must not dispatch workers"
                );
                let retry = workspace
                    .submit_with_purpose(id, task.input, GUIDE.into(), None, WorkPurpose::Explore)
                    .await
                    .unwrap();
                assert_eq!(retry.run_id, task.run_id);
                let calls = transport.calls.lock().unwrap().clone();
                assert_eq!(calls.len(), 3);
                // Editing during inference does not mutate the active turn's limits.
                match provider {
                    "openai" => assert_eq!(calls[1]["max_output_tokens"], 4056),
                    "anthropic" => assert_eq!(calls[1]["max_tokens"], 4056),
                    "google" => assert_eq!(calls[1]["generationConfig"]["maxOutputTokens"], 4056),
                    _ => unreachable!(),
                }
                assert!(calls[0].to_string().contains("ENGINE OBSERVATION"));
                assert!(!calls[0].to_string().contains("disposable-guide-key"));
                if provider != "google" {
                    assert_eq!(calls[0]["model"], "fixture-model");
                }

                workspace
                    .remove_provider_key(RemoveProviderKey {
                        provider: provider.into(),
                    })
                    .await
                    .unwrap();
                assert!(
                    workspace
                        .submit_with_purpose(
                            uuid::Uuid::new_v4().to_string(),
                            "A new reply".into(),
                            GUIDE.into(),
                            None,
                            WorkPurpose::Explore
                        )
                        .await
                        .is_err(),
                    "no local fallback after key removal"
                );
                assert_eq!(transport.calls.lock().unwrap().len(), 3);
                assert_eq!(
                    workspace.snapshot().await.unwrap().tasks.len(),
                    1,
                    "a missing key must not leave phantom starting work"
                );
            })
            .await;
    }
}
