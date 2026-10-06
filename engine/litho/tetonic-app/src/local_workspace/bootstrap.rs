//! Local owner startup, default agents, and managed execution host configuration.
use std::path::PathBuf;

use super::*;
use crate::job_launch::{prepare_launch, RegisteredLaunchHost};
use crate::resources::{HarnessPreparationLimits, RegisteredExecutionSettings};

impl LocalWorkspace {
    /// Startup is a local owner operation, never an unauthenticated API endpoint.
    /// A dedicated database is required; existing unrelated principals are not adopted.
    pub async fn open(database: PathBuf, model: String, ollama: String) -> Result<Self, AppError> {
        Self::open_with_workspace(database, model, ollama, None).await
    }

    pub async fn open_with_workspace(
        database: PathBuf,
        model: String,
        ollama: String,
        workspace_root: Option<PathBuf>,
    ) -> Result<Self, AppError> {
        let local = LocalControl::open(database.clone(), AUDIENCE.into())
            .await
            .map_err(resource)?;
        match local
            .bootstrap(OWNER.into(), ORG.into(), "My workspace".into())
            .await
        {
            Ok(()) | Err(crate::resources::ResourceError::Conflict) => {}
            Err(error) => return Err(resource(error)),
        }
        let credential = local
            .credentials()
            .issue(OWNER.into(), 86_400)
            .await
            .map_err(resource)?;
        let secret = credential.expose_secret();
        let resources = local.resources();
        resources
            .create_team(secret, ORG.into(), TEAM.into(), "Personal".into())
            .await
            .map_err(resource)?;

        let has_workspace = workspace_root.is_some();
        let mut allowed_tools: std::collections::HashSet<String> = if has_workspace {
            [
                "read_file",
                "list_dir",
                "grep",
                "glob",
                "edit_file",
                "write_file",
                "run_shell",
                "finish",
            ]
            .into_iter()
            .map(String::from)
            .collect()
        } else {
            Default::default()
        };
        // Keep control state outside the command working folder. The process
        // sink enforces this too; do not offer an unusable terminal permission.
        let shell_ready = workspace_root
            .as_ref()
            .and_then(|path| tetonic_tools::Workspace::new(path).ok())
            .zip(database.canonicalize().ok())
            .is_some_and(|(workspace, database)| !database.starts_with(workspace.root()));
        if !shell_ready {
            allowed_tools.remove("run_shell");
        }

        let requested_tools: Vec<String> = if has_workspace {
            vec![
                "read_file".into(),
                "list_dir".into(),
                "grep".into(),
                "glob".into(),
                "edit_file".into(),
                "write_file".into(),
            ]
        } else {
            vec![]
        };

        let default_instructions = if has_workspace {
            "You are a local engineering assistant with access to the workspace. Inspect files, search code, list directories, and make bounded modifications to solve the owner's request. Call finish with your complete answer and summary when done."
        } else {
            "Help the owner think through their request. Answer clearly and concisely. Call finish with your complete answer as the summary. You have no web, filesystem, shell or external tools; do not claim to have performed actions or research."
        };

        match resources
            .register_agent(
                secret,
                ORG.into(),
                AGENT.into(),
                "general".into(),
                serde_json::json!({
                    "instructions": default_instructions,
                    "requested_tools": requested_tools
                }),
            )
            .await
        {
            Ok(_) => {}
            Err(crate::resources::ResourceError::Conflict) => {
                let _ = resources
                    .publish_agent_revision(
                        secret,
                        ORG.into(),
                        AGENT.into(),
                        "general".into(),
                        serde_json::json!({
                            "instructions": default_instructions,
                            "requested_tools": requested_tools
                        }),
                    )
                    .await;
            }
            Err(e) => return Err(resource(e)),
        }
        let default_personas = [
            (shaping::GUIDE, shaping::GUIDE_INSTRUCTIONS),
            ("The Digest Assistant", "You are The Digest Assistant, an executive synthesis assistant. Provide high-level status standups, highlight completed milestones, and answer progress queries."),
            ("Researcher", "You are the Team Researcher. Investigate questions, synthesize facts, and structure comprehensive briefings using workspace inspection tools."),
            ("Analyst", "You are the Team Analyst. Evaluate plans, identify quantitative tradeoffs, critique proposals, and review execution quality."),
        ];
        for (name, instructions) in default_personas {
            let persona_tools = if name == shaping::GUIDE {
                vec![]
            } else {
                requested_tools.clone()
            };
            if resources
                .register_agent(
                    secret,
                    ORG.into(),
                    name.into(),
                    "general".into(),
                    serde_json::json!({
                        "instructions": instructions,
                        "requested_tools": persona_tools,
                    }),
                )
                .await
                .is_err()
            {
                let _ = resources
                    .publish_agent_revision(
                        secret,
                        ORG.into(),
                        name.into(),
                        "general".into(),
                        serde_json::json!({
                            "instructions": instructions,
                            "requested_tools": persona_tools,
                        }),
                    )
                    .await;
            }
        }
        // The Guide is engine-managed. Update its selected revision deliberately;
        // merely publishing leaves existing installations on old instructions.
        if let Some(guide) = resources
            .get_agent(secret, ORG.into(), shaping::GUIDE.into())
            .await
            .map_err(resource)?
        {
            let envelope: serde_json::Value = serde_json::from_str(&guide.definition_json)
                .map_err(|_| AppError::InferenceUnavailable)?;
            let mut configuration = envelope["configuration"].clone();
            if configuration["instructions"].as_str() != Some(shaping::GUIDE_INSTRUCTIONS) {
                configuration["instructions"] = shaping::GUIDE_INSTRUCTIONS.into();
                resources
                    .edit_agent(
                        secret,
                        crate::resources::EditAgent {
                            org: ORG.into(),
                            key: shaping::GUIDE.into(),
                            request: format!(
                                "guide-conversation-{}",
                                guide.identity.bound_definition_digest
                            ),
                            expected: guide.identity.bound_definition_digest,
                            harness: "general".into(),
                            configuration,
                        },
                    )
                    .await
                    .map_err(resource)?;
            }
        }
        let context = local
            .contexts()
            .team_participation_context(secret, ORG.into(), TEAM.into())
            .await
            .map_err(resource)?;
        let host = prepare_launch(
            secret,
            RegisteredLaunchHost {
                database,
                audience: AUDIENCE.into(),
                ollama,
                settings: RegisteredExecutionSettings {
                    mcp: None,
                    plan_dispatch: None,
                    response_schema: None,
                    hosted: None,
                    max_elapsed_seconds: 120,
                    reported_token_ceiling: Some(4096),
                    workspace_root,
                    model,
                    num_ctx: 8192,
                    data_class: DataClass::Secret,
                    allowed_tools,
                    limits: HarnessPreparationLimits {
                        human_handoff: false,
                        max_steps: if has_workspace { 8 } else { 4 },
                        max_input_bytes: INPUT_LIMIT,
                    },
                },
            },
        )
        .await?;
        let keys = std::sync::Arc::new(providers::ProviderKeys {
            store: host
                .app
                .turn
                .store
                .clone()
                .ok_or(AppError::InferenceUnavailable)?,
            vault: std::sync::Arc::new(tetonic_secrets::key_storage::PlatformKeyStorage),
        });
        Ok(Self {
            local,
            host,
            context,
            admission: tokio::sync::Mutex::new(()),
            keys,
            #[cfg(test)]
            hosted_transport: None,
        })
    }
}
