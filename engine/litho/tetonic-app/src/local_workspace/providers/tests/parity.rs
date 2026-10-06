//! Same managed grants and tool hosts; different provider wire protocols.
use super::*;

pub(super) fn completion(provider: &str, name: &str, args: Value, id: &str) -> Value {
    match provider {
        "openai" => {
            json!({"status":"completed","output":[{"type":"function_call","call_id":id,"name":name,"arguments":args.to_string()}],"usage":{"input_tokens":20,"output_tokens":20}})
        }
        "anthropic" => {
            json!({"role":"assistant","content":[{"type":"tool_use","id":id,"name":name,"input":args}],"stop_reason":"tool_use","usage":{"input_tokens":20,"output_tokens":20}})
        }
        "google" => {
            json!({"candidates":[{"finishReason":"STOP","content":{"role":"model","parts":[{"functionCall":{"id":id,"name":name,"args":args},"thoughtSignature":"opaque-signature"}]}}],"usageMetadata":{"promptTokenCount":20,"candidatesTokenCount":15,"thoughtsTokenCount":5}})
        }
        _ => panic!("unsupported fixture provider"),
    }
}

pub(super) fn output(body: &Value, provider: &str, expected_id: &str) -> String {
    match provider {
        "openai" => {
            let item = body["input"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find(|i| i["type"] == "function_call_output")
                .expect("actual tool result");
            assert_eq!(item["call_id"], expected_id);
            item["output"].as_str().unwrap().into()
        }
        "anthropic" => {
            let item = body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .flat_map(|m| m["content"].as_array().unwrap())
                .find(|p| p["type"] == "tool_result")
                .expect("actual tool result");
            assert_eq!(item["tool_use_id"], expected_id);
            item["content"].as_str().unwrap().into()
        }
        "google" => {
            let item = body["contents"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .flat_map(|m| m["parts"].as_array().unwrap())
                .find_map(|p| p.get("functionResponse"))
                .expect("actual tool result");
            assert_eq!(item["id"], expected_id);
            item["response"]["output"].as_str().unwrap().into()
        }
        _ => panic!("unsupported fixture provider"),
    }
}

pub(super) fn names<'a>(body: &'a Value, provider: &str) -> Vec<&'a str> {
    let tools = if provider == "google" {
        &body["tools"][0]["functionDeclarations"]
    } else {
        &body["tools"]
    };
    tools
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect()
}

struct McpTransport {
    provider: &'static str,
    tool: String,
    calls: Mutex<Vec<Value>>,
}
#[async_trait::async_trait]
impl HostedTransport for McpTransport {
    async fn complete(&self, body: Value) -> Result<Value, InferenceError> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(body.clone());
        Ok(if calls.len() == 1 {
            completion(
                self.provider,
                &self.tool,
                json!({"query":"Tuesday availability"}),
                "real-call",
            )
        } else {
            completion(
                self.provider,
                "finish",
                json!({"summary":output(&body,self.provider,"real-call")}),
                "done",
            )
        })
    }
}

#[tokio::test]
async fn every_hosted_provider_uses_selected_mcp_and_denies_ungranted_or_changed_tools() {
    for provider in ["openai", "anthropic", "google"] {
        for (selected, changed, stop) in [
            (true, false, false),
            (false, false, false),
            (true, true, false),
            (true, false, true),
        ] {
            tokio::task::LocalSet::new()
                .run_until(async {
                    let fixture = crate::mcp::tests::fixture::Fixture::new().await;
                    let dir = tempfile::tempdir().unwrap();
                    let mut workspace = LocalWorkspace::open_with_workspace(
                        dir.path().join("state.db"),
                        "offline".into(),
                        "http://127.0.0.1:1".into(),
                        None,
                    )
                    .await
                    .unwrap()
                    .with_mcp_config(&fixture.config)
                    .unwrap();
                    workspace.discover_mcp("calendar").await.unwrap();
                    let catalog = workspace.agent_catalog().await.unwrap();
                    let tool = catalog.mcp_connections[0]
                        .tools
                        .iter()
                        .find(|t| t.name == "search")
                        .unwrap()
                        .id
                        .clone();
                    let transport = Arc::new(McpTransport {
                        provider,
                        tool: tool.clone(),
                        calls: Mutex::default(),
                    });
                    workspace.hosted_transport = Some(transport.clone());
                    workspace.keys = Arc::new(ProviderKeys {
                        store: workspace.keys.store.clone(),
                        vault: Arc::new(Vault::default()),
                    });
                    workspace
                        .save_provider_key(SaveProviderKey {
                            provider: provider.into(),
                            api_key: "disposable-provider-key".into(),
                        })
                        .await
                        .unwrap();
                    let input = CreateLocalAgent {
                        provider: provider.into(),
                        hosted_consent: true,
                        hosted_tools_consent: false,
                        expected_workspace_root: None,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        name: "Scheduler".into(),
                        purpose: "Check calendar availability".into(),
                        model: "fixture-model".into(),
                        harness: "general".into(),
                        max_steps: 3,
                        max_seconds: 30,
                        max_tokens: 1024,
                        tools: Some(if selected { vec![tool.clone()] } else { vec![] }),
                    };
                    if selected {
                        assert!(
                            workspace.create_agent(input.clone()).await.is_err(),
                            "MCP data disclosure needs consent without a folder"
                        );
                    }
                    let agent = workspace
                        .create_agent(CreateLocalAgent {
                            hosted_tools_consent: true,
                            ..input
                        })
                        .await
                        .unwrap();
                    assert!(agent.hosted_workspace.is_none());
                    if let Some(d) = &agent.tool_disclosure {
                        assert_eq!(d.tools, vec![tool.clone()]);
                    }
                    let mut tampered = agent.clone();
                    if let Some(d) = &mut tampered.tool_disclosure {
                        d.provider = "other".into();
                        assert!(workspace.hosted_binding(&tampered).await.is_err());
                    }
                    if changed {
                        fixture.mode.store(1, Ordering::SeqCst);
                    }
                    if stop {
                        fixture.mode.store(3, Ordering::SeqCst);
                    }
                    let id = uuid::Uuid::new_v4().to_string();
                    workspace
                        .submit_for_agent(id.clone(), "Check Tuesday".into(), agent.key.clone())
                        .await
                        .unwrap();
                    if stop {
                        tokio::time::timeout(std::time::Duration::from_secs(5), async {
                            while !fixture
                                .calls
                                .lock()
                                .unwrap()
                                .iter()
                                .any(|r| r["method"] == "tools/call")
                            {
                                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                            }
                        })
                        .await
                        .unwrap();
                        workspace.cancel(&id).await.unwrap();
                    }
                    let result = settled(&workspace, &id).await;
                    let calls = transport.calls.lock().unwrap().clone();
                    assert_eq!(
                        names(&calls[0], provider).contains(&tool.as_str()),
                        selected
                    );
                    assert!(!names(&calls[0], provider).contains(&"read_file"));
                    let external = fixture
                        .calls
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|r| r["method"] == "tools/call")
                        .count();
                    if stop {
                        assert_eq!(result.state, "canceled");
                        assert_eq!(calls.len(), 1);
                    } else {
                        assert_eq!(result.state, "completed", "{provider}: {:?}", result.error);
                        let returned = output(&calls[1], provider, "real-call");
                        assert_eq!(returned.contains("Tuesday 10:00"), selected && !changed);
                        assert_eq!(external, usize::from(selected && !changed));
                    }
                    // Removing the key invalidates access without changing tool grants.
                    workspace
                        .remove_provider_key(RemoveProviderKey {
                            provider: provider.into(),
                        })
                        .await
                        .unwrap();
                    assert!(workspace.hosted_binding(&agent).await.is_err());
                })
                .await;
        }
    }
}
