use super::*;

// The village-mcp harness owns an isolated durable v5 world and three services.
// This exercises the actual reviewed client boundary, not autonomous decisions.
#[tokio::test]
#[ignore = "requires TETONIC_MCP_GAME_ENDPOINT pointing at an isolated Village MCP"]
async fn village_gameplay_through_tetonic_mcp() {
    let endpoint = std::env::var("TETONIC_MCP_GAME_ENDPOINT").expect("isolated MCP endpoint");
    let config = json!({"connections":[{"id":"village","name":"Village","endpoint":endpoint,"action_tools":[
        "village_perceive", "village_inspect", "village_write", "village_read",
        "village_use", "village_receipt", "village_disconnect"
    ]}]});
    let registry = McpRegistry::from_json(&serde_json::to_vec(&config).unwrap()).unwrap();
    let view = registry.refresh("village").await.unwrap();
    assert_eq!(view.status, "discovered", "{}", view.message);
    assert_eq!(view.tools.len(), 7);
    assert!(view.tools.iter().all(|t| !t.read_only));
    assert!(
        view.tools
            .iter()
            .find(|t| t.name == "village_use")
            .unwrap()
            .destructive
    );
    let selected: HashSet<String> = view.tools.iter().map(|t| t.id.clone()).collect();
    assert_eq!(registry.advertisements(&selected).len(), 7);
    assert!(registry.advertisements(&HashSet::new()).is_empty());
    assert_eq!(
        registry
            .reviewed_manifests("village", &selected.iter().cloned().collect::<Vec<_>>())
            .unwrap()
            .len(),
        7
    );
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
    let perception = tokio::time::timeout(std::time::Duration::from_secs(25), async {
        loop {
            let p = invoke("village_perceive", json!({})).await;
            assert!(p.ok, "{p:?}");
            let data: Value = serde_json::from_str(&p.content).unwrap();
            assert_eq!(data["state"]["schema_id"], "village_local_v5");
            if data["state"]["data"]["clock"]["paused"].is_null() {
                break data;
            }
        }
    })
    .await
    .unwrap();
    let context = |p: &Value| {
        let data = &p["state"]["data"];
        let mut revisions = serde_json::Map::new();
        revisions.insert(
            data["body"]["id"].as_str().unwrap().into(),
            data["body"]["revision"].clone(),
        );
        for item in data["inventory"].as_array().unwrap() {
            revisions.insert(
                item["id"].as_str().unwrap().into(),
                item["revision"].clone(),
            );
        }
        json!({"world_context_revision":data["_world_context_revision"],"world_generation":data["world_generation"],"expected_revisions":revisions})
    };
    let mut write_args = context(&perception);
    write_args["target_id"] = json!("tetonic-mcp-test-document");
    write_args["text"] = json!("A durable claim through the Tetonic MCP client.");
    write_args["claimed_author"] = json!("Another resident");
    let written = invoke("village_write", write_args).await;
    assert!(written.ok, "{written:?}");
    let receipt: Value = serde_json::from_str(&written.content).unwrap();
    let lookup = invoke(
        "village_receipt",
        json!({"receipt_action_id":receipt["action_id"]}),
    )
    .await;
    assert!(lookup.ok, "{lookup:?}");
    assert!(lookup.content.contains("completed"));
    let p = invoke("village_perceive", json!({})).await;
    assert!(p.ok, "{p:?}");
    let p: Value = serde_json::from_str(&p.content).unwrap();
    let mut read_args = context(&p);
    read_args["target_id"] = json!("tetonic-mcp-test-document");
    let read = invoke("village_read", read_args.clone()).await;
    assert!(
        read.ok && read.content.contains("authored_claim"),
        "{read:?}"
    );
    assert!(!read.content.contains("\"writer\""));
    read_args["target_id"] = json!("resident-b-document");
    let hidden = invoke("village_inspect", read_args).await;
    assert!(
        !hidden.ok && hidden.content.contains("unavailable"),
        "{hidden:?}"
    );
    let mut stale = context(&p);
    stale["world_generation"] = json!(999);
    stale["target_id"] = json!("tetonic-mcp-test-document");
    stale["text"] = json!("Must not commit");
    let rejected = invoke("village_write", stale).await;
    assert!(
        !rejected.ok && rejected.content.contains("stale_context"),
        "{rejected:?}"
    );
    assert!(invoke("village_disconnect", json!({})).await.ok);
}
