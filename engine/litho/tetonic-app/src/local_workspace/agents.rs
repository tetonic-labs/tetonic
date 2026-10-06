use super::*;
use crate::resources::GeneralAgentPreferences;
use futures::{StreamExt, TryStreamExt};
use serde::Deserialize;

mod profiles;
pub use profiles::LocalAgentRuntimeProfile;

#[derive(Clone, Serialize)]
pub struct LocalAgent {
    pub hosted_workspace: Option<String>,
    pub plan_coordinator: bool,
    pub provider: String,
    pub hosted_consent: bool,
    pub key: String,
    pub id: String,
    pub name: String,
    pub purpose: String,
    pub model: String,
    pub harness: String,
    pub max_steps: usize,
    pub max_seconds: u64,
    pub max_tokens: u64,
    pub tools: Vec<String>,
}

#[derive(Serialize)]
pub struct LocalAgentCatalog {
    pub workspace_root: Option<String>,
    pub runtime_profiles: Vec<LocalAgentRuntimeProfile>,
    pub providers: Vec<LocalProvider>,
    pub local_error: Option<String>,
    pub models: Vec<String>,
    pub harnesses: Vec<String>,
    /// Tools actually permitted by this host for local model execution.
    pub tools: Vec<String>,
    pub max_steps: usize,
    pub max_seconds: u64,
    pub max_tokens: u64,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateLocalAgent {
    #[serde(default = "local_provider")]
    pub provider: String,
    #[serde(default)]
    pub hosted_consent: bool,
    #[serde(default)]
    pub hosted_tools_consent: bool,
    pub request_id: String,
    pub name: String,
    pub purpose: String,
    pub model: String,
    pub harness: String,
    pub max_steps: usize,
    pub max_seconds: u64,
    pub max_tokens: u64,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
}

fn local_provider() -> String {
    "ollama".into()
}

impl LocalWorkspace {
    pub async fn agent_catalog(&self) -> Result<LocalAgentCatalog, AppError> {
        let (models, local_error) = match self.installed_models().await {
            Ok(models) => (models, None),
            Err(error) => (Vec::new(), Some(error.employee_message().to_string())),
        };
        let mut tools: Vec<_> = self
            .host
            .settings
            .allowed_tools
            .iter()
            .filter(|tool| tool.as_str() != "finish")
            .cloned()
            .collect();
        tools.sort();
        Ok(LocalAgentCatalog {
            workspace_root: self
                .host
                .settings
                .workspace_root
                .as_ref()
                .and_then(|path| tetonic_tools::Workspace::new(path).ok())
                .map(|workspace| workspace.root().to_string_lossy().into_owned()),
            runtime_profiles: self.agent_runtime_profiles(),
            models,
            local_error,
            providers: self.providers().await,
            harnesses: vec!["general".into()],
            tools,
            max_steps: self.host.settings.limits.max_steps,
            max_seconds: self.host.settings.max_elapsed_seconds,
            max_tokens: self.host.settings.reported_token_ceiling.unwrap_or(4096),
        })
    }

    async fn installed_models(&self) -> Result<Vec<String>, AppError> {
        let provider = tetonic_inference::OllamaProvider::new(
            self.host.app.turn.ollama_base(),
            self.host.app.turn.guard(),
        );
        let mut models =
            tokio::time::timeout(std::time::Duration::from_secs(3), provider.list_models())
                .await
                .map_err(|_| {
                    AppError::InvalidRequest(
                        "Local model discovery timed out. Check Ollama and retry.".into(),
                    )
                })?
                .map_err(|_| {
                    AppError::InvalidRequest(
                        "Cannot read installed models. Check Ollama and retry.".into(),
                    )
                })?;
        models.sort();
        models.dedup();
        let compatible = futures::stream::iter(models)
            .map(|model| {
                let provider = &provider;
                async move {
                    let info = provider.model_info(&model).await?;
                    Ok::<_, tetonic_inference::InferenceError>(
                        info.capabilities
                            .iter()
                            .any(|capability| capability == "tools")
                            .then_some(model),
                    )
                }
            })
            .buffered(4)
            .try_collect::<Vec<_>>();
        let models = tokio::time::timeout(std::time::Duration::from_secs(3), compatible)
            .await
            .map_err(|_| {
                AppError::InvalidRequest(
                    "Local model capability lookup timed out. Retry discovery.".into(),
                )
            })?
            .map_err(|_| {
                AppError::InvalidRequest(
                    "Could not check installed model capabilities. Retry discovery.".into(),
                )
            })?
            .into_iter()
            .flatten()
            .collect();
        Ok(models)
    }

    pub async fn create_agent(&self, mut input: CreateLocalAgent) -> Result<LocalAgent, AppError> {
        validate_request_id(&input.request_id)?;
        input.name = input.name.trim().into();
        input.purpose = input.purpose.trim().into();
        if input.name.is_empty()
            || input.name.chars().count() > 60
            || input.name.chars().any(char::is_control)
            || input.purpose.len() > 4000
            || input.purpose.contains('\0')
            || input.model.is_empty()
            || input.model.len() > 256
            || input
                .model
                .chars()
                .any(|c| c.is_control() || c.is_whitespace())
        {
            return Err(AppError::InvalidRequest(
                "Invalid agent name, purpose, model or harness.".into(),
            ));
        }
        self.check_limits(input.max_steps, input.max_seconds, input.max_tokens)?;
        let profile = self
            .agent_runtime_profiles()
            .into_iter()
            .find(|profile| profile.provider == input.provider && profile.harness == input.harness)
            .ok_or_else(|| {
                AppError::InvalidRequest(
                    "This model provider and harness cannot run together on this host.".into(),
                )
            })?;
        if !matches!(input.provider.as_str(), "ollama" | "openai" | "anthropic")
            || (input.provider != "ollama" && !input.hosted_consent)
        {
            return Err(AppError::InvalidRequest(
                "Choose a supported provider and allow hosted prompts when using a lab model."
                    .into(),
            ));
        }
        let key = format!("local-agent-{}", input.request_id);
        // An omitted selection grants no workspace access. Reject unavailable tools
        // rather than silently accepting a definition different from the request.
        let mut requested_tools = input.tools.unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        requested_tools.retain(|tool| tool != "finish" && seen.insert(tool.clone()));
        if requested_tools
            .iter()
            .any(|tool| !profile.tools.contains(tool))
        {
            return Err(AppError::InvalidRequest(
                profile
                    .tool_restriction
                    .unwrap_or_else(|| "A requested tool is not available on this host.".into()),
            ));
        }
        let hosted_workspace = if input.provider != "ollama" && !requested_tools.is_empty() {
            if !input.hosted_tools_consent {
                return Err(AppError::InvalidRequest("Allow selected file results to be sent to this provider, or remove the selected tools.".into()));
            }
            let root = self
                .host
                .settings
                .workspace_root
                .as_ref()
                .ok_or(AppError::WorkspaceUnavailable)?;
            Some(
                tetonic_tools::Workspace::new(root)
                    .map_err(|_| AppError::WorkspaceUnavailable)?
                    .root()
                    .to_str()
                    .ok_or(AppError::WorkspaceUnavailable)?
                    .to_owned(),
            )
        } else {
            None
        };
        let config = serde_json::json!({
            "instructions": if input.purpose.is_empty() { "Help the owner think through their request. Inspect the workspace with available tools and call finish with your complete answer as the summary." } else { &input.purpose },
            "requested_tools": requested_tools, "max_steps": input.max_steps,
            "preferences": GeneralAgentPreferences {
                hosted_workspace,
                provider: (input.provider != "ollama").then_some(input.provider.clone()),
                hosted_consent: input.provider != "ollama" && input.hosted_consent,
                display_name: input.name, model: input.model.clone(),
                max_elapsed_seconds: input.max_seconds, reported_token_ceiling: input.max_tokens,
            }
        });
        let resources = self.local.resources();
        // An identical retry must still work if Ollama has since gone offline.
        // register_agent checks exact definition equality and rejects changed retries.
        if resources
            .get_agent(&self.host.credential, ORG.into(), key.clone())
            .await
            .map_err(resource)?
            .is_none()
        {
            if input.provider == "ollama" {
                self.require_installed_model(&input.model).await?;
            } else if !self.keys.ready(&input.provider).await {
                return Err(AppError::InvalidRequest(
                    "Save this provider's API key before creating the agent.".into(),
                ));
            }
        }
        let stored = resources
            .register_agent(
                &self.host.credential,
                ORG.into(),
                key.clone(),
                "general".into(),
                config,
            )
            .await
            .map_err(resource)?;
        self.agent_profile(key, &stored)
    }

    pub(super) async fn require_installed_model(&self, model: &str) -> Result<(), AppError> {
        if !self
            .installed_models()
            .await?
            .iter()
            .any(|installed| tetonic_inference::ollama_model_matches(installed, model))
        {
            return Err(AppError::InvalidRequest(
                "Choose an installed local model with tool calling support.".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn check_limits(
        &self,
        steps: usize,
        seconds: u64,
        tokens: u64,
    ) -> Result<(), AppError> {
        if steps == 0
            || steps > self.host.settings.limits.max_steps
            || seconds < 10
            || seconds > self.host.settings.max_elapsed_seconds
            || tokens < 256
            || tokens > self.host.settings.reported_token_ceiling.unwrap_or(4096)
        {
            return Err(AppError::InvalidRequest(
                "Agent run limits exceed this local host's allowed range.".into(),
            ));
        }
        Ok(())
    }

    pub(super) async fn registered_agent(
        &self,
        key: &str,
    ) -> Result<tetonic_memory::RegisteredAgent, AppError> {
        if key != AGENT
            && key != shaping::GUIDE
            && key != plan_execution::COORDINATOR
            && key
                .strip_prefix("local-agent-")
                .is_none_or(|id| uuid::Uuid::parse_str(id).is_err())
        {
            return Err(AppError::InvalidRequest("Unknown local agent.".into()));
        }
        self.local
            .resources()
            .get_agent(&self.host.credential, ORG.into(), key.into())
            .await
            .map_err(resource)?
            .ok_or_else(|| AppError::InvalidRequest("Local agent unavailable.".into()))
    }

    pub(super) fn agent_profile(
        &self,
        key: String,
        stored: &tetonic_memory::RegisteredAgent,
    ) -> Result<LocalAgent, AppError> {
        let value: serde_json::Value = serde_json::from_str(&stored.definition_json)
            .map_err(|_| AppError::InvalidRequest("Invalid stored agent definition.".into()))?;
        if value["schema_version"] != 1 || value["harness"] != "general" {
            return Err(AppError::InvalidRequest(
                "This stored agent uses an unsupported definition or harness.".into(),
            ));
        }
        let config = &value["configuration"];
        let prefs: GeneralAgentPreferences = if key == AGENT {
            GeneralAgentPreferences {
                hosted_workspace: None,
                provider: None,
                hosted_consent: false,
                display_name: AGENT.into(),
                model: self.host.settings.model.clone(),
                max_elapsed_seconds: self.host.settings.max_elapsed_seconds,
                reported_token_ceiling: self.host.settings.reported_token_ceiling.unwrap_or(4096),
            }
        } else if let Some(prefs_val) = config.get("preferences").filter(|v| !v.is_null()) {
            serde_json::from_value(prefs_val.clone()).map_err(|_| {
                AppError::InvalidRequest("Invalid stored agent settings. The agent has not been switched to another model.".into())
            })?
        } else {
            GeneralAgentPreferences {
                hosted_workspace: None,
                provider: None,
                hosted_consent: false,
                display_name: key.clone(),
                model: self.host.settings.model.clone(),
                max_elapsed_seconds: self.host.settings.max_elapsed_seconds,
                reported_token_ceiling: self.host.settings.reported_token_ceiling.unwrap_or(4096),
            }
        };
        let tools: Vec<String> = config
            .get("requested_tools")
            .map(|tools| serde_json::from_value(tools.clone()))
            .transpose()
            .map_err(|_| AppError::InvalidRequest("Invalid stored agent tools.".into()))?
            .unwrap_or_default();
        Ok(LocalAgent {
            hosted_workspace: prefs.hosted_workspace,
            plan_coordinator: key == plan_execution::COORDINATOR,
            provider: prefs.provider.unwrap_or_else(local_provider),
            hosted_consent: prefs.hosted_consent,
            key,
            id: stored.identity.identity_id.clone(),
            name: prefs.display_name,
            purpose: config["instructions"].as_str().unwrap_or_default().into(),
            model: prefs.model,
            harness: "general".into(),
            max_steps: config["max_steps"]
                .as_u64()
                .unwrap_or(self.host.settings.limits.max_steps as u64)
                as usize,
            max_seconds: prefs.max_elapsed_seconds,
            max_tokens: prefs.reported_token_ceiling,
            tools,
        })
    }

    pub(super) async fn agents(&self) -> Result<Vec<LocalAgent>, AppError> {
        self.local
            .resources()
            .list_agents(&self.host.credential, ORG.into())
            .await
            .map_err(resource)?
            .into_iter()
            .filter(|(key, _)| {
                key == AGENT
                    || key == shaping::GUIDE
                    || key == plan_execution::COORDINATOR
                    || key.starts_with("local-agent-")
            })
            .map(|(key, stored)| self.agent_profile(key, &stored))
            .collect()
    }
}
