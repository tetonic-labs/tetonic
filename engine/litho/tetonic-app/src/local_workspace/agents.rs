use super::*;
use crate::resources::GeneralAgentPreferences;
use futures::{StreamExt, TryStreamExt};
use serde::Deserialize;

mod configuration;
mod editing;
mod execution;
mod profiles;
pub use editing::UpdateLocalAgent;
pub use profiles::LocalAgentRuntimeProfile;

#[derive(Clone, Serialize)]
pub struct LocalAgent {
    pub definition_digest: String,
    pub editable: bool,
    pub tool_disclosure: Option<crate::resources::ToolDisclosure>,
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
    pub mcp_connections: Vec<crate::mcp::McpConnectionView>,
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
    /// The folder displayed when disclosure was approved, not a requested root.
    #[serde(default)]
    pub expected_workspace_root: Option<String>,
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
        tools.extend(
            self.host
                .settings
                .mcp
                .as_ref()
                .map(|m| m.tool_names())
                .unwrap_or_default(),
        );
        Ok(LocalAgentCatalog {
            mcp_connections: self.mcp_connections(),
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

    pub async fn create_agent(&self, input: CreateLocalAgent) -> Result<LocalAgent, AppError> {
        let config = self.agent_configuration(input.clone())?;
        let key = format!("local-agent-{}", input.request_id);
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
        let default_tokens = if key == shaping::GUIDE {
            LOCAL_TOKEN_CEILING
        } else {
            DEFAULT_WORK_TOKENS
        }
        .min(
            self.host
                .settings
                .reported_token_ceiling
                .unwrap_or(DEFAULT_WORK_TOKENS),
        );
        let prefs: GeneralAgentPreferences = if key == AGENT && config.get("preferences").is_none()
        {
            GeneralAgentPreferences {
                tool_disclosure: None,
                hosted_workspace: None,
                provider: None,
                hosted_consent: false,
                display_name: AGENT.into(),
                model: self.host.settings.model.clone(),
                max_elapsed_seconds: self.host.settings.max_elapsed_seconds,
                reported_token_ceiling: default_tokens,
            }
        } else if let Some(prefs_val) = config.get("preferences").filter(|v| !v.is_null()) {
            serde_json::from_value(prefs_val.clone()).map_err(|_| {
                AppError::InvalidRequest("Invalid stored agent settings. The agent has not been switched to another model.".into())
            })?
        } else {
            GeneralAgentPreferences {
                tool_disclosure: None,
                hosted_workspace: None,
                provider: None,
                hosted_consent: false,
                display_name: key.clone(),
                model: self.host.settings.model.clone(),
                max_elapsed_seconds: self.host.settings.max_elapsed_seconds,
                reported_token_ceiling: default_tokens,
            }
        };
        let tools: Vec<String> = config
            .get("requested_tools")
            .map(|tools| serde_json::from_value(tools.clone()))
            .transpose()
            .map_err(|_| AppError::InvalidRequest("Invalid stored agent tools.".into()))?
            .unwrap_or_default();
        Ok(LocalAgent {
            definition_digest: stored.identity.bound_definition_digest.clone(),
            editable: key != shaping::GUIDE && key != plan_execution::COORDINATOR,
            tool_disclosure: prefs.tool_disclosure,
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
