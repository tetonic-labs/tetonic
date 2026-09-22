//! Shared production agent-config wiring for daemon and CLI (AC2-1 parity).

use std::path::Path;

use tetonic_core::AgentConfig;
use tetonic_domain::{DataClass, DisclosureTier};

/// Fields shared by daemon and CLI when building [`AgentConfig`].
pub struct AgentConfigInput {
    pub model: String,
    pub agent_id: String,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub task_id: Option<String>,
    pub attempt_id: Option<String>,
    pub workspace_root: Option<std::path::PathBuf>,
    pub verify_cmd: Option<String>,
    pub briefing: Option<String>,
    pub project_context: Option<String>,
    pub model_tier: Option<String>,
    pub explain_turn: bool,
    pub num_ctx: usize,
    pub max_steps: usize,
    pub context_reserve: usize,
    pub data_class: DataClass,
    pub disclosure_tier: DisclosureTier,
}

/// Construct a base agent config before specialist/orchestrator overrides.
pub fn base_agent_config(input: AgentConfigInput) -> AgentConfig {
    AgentConfig {
        model: input.model,
        max_steps: input.max_steps,
        num_ctx: input.num_ctx,
        context_reserve: input.context_reserve,
        session_id: input.session_id,
        run_id: input.run_id,
        task_id: input.task_id,
        attempt_id: input.attempt_id,
        workspace_root: input.workspace_root,
        agent_id: input.agent_id,
        briefing: input.briefing,
        project_context: input.project_context,
        model_tier: input.model_tier,
        explain_turn: input.explain_turn,
        data_class: input.data_class,
        disclosure_tier: input.disclosure_tier,
        draft_model: std::env::var("LOKAI_DRAFT_MODEL").ok(),
        ..Default::default()
    }
}

/// Whether LSP subprocess tools should be enabled for a workspace root.
/// Product pack supplies [`LspSessionOpen::available`]; runtime does not name `lokai-lsp`.
pub fn lsp_enabled_for_workspace(_root: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use lokai_tools::{Tools, Workspace};
    use tetonic_domain::{DataClass, DisclosureTier};

    use super::*;

    #[test]
    fn daemon_cli_tool_graph_parity() {
        let fixture = tempfile::tempdir().unwrap();
        let ws = Workspace::new(fixture.path()).unwrap();
        let daemon_tools = Tools::new(ws.clone(), true).with_orchestration(true);
        let cli_tools = Tools::new(ws, false).with_orchestration(true);
        assert_eq!(
            daemon_tools.enforcement_level(),
            cli_tools.enforcement_level()
        );
        assert_eq!(
            daemon_tools.enforcement_level(),
            lokai_tools::EnforcementLevel::Sandboxed
        );
    }

    #[test]
    fn shared_agent_config_fields_match() {
        let cfg = base_agent_config(AgentConfigInput {
            model: "mock".into(),
            agent_id: "a0".into(),
            session_id: Some("s1".into()),
            run_id: None,
            task_id: None,
            attempt_id: None,
            workspace_root: None,
            verify_cmd: Some("cargo test".into()),
            briefing: None,
            project_context: None,
            model_tier: Some("fast".into()),
            explain_turn: false,
            num_ctx: 8192,
            max_steps: 32,
            context_reserve: 1024,
            data_class: DataClass::RepositorySource,
            disclosure_tier: DisclosureTier::Auditable,
        });
        assert_eq!(cfg.model, "mock");
        assert_eq!(cfg.session_id.as_deref(), Some("s1"));
        assert_eq!(cfg.num_ctx, 8192);
    }
}
