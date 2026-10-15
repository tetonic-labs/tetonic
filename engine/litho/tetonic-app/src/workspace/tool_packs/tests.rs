use super::*;
use crate::local_workspace::LocalWorkspace;

#[tokio::test]
async fn packs_pin_exact_tools_across_providers_and_never_bypass_consent_or_availability() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("files");
    std::fs::create_dir(&root).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = LocalWorkspace::open_with_workspace(
                dir.path().join("db"),
                "test".into(),
                "http://127.0.0.1:1".into(),
                Some(root.clone()),
            )
            .await
            .unwrap();
            let pack = workspace
                .save_tool_pack(SaveToolPack {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    id: uuid::Uuid::new_v4().to_string(),
                    expected_revision: 0,
                    name: "Research".into(),
                    description: "Read".into(),
                    tools: vec!["read_file".into(), "grep".into()],
                })
                .await
                .unwrap();
            let folder = workspace.services.available_folders().unwrap()[0].clone();
            let request = CreateLocalAgent {
                tool_pack_selection: Some(ToolPackSelection {
                    packs: vec![ToolPackRef {
                        id: pack.id.clone(),
                        revision: 1,
                    }],
                    excluded_tools: vec!["grep".into()],
                }),
                provider: "ollama".into(),
                hosted_consent: true,
                hosted_tools_consent: true,
                expected_workspace_root: Some(folder),
                workspace_root: None,
                request_id: uuid::Uuid::new_v4().to_string(),
                name: "Robin".into(),
                purpose: "Read".into(),
                model: "test".into(),
                harness: "general".into(),
                max_steps: 2,
                max_seconds: 60,
                max_tokens: 1024,
                tools: Some(vec!["read_file".into(), "list_dir".into()]),
            };
            for (provider, model) in [
                ("ollama", "test"),
                ("openai", "configured-model"),
                ("anthropic", "configured-model"),
                ("google", "configured-model"),
            ] {
                let mut input = request.clone();
                input.provider = provider.into();
                input.model = model.into();
                let config = workspace
                    .services
                    .resolved_agent_configuration(input.clone())
                    .await
                    .unwrap();
                assert_eq!(
                    config["requested_tools"],
                    serde_json::json!(["list_dir", "read_file"])
                );
                assert_eq!(config["tool_packs"][0]["revision"], 1);
                if provider != "ollama" {
                    input.hosted_tools_consent = false;
                    assert!(workspace
                        .services
                        .resolved_agent_configuration(input)
                        .await
                        .is_err());
                }
            }
            let original = workspace
                .services
                .resolved_agent_configuration(request.clone())
                .await
                .unwrap();
            workspace
                .save_tool_pack(SaveToolPack {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    id: pack.id.clone(),
                    expected_revision: 1,
                    name: pack.name,
                    description: pack.description,
                    tools: vec!["run_shell".into()],
                })
                .await
                .unwrap();
            assert_eq!(
                original,
                workspace
                    .services
                    .resolved_agent_configuration(request.clone())
                    .await
                    .unwrap()
            );
            let mut invalid = request.clone();
            invalid.tool_pack_selection.as_mut().unwrap().packs[0].revision = 999;
            assert!(workspace
                .services
                .resolved_agent_configuration(invalid)
                .await
                .is_err());
            let mut invalid = request.clone();
            invalid
                .tool_pack_selection
                .as_mut()
                .unwrap()
                .excluded_tools
                .push("made_up".into());
            assert!(workspace
                .services
                .resolved_agent_configuration(invalid)
                .await
                .is_err());
            for tool in [
                "finish",
                "dispatch_assignment",
                "made_up",
                "mcp_missing",
                "skill_missing",
            ] {
                assert!(workspace
                    .save_tool_pack(SaveToolPack {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        id: uuid::Uuid::new_v4().to_string(),
                        expected_revision: 0,
                        name: "Bad".into(),
                        description: String::new(),
                        tools: vec![tool.into()]
                    })
                    .await
                    .is_err());
            }
            let mut restricted = workspace.services.clone();
            restricted.host.settings.allowed_tools.remove("read_file");
            assert!(restricted
                .resolved_agent_configuration(request)
                .await
                .is_err());
        })
        .await;
}
