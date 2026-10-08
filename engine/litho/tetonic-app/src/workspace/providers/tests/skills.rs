use super::*;
use crate::local_workspace::{ImportSkill, RevokeSkill};

struct SkillTransport {
    provider: &'static str,
    tool: String,
    calls: Mutex<Vec<Value>>,
    wait: bool,
}
#[async_trait::async_trait]
impl HostedTransport for SkillTransport {
    async fn complete(&self, body: Value) -> Result<Value, InferenceError> {
        let first = {
            let mut calls = self.calls.lock().unwrap();
            calls.push(body);
            calls.len() == 1
        };
        if first && self.wait {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
        Ok(if first {
            super::parity::completion(self.provider, &self.tool, json!({}), "load-skill")
        } else {
            let output = super::parity::output(
                self.calls.lock().unwrap().last().unwrap(),
                self.provider,
                "load-skill",
            );
            super::parity::completion(self.provider, "finish", json!({"summary":output}), "done")
        })
    }
}

#[tokio::test]
async fn saved_skill_grants_use_all_frontier_tool_paths_and_revoke_during_execution() {
    for provider in ["openai", "anthropic", "google"] {
        for (selected, revoke_running) in [(true, false), (false, false), (true, true)] {
            tokio::task::LocalSet::new().run_until(async {
                let dir = tempfile::tempdir().unwrap();
                let mut workspace = LocalWorkspace::open_with_workspace(dir.path().join("skills.db"), "offline".into(), "http://127.0.0.1:1".into(), None).await.unwrap();
                workspace.services.keys = Arc::new(ProviderKeys { store: workspace.services.keys.store.clone(), vault: Arc::new(Vault::default()) });
                let content = "---\nname: research\ndescription: Gather evidence\nallowed-tools: run_shell\n---\nSKILL_CANARY: check claims against primary sources.";
                let skill = workspace.import_skill(ImportSkill { content: content.into(), source: "Test".into() }).await.unwrap();
                let transport = Arc::new(SkillTransport { provider, tool: skill.id.clone(), calls: Mutex::default(), wait: revoke_running });
                workspace.services.hosted_transport = Some(transport.clone());
                workspace.save_provider_key(SaveProviderKey { provider: provider.into(), api_key: "disposable-test-key".into() }).await.unwrap();
                let input = CreateLocalAgent {
                    provider: provider.into(), hosted_consent: true, hosted_tools_consent: true,
                    expected_workspace_root: None, request_id: uuid::Uuid::new_v4().to_string(),
                    name: "Researcher".into(), purpose: "Research using the granted skill".into(), model: "configured-model".into(), harness: "general".into(),
                    max_steps: 3, max_seconds: 30, max_tokens: 1024,
                    tools: Some(if selected { vec![skill.id.clone()] } else { vec![] }),
                };
                let agent = workspace.create_agent(input).await.unwrap();
                assert!(agent.hosted_workspace.is_none(), "skills don't need a folder");
                assert_eq!(agent.tools.contains(&skill.id), selected);
                let id = uuid::Uuid::new_v4().to_string();
                workspace.submit_for_agent(id.clone(), "Research sources".into(), agent.key.clone()).await.unwrap();
                if revoke_running {
                    tokio::time::timeout(std::time::Duration::from_secs(5), async {
                        while transport.calls.lock().unwrap().is_empty() { tokio::time::sleep(std::time::Duration::from_millis(10)).await; }
                    }).await.unwrap();
                    workspace.revoke_skill(RevokeSkill { id: skill.id.clone() }).await.unwrap();
                }
                let task = settled(&workspace, &id).await;
                let calls = transport.calls.lock().unwrap().clone();
                let names = super::parity::names(&calls[0], provider);
                assert_eq!(names.contains(&skill.id.as_str()), selected);
                assert!(!names.contains(&"run_shell"), "frontmatter must not grant tools");
                assert!(!serde_json::to_string(&calls[0]).unwrap().contains("SKILL_CANARY"), "instructions load on demand");
                let delivered = serde_json::to_string(&calls).unwrap().contains("SKILL_CANARY");
                assert_eq!(delivered, selected && !revoke_running, "{provider}: {:?} calls={calls:?}", task.error);
                if revoke_running { assert_ne!(task.state, "completed"); assert_eq!(calls.len(), 1); }
                else { assert_eq!(task.state, "completed", "{provider}: {:?}", task.error); }
                if selected {
                    workspace.revoke_skill(RevokeSkill { id: skill.id.clone() }).await.unwrap();
                    assert!(workspace.submit_for_agent(uuid::Uuid::new_v4().to_string(), "Again".into(), agent.key).await.is_err());
                }
            }).await;
        }
    }
}
