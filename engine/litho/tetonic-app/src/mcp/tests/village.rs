use super::*;

// Run only against a fresh, isolated world: this test moves a resident and
// transfers timber. The village-mcp test harness owns both server processes.
#[tokio::test]
#[ignore = "requires TETONIC_MCP_GAME_ENDPOINT pointing at an isolated Village MCP"]
async fn village_gameplay_through_tetonic_mcp() {
    let endpoint = std::env::var("TETONIC_MCP_GAME_ENDPOINT").expect("isolated MCP endpoint");
    let config = json!({"connections":[{"id":"village","name":"Village","endpoint":endpoint,"action_tools":[
        "village_perceive", "village_available_actions", "village_inspect", "village_navigate",
        "village_interact", "village_speak", "village_idle", "village_disconnect"
    ]}]});
    let registry = McpRegistry::from_json(&serde_json::to_vec(&config).unwrap()).unwrap();
    let view = registry.refresh("village").await.unwrap();
    assert_eq!(view.status, "discovered", "{}", view.message);
    assert_eq!(view.tools.len(), 8);
    assert!(view.tools.iter().all(|t| !t.read_only));
    let invoke = |name: &'static str, args: Value| {
        let registry = registry.clone();
        let id = view
            .tools
            .iter()
            .find(|t| t.name == name)
            .unwrap()
            .id
            .clone();
        async move {
            registry
                .call(&id, &args, &CancellationSignal::default())
                .await
        }
    };
    let perception = invoke("village_perceive", json!({})).await;
    assert!(perception.ok, "{perception:?}");
    let perception: Value = serde_json::from_str(&perception.content).unwrap();
    let revision = perception["state"]["data"]["_world_context_revision"].clone();
    let bundle = perception["state"]["data"]["observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == "wood-bundle")
        .expect("observed bundle in isolated base world")["id"]
        .clone();
    let options = invoke(
        "village_available_actions",
        json!({"world_context_revision":revision}),
    )
    .await;
    assert!(options.ok, "{options:?}");
    let inspected = invoke(
        "village_inspect",
        json!({"world_context_revision":revision,"target_id":bundle}),
    )
    .await;
    assert!(inspected.ok, "{inspected:?}");
    let rejected = invoke(
        "village_interact",
        json!({"world_context_revision":revision,"target_id":bundle,"verb":"pickup"}),
    )
    .await;
    assert!(
        !rejected.ok && rejected.content.contains("adjacent"),
        "{rejected:?}"
    );
    let navigation = invoke(
        "village_navigate",
        json!({"world_context_revision":revision,"target_id":bundle}),
    )
    .await;
    assert!(
        navigation.ok && navigation.content.contains("Journey accepted"),
        "{navigation:?}"
    );
    tokio::time::timeout(std::time::Duration::from_secs(25), async {
        loop {
            let p = invoke("village_perceive", json!({})).await;
            assert!(p.ok, "{p:?}");
            let data: Value = serde_json::from_str(&p.content).unwrap();
            if data["state"]["data"]["task"]["status"] == "arrived" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    let pickup = invoke(
        "village_interact",
        json!({"world_context_revision":revision,"target_id":bundle,"verb":"pickup"}),
    )
    .await;
    assert!(pickup.ok, "{pickup:?}");
    let p = invoke("village_perceive", json!({})).await;
    let data: Value = serde_json::from_str(&p.content).unwrap();
    assert!(data["state"]["data"]["inventory"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["item"] == "timber" && item["count"].as_u64().unwrap_or(0) > 0));
    let speech = invoke(
        "village_speak",
        json!({"world_context_revision":revision,"text":"I collected timber through Tetonic MCP."}),
    )
    .await;
    assert!(speech.ok, "{speech:?}");
    assert!(invoke("village_disconnect", json!({})).await.ok);
}
