use super::*;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tetonic_domain::key_storage::{KeyStorageError, SecretBytes};
use tetonic_inference::hosted::HostedTransport;

#[derive(Default)]
struct Vault {
    values: Mutex<std::collections::HashMap<String, Vec<u8>>>,
}
impl KeyStorage for Vault {
    fn create(&self, value: &[u8]) -> Result<SecretKeyRef, KeyStorageError> {
        let id = uuid::Uuid::new_v4().to_string();
        self.values
            .lock()
            .unwrap()
            .insert(id.clone(), value.to_vec());
        Ok(SecretKeyRef(id))
    }
    fn read(&self, key: &SecretKeyRef) -> Result<SecretBytes, KeyStorageError> {
        self.values
            .lock()
            .unwrap()
            .get(&key.0)
            .cloned()
            .map(SecretBytes::new)
            .ok_or(KeyStorageError::Missing)
    }
    fn delete(&self, key: &SecretKeyRef) -> Result<(), KeyStorageError> {
        self.values.lock().unwrap().remove(&key.0);
        Ok(())
    }
}

#[derive(Default)]
struct Transport {
    calls: Mutex<Vec<Value>>,
    wait: AtomicBool,
}
#[async_trait::async_trait]
impl HostedTransport for Transport {
    async fn list_models(
        &self,
        endpoint: &str,
        cursor: Option<&str>,
    ) -> Result<Value, InferenceError> {
        assert!(cursor.is_none());
        let model = if endpoint == "https://api.openai.com/v1/models" {
            "gpt-4.1"
        } else {
            assert_eq!(endpoint, "https://api.anthropic.com/v1/models");
            "claude-sonnet-4-6"
        };
        Ok(json!({"data":[{"id":model}],"has_more":false}))
    }
    async fn complete(&self, body: Value) -> Result<Value, InferenceError> {
        self.calls.lock().unwrap().push(body.clone());
        if self.wait.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        if body["model"].as_str().unwrap().starts_with("claude") {
            Ok(
                json!({"role":"assistant","type":"message","content":[{"type":"tool_use","id":"finish-1","name":"finish","input":{"summary":"Hosted answer"}}],"usage":{"input_tokens":10,"output_tokens":20},"stop_reason":"tool_use"}),
            )
        } else {
            Ok(
                json!({"status":"completed","output":[{"type":"function_call","call_id":"finish-1","name":"finish","arguments":"{\"summary\":\"Hosted answer\"}"}],"usage":{"input_tokens":10,"output_tokens":20}}),
            )
        }
    }
}

async fn settled(workspace: &LocalWorkspace, id: &str) -> LocalTask {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let task = workspace.task(id).await.unwrap();
            if matches!(task.state.as_str(), "completed" | "failed" | "canceled") {
                break task;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap()
}

#[cfg(test)]
#[tokio::test]
async fn hosted_agents_use_managed_runs_without_ollama_and_keep_keys_out_of_history() {
    hosted_agent_round_trip(false).await;
}

#[tokio::test]
async fn hosted_prompt_only_agents_do_not_inherit_host_workspace_tools() {
    hosted_agent_round_trip(true).await;
}

async fn hosted_agent_round_trip(with_folder: bool) {
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let database = dir.path().join("hosted.db");
            let vault = Arc::new(Vault::default());
            let transport = Arc::new(Transport::default());
            let folder = tempfile::tempdir().unwrap();
            let workspace_root = with_folder.then(|| folder.path().to_path_buf());
            let mut workspace = LocalWorkspace::open_with_workspace(
                database.clone(),
                "offline".into(),
                "http://127.0.0.1:1".into(),
                workspace_root.clone(),
            )
            .await
            .unwrap();
            workspace.keys = Arc::new(ProviderKeys {
                store: workspace.keys.store.clone(),
                vault: vault.clone(),
            });
            workspace.hosted_transport = Some(transport.clone());
            let catalog = workspace.agent_catalog().await.unwrap();
            assert!(catalog.models.is_empty());
            assert!(catalog.local_error.is_some());
            assert_eq!(catalog.providers.len(), 2);
            assert_eq!(!catalog.tools.is_empty(), with_folder);
            for (provider, model) in [("openai", "gpt-4.1"), ("anthropic", "claude-sonnet-4-6")] {
                let input = CreateLocalAgent {
                    provider: provider.into(),
                    hosted_consent: true,
                    hosted_tools_consent: false,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    name: "Lab analyst".into(),
                    purpose: "Use the copper compass. Return your answer with finish.".into(),
                    model: model.into(),
                    harness: "general".into(),
                    max_steps: 2,
                    max_seconds: 30,
                    max_tokens: 1024,
                    tools: None,
                };
                assert!(workspace.create_agent(input.clone()).await.is_err());
                assert!(workspace.provider_models(provider).await.is_err());
                let secret = format!("disposable-{provider}-credential");
                workspace
                    .save_provider_key(SaveProviderKey {
                        provider: provider.into(),
                        api_key: secret.clone(),
                    })
                    .await
                    .unwrap();
                // Rotation publishes the new reference before deleting the old entry.
                let discovered = workspace.provider_models(provider).await.unwrap();
                assert_eq!(discovered.models, vec![model]);
                assert!(!discovered.capabilities_verified);
                assert_eq!(discovered.provider, provider);
                workspace
                    .save_provider_key(SaveProviderKey {
                        provider: provider.into(),
                        api_key: secret.clone(),
                    })
                    .await
                    .unwrap();
                assert_eq!(
                    vault.values.lock().unwrap().len(),
                    if provider == "openai" { 1 } else { 2 }
                );
                assert!(workspace
                    .create_agent(CreateLocalAgent {
                        hosted_consent: false,
                        ..input.clone()
                    })
                    .await
                    .is_err());
                let agent = workspace.create_agent(input.clone()).await.unwrap();
                let id = uuid::Uuid::new_v4().to_string();
                let task = workspace
                    .submit_for_agent(
                        id.clone(),
                        "Offer a useful thought".into(),
                        agent.key.clone(),
                    )
                    .await
                    .unwrap();
                let result = settled(&workspace, &id).await;
                assert_eq!(
                    result.state,
                    "completed",
                    "{}",
                    serde_json::to_string(&result).unwrap()
                );
                assert!(result
                    .messages
                    .iter()
                    .any(|m| m.content.contains("Hosted answer")));
                let calls_before_retry = transport.calls.lock().unwrap().len();
                assert_eq!(
                    workspace
                        .submit_for_agent(id.clone(), task.input, agent.key.clone())
                        .await
                        .unwrap()
                        .run_id,
                    task.run_id
                );
                assert_eq!(transport.calls.lock().unwrap().len(), calls_before_retry);
                let calls = transport.calls.lock().unwrap().clone();
                let body = calls.last().unwrap();
                assert_eq!(body["model"], model);
                assert!(body.to_string().contains("copper compass"));
                assert!(!body.to_string().contains(&secret));
                assert!(body.get("temperature").is_none());
                let names: Vec<_> = body["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|tool| {
                        tool["name"]
                            .as_str()
                            .or_else(|| tool["function"]["name"].as_str())
                            .unwrap()
                    })
                    .collect();
                assert_eq!(
                    names,
                    vec!["finish"],
                    "ambient tools must never reach hosted inference"
                );
                if provider == "openai" {
                    assert_eq!(body["max_output_tokens"], 1024);
                    assert_eq!(body["stream"], true);
                    assert_eq!(body["store"], false);
                    assert!(body.get("max_tokens").is_none());
                }
                let snapshot = serde_json::to_string(&workspace.snapshot().await.unwrap()).unwrap();
                assert!(!snapshot.contains(&secret));
                assert!(!snapshot.contains("key_reference"));
                // A prompt containing a credential must never reach the hosted transport.
                let blocked_id = uuid::Uuid::new_v4().to_string();
                workspace
                    .submit_for_agent(
                        blocked_id.clone(),
                        "Explain AKIAIOSFODNN7EXAMPLE".into(),
                        agent.key.clone(),
                    )
                    .await
                    .unwrap();
                assert_eq!(settled(&workspace, &blocked_id).await.state, "failed");
                assert_eq!(transport.calls.lock().unwrap().len(), calls_before_retry);
            }
            let store = workspace.keys.store.clone();
            drop(workspace);
            drop(store);
            let mut restored = LocalWorkspace::open_with_workspace(
                database.clone(),
                "offline".into(),
                "http://127.0.0.1:1".into(),
                workspace_root,
            )
            .await
            .unwrap();
            restored.keys = Arc::new(ProviderKeys {
                store: restored.keys.store.clone(),
                vault,
            });
            restored.hosted_transport = Some(transport.clone());
            assert!(restored.providers().await.iter().all(|p| p.key_saved));
            let agents = restored.agents().await.unwrap();
            let agent = agents.iter().find(|a| a.provider == "openai").unwrap();
            transport.wait.store(true, Ordering::SeqCst);
            let calls_before_cancel = transport.calls.lock().unwrap().len();
            let id = uuid::Uuid::new_v4().to_string();
            restored
                .submit_for_agent(id.clone(), "Wait here".into(), agent.key.clone())
                .await
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while transport.calls.lock().unwrap().len() == calls_before_cancel {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            restored.cancel(&id).await.unwrap();
            assert_eq!(settled(&restored, &id).await.state, "canceled");
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    let active = restored
                        .keys
                        .store
                        .read(|db| db.list_active_compute_reservation_rows())
                        .await
                        .unwrap()
                        .unwrap();
                    if active.is_empty() {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("cancellation must release the hosted reservation");
            transport.wait.store(false, Ordering::SeqCst);
            let next = uuid::Uuid::new_v4().to_string();
            restored
                .submit_for_agent(next.clone(), "Work after cancel".into(), agent.key.clone())
                .await
                .unwrap();
            assert_eq!(settled(&restored, &next).await.state, "completed");
            restored
                .remove_provider_key(RemoveProviderKey {
                    provider: "openai".into(),
                })
                .await
                .unwrap();
            assert!(!restored.keys.ready("openai").await);
            assert!(restored.provider_models("openai").await.is_err());
            assert!(restored.provider_models("unknown").await.is_err());
            assert!(restored
                .submit_for_agent(
                    uuid::Uuid::new_v4().to_string(),
                    "Missing key".into(),
                    agent.key.clone()
                )
                .await
                .is_err());
            // Removing an already removed key is safe and repeatable.
            restored
                .remove_provider_key(RemoveProviderKey {
                    provider: "openai".into(),
                })
                .await
                .unwrap();
        })
        .await;
}
