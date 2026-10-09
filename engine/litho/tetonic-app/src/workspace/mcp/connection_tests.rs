#![cfg(test)]
use super::*;
use serde_json::json;
use std::sync::{Arc, Mutex};
use tetonic_domain::key_storage::{KeyStorage, KeyStorageError, SecretBytes, SecretKeyRef};

#[derive(Default)]
struct Vault(Mutex<std::collections::HashMap<String, Vec<u8>>>);
impl KeyStorage for Vault {
    fn create(&self, value: &[u8]) -> Result<SecretKeyRef, KeyStorageError> {
        let id = uuid::Uuid::new_v4().to_string();
        self.0.lock().unwrap().insert(id.clone(), value.to_vec());
        Ok(SecretKeyRef(id))
    }
    fn read(&self, key: &SecretKeyRef) -> Result<SecretBytes, KeyStorageError> {
        self.0
            .lock()
            .unwrap()
            .get(&key.0)
            .cloned()
            .map(SecretBytes::new)
            .ok_or(KeyStorageError::Missing)
    }
    fn delete(&self, key: &SecretKeyRef) -> Result<(), KeyStorageError> {
        self.0.lock().unwrap().remove(&key.0);
        Ok(())
    }
}
fn connect(endpoint: &str) -> SaveMcpConnection {
    SaveMcpConnection {
        id: "saved_calendar".into(),
        expected_revision: 0,
        name: "Calendar".into(),
        endpoint: endpoint.into(),
        auth: "bearer".into(),
        token: Some("TEST_SERVICE_TOKEN".into()),
        enabled: true,
        approved_tools: None,
    }
}
fn edit(view: &crate::mcp::McpConnectionView) -> SaveMcpConnection {
    SaveMcpConnection {
        id: view.id.clone(),
        expected_revision: view.revision,
        name: view.name.clone(),
        endpoint: view.endpoint.clone(),
        auth: view.auth.clone(),
        token: None,
        enabled: view.enabled,
        approved_tools: None,
    }
}
fn vault(workspace: &mut LocalWorkspace, vault: Arc<Vault>) {
    workspace.services.keys = Arc::new(providers::ProviderKeys {
        store: workspace.services.keys.store.clone(),
        vault,
    });
    workspace.services.host.settings.mcp =
        Some(crate::mcp::McpRegistry::load(workspace.services.mcp_scope()).unwrap());
}

#[tokio::test]
async fn connection_review_restart_real_tool_execution_and_revocation_use_existing_managed_path() {
    let fixture = crate::mcp::tests::fixture::Fixture::new().await;
    fixture.mode.store(8, std::sync::atomic::Ordering::SeqCst);
    let config: serde_json::Value = serde_json::from_slice(&fixture.config).unwrap();
    let endpoint = config["connections"][0]["endpoint"].as_str().unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("connections.db");
            let keys = Arc::new(Vault::default());
            let mut workspace = LocalWorkspace::open(
                path.clone(),
                "qwen3.5:latest".into(),
                "http://127.0.0.1:1".into(),
            )
            .await
            .unwrap();
            vault(&mut workspace, keys.clone());
            let saved = workspace
                .save_mcp_connection(connect(endpoint))
                .await
                .unwrap();
            assert_eq!(saved.revision, 1);
            assert!(workspace
                .save_mcp_connection(connect(endpoint))
                .await
                .is_err());
            assert_eq!(
                keys.0.lock().unwrap().len(),
                1,
                "stale saves cannot leak vault entries"
            );
            let discovered = workspace.discover_mcp(&saved.id).await.unwrap();
            assert_eq!(discovered.tools.len(), 2);
            assert!(discovered.tools.iter().all(|t| !t.approved));
            assert!(workspace
                .services
                .mcp_registry()
                .unwrap()
                .tool_names()
                .is_empty());
            let tool = discovered
                .tools
                .iter()
                .find(|t| t.name == "search")
                .unwrap()
                .id
                .clone();
            let mut review = edit(&saved);
            review.approved_tools = Some(vec![tool.clone()]);
            let approved = workspace.save_mcp_connection(review).await.unwrap();
            assert_eq!(
                workspace.services.mcp_registry().unwrap().tool_names(),
                vec![tool.clone()]
            );
            assert!(!serde_json::to_string(&approved)
                .unwrap()
                .contains("TEST_SERVICE_TOKEN"));
            let mut redirect = edit(&approved);
            redirect.endpoint = "https://other.example/mcp".into();
            assert!(workspace.save_mcp_connection(redirect).await.is_err());
            drop(workspace);
            let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_tool(
                true,
                &tool,
                json!({"query":"Tuesday"}),
            )
            .await;
            let mut workspace = LocalWorkspace::open(path.clone(), "qwen3.5:latest".into(), url)
                .await
                .unwrap();
            vault(&mut workspace, keys.clone());
            assert_eq!(
                workspace.services.mcp_registry().unwrap().tool_names(),
                vec![tool.clone()]
            );
            let agent = workspace
                .create_agent(CreateLocalAgent {
                    workspace_root: None,
                    provider: "ollama".into(),
                    hosted_consent: false,
                    hosted_tools_consent: false,
                    expected_workspace_root: None,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    name: "Calendar researcher".into(),
                    purpose: "Read availability".into(),
                    model: "qwen3.5:latest".into(),
                    harness: "general".into(),
                    max_steps: 4,
                    max_seconds: 30,
                    max_tokens: 1024,
                    tools: Some(vec![tool.clone()]),
                })
                .await
                .unwrap();
            let work = uuid::Uuid::new_v4().to_string();
            workspace
                .submit_for_agent(work.clone(), "Check availability".into(), agent.key.clone())
                .await
                .unwrap();
            let task = tokio::time::timeout(std::time::Duration::from_secs(15), async {
                loop {
                    let t = workspace.task(&work).await.unwrap();
                    if matches!(t.state.as_str(), "completed" | "failed" | "canceled") {
                        break t;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            assert_eq!(task.state, "completed", "{:?}", task.error);
            assert!(calls
                .lock()
                .unwrap()
                .iter()
                .any(|call| call.to_string().contains("Tuesday 10:00")));
            let mut disconnect = edit(&approved);
            disconnect.enabled = false;
            fixture.mode.store(3, std::sync::atomic::Ordering::SeqCst);
            let previous_calls = fixture
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|v| v["method"] == "tools/call")
                .count();
            let registry = workspace.services.host.settings.mcp.clone().unwrap();
            let args = json!({"query":"Wednesday"});
            let cancel = tetonic_domain::work_scope::CancellationSignal::default();
            let pending = registry.call(&tool, &args, &cancel);
            let revoke = async {
                while fixture
                    .calls
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|v| v["method"] == "tools/call")
                    .count()
                    == previous_calls
                {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                workspace.save_mcp_connection(disconnect).await.unwrap()
            };
            let (interrupted, stopped) =
                tokio::time::timeout(std::time::Duration::from_secs(1), async {
                    tokio::join!(pending, revoke)
                })
                .await
                .unwrap();
            assert!(!interrupted.ok);
            assert!(interrupted.content.contains("not confirmed"));
            assert_eq!(stopped.status, "disconnected");
            assert!(keys.0.lock().unwrap().is_empty());
            assert!(workspace
                .services
                .mcp_registry()
                .unwrap()
                .tool_names()
                .is_empty());
            assert!(
                !workspace
                    .services
                    .mcp_registry()
                    .unwrap()
                    .call(
                        &tool,
                        &json!({}),
                        &tetonic_domain::work_scope::CancellationSignal::default()
                    )
                    .await
                    .ok
            );
            let actual = workspace
                .services
                .agents()
                .await
                .unwrap()
                .into_iter()
                .find(|a| a.key == agent.key)
                .unwrap();
            assert_eq!(
                actual.tools, agent.tools,
                "disconnect must not rewrite saved agent definitions"
            );
            server.abort();
        })
        .await;
}

#[tokio::test]
async fn token_rotation_invalidates_old_manifest_bindings_and_uncertain_saves_cannot_duplicate_credentials(
) {
    let fixture = crate::mcp::tests::fixture::Fixture::new().await;
    let config: serde_json::Value = serde_json::from_slice(&fixture.config).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let mut workspace = LocalWorkspace::open(
                dir.path().join("state.db"),
                "model".into(),
                "http://127.0.0.1:1".into(),
            )
            .await
            .unwrap();
            let keys = Arc::new(Vault::default());
            vault(&mut workspace, keys.clone());
            let saved = workspace
                .save_mcp_connection(connect(
                    config["connections"][0]["endpoint"].as_str().unwrap(),
                ))
                .await
                .unwrap();
            let discovered = workspace.discover_mcp(&saved.id).await.unwrap();
            let tool = discovered.tools[0].id.clone();
            let mut request = edit(&saved);
            request.approved_tools = Some(vec![tool.clone()]);
            let reviewed = workspace.save_mcp_connection(request).await.unwrap();
            let mut rotate = edit(&reviewed);
            rotate.token = Some("REPLACEMENT_TOKEN".into());
            let rotated = workspace.save_mcp_connection(rotate).await.unwrap();
            assert_eq!(keys.0.lock().unwrap().len(), 1);
            assert!(rotated.tools.is_empty());
            assert!(!workspace.services.mcp_registry().unwrap().contains(&tool));
            let refreshed = workspace.discover_mcp(&saved.id).await.unwrap();
            assert!(refreshed.tools.iter().all(|t| t.id != tool && !t.approved));
            let mut forged = edit(&rotated);
            forged.approved_tools = Some(vec![tool]);
            assert!(workspace.save_mcp_connection(forged).await.is_err());
            fixture.mode.store(6, std::sync::atomic::Ordering::SeqCst);
            let auth_failure = workspace.discover_mcp(&saved.id).await.unwrap();
            assert_eq!(auth_failure.status, "unavailable");
            assert!(auth_failure.message.contains("authentication"));
            assert!(!auth_failure.message.contains("REPLACEMENT_TOKEN"));
        })
        .await;
}
