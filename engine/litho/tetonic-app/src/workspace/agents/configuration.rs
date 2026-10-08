use super::*;

impl WorkspaceServices {
    pub(crate) fn agent_configuration(
        &self,
        mut input: CreateLocalAgent,
    ) -> Result<serde_json::Value, AppError> {
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
        if !matches!(
            input.provider.as_str(),
            "ollama" | "openai" | "anthropic" | "google"
        ) || (input.provider != "ollama" && !input.hosted_consent)
        {
            return Err(AppError::InvalidRequest(
                "Choose a supported provider and allow hosted prompts when using a lab model."
                    .into(),
            ));
        }
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
        if input.provider != "ollama" && !requested_tools.is_empty() && !input.hosted_tools_consent
        {
            return Err(AppError::InvalidRequest("Allow selected tool inputs and results to be sent to this provider, or remove the selected tools.".into()));
        }
        let hosted_workspace = if input.provider != "ollama"
            && crate::resources::uses_workspace(&requested_tools)
        {
            let root = self
                .host
                .settings
                .workspace_root
                .as_ref()
                .ok_or(AppError::WorkspaceUnavailable)?;
            let approved_root = tetonic_tools::Workspace::new(root)
                .map_err(|_| AppError::WorkspaceUnavailable)?
                .root()
                .to_str()
                .ok_or(AppError::WorkspaceUnavailable)?
                .to_owned();
            if input.expected_workspace_root.as_deref() != Some(approved_root.as_str()) {
                return Err(AppError::InvalidRequest("The configured folder changed or its approval is missing. Refresh agent setup and approve the displayed folder.".into()));
            }
            Some(approved_root)
        } else {
            None
        };
        if input.provider != "ollama" {
            providers::inference_endpoint(&input.provider, &input.model)?;
        }
        let tool_disclosure = if input.provider != "ollama" && !requested_tools.is_empty() {
            let mut selected = requested_tools.clone();
            selected.sort();
            Some(crate::resources::ToolDisclosure {
                version: 1,
                provider: input.provider.clone(),
                endpoint: providers::inference_endpoint(&input.provider, &input.model)?,
                tools: selected,
                workspace: hosted_workspace.clone(),
            })
        } else {
            None
        };
        let config = serde_json::json!({
            "instructions": if input.purpose.is_empty() { "Help the owner think through their request. Inspect the workspace with available tools and call finish with your complete answer as the summary." } else { &input.purpose },
            "requested_tools": requested_tools, "max_steps": input.max_steps,
            "preferences": GeneralAgentPreferences {
                tool_disclosure,
                hosted_workspace,
                provider: (input.provider != "ollama").then_some(input.provider.clone()),
                hosted_consent: input.provider != "ollama" && input.hosted_consent,
                display_name: input.name, model: input.model.clone(),
                max_elapsed_seconds: input.max_seconds, reported_token_ceiling: input.max_tokens,
            }
        });
        Ok(config)
    }
}
