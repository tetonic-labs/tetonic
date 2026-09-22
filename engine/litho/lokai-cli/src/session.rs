//! CLI session config and turn context. Product builds the execution host.

use std::sync::Arc;

use lokai_app::Application;

use crate::app_kernel::TerminalApprovalCoordinator;

/// Normalized session configuration.
#[derive(Clone)]
#[allow(dead_code)]
pub struct CliSessionConfig {
    pub workspace_root: String,
    pub model_fast: String,
    pub model_hard: String,
    pub ollama_base: String,
}

#[derive(Clone)]
pub struct CliTurnContext {
    pub app: Arc<Application>,
    pub config: CliSessionConfig,
    pub session_id: String,
    pub llm_router: bool,
    pub approval_coordinator: Arc<TerminalApprovalCoordinator>,
}
