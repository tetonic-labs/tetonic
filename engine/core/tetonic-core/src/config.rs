use tetonic_domain::{DataClass, DisclosureTier};

/// Agent loop configuration.

#[derive(Clone)]
pub struct AgentConfig {
    pub model: String,
    /// Effort cap: maximum model turns before we stop.
    pub max_steps: usize,
    pub temperature: f32,
    /// Context window requested from the runtime (Ollama `num_ctx`).
    pub num_ctx: usize,
    /// Tokens held back from the budget for the model's reply.
    pub context_reserve: usize,
    /// Fraction of the budget that triggers auto-compaction: when the working
    /// set crosses this, older turns are summarized into a compact brief
    /// (Claude-Code-style "recontextualize") instead of being hard-dropped.
    pub compaction_threshold: f32,
    /// Most recent messages always kept verbatim when compacting.
    pub keep_recent: usize,
    /// Coordinator session id (fabric receipts / turn affinity).
    pub session_id: Option<String>,
    /// Durable run/task/attempt identity for fabric jobs.
    pub run_id: Option<String>,
    pub task_id: Option<String>,
    pub attempt_id: Option<String>,
    /// Authoritative workspace root for version-bound context and dispatch.
    pub workspace_root: Option<std::path::PathBuf>,
    /// Explicit process working directory for process actions (C1).
    pub process_working_directory: Option<std::path::PathBuf>,
    /// Agent node id in the run tree (root `"a0"` today).
    pub agent_id: String,
    /// Session briefing injected after the system prompt on the first turn (D5).
    pub briefing: Option<String>,
    /// Project digest + notes injected after briefing (D4).
    pub project_context: Option<String>,
    /// Data class for fabric/policy routing (D1).
    pub data_class: DataClass,
    /// Disclosure tier for remote fabric jobs (SEC2-E2-029).
    pub disclosure_tier: DisclosureTier,
    /// Specialist role id when spawned by orchestrator (D11).
    pub specialist_role: Option<String>,
    /// Extra system instructions for specialist agents (D11).
    pub system_overlay: Option<String>,
    /// Fabric placement hint: `fast` or `hard` (A9 v5).
    pub model_tier: Option<String>,
    /// Read-only / explain turn: answer in prose, skip verify gate, softer compaction.
    pub explain_turn: bool,
    /// Retries when the model answers in prose instead of calling tools (implementation tasks).
    pub empty_tool_retry_limit: u32,
    /// Consecutive no-progress tool steps before stopping the turn early.
    pub no_progress_limit: u32,
    /// Consecutive empty search results before nudging alternate strategies.
    pub search_miss_streak_limit: u32,
    /// Max mutating writes per path before blocking fragmentation.
    pub write_repeat_limit: u32,
    /// Inherited workspace version for fast dispatch without disk scans (OPT-401).
    pub inherited_workspace_version: Option<tetonic_domain::WorkspaceVersion>,
    /// Speculative draft model for accelerated token decoding (OPT-501).
    pub draft_model: Option<String>,
    /// Number of speculative tokens to generate per forward pass.
    pub draft_count: Option<u32>,
    /// Centralized execution thresholds and resource limits (OPT-703).
    pub limits: EngineLimits,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineLimits {
    pub max_explain_whole_file_bytes: usize,
    pub default_grep_limit: usize,
    pub max_tool_output_bytes: usize,
}

impl Default for EngineLimits {
    fn default() -> Self {
        Self {
            max_explain_whole_file_bytes: 16 * 1024,
            default_grep_limit: 100,
            max_tool_output_bytes: 60_000,
        }
    }
}

fn default_agent_id() -> String {
    "a0".into()
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model: lokai_inference::DEFAULT_MODEL.to_string(),
            // Raised from 12: the no-progress guard in `turn` now stops stuck
            // loops early, so a higher cap helps genuinely multi-step work
            // (multi-file edits, refactors) finish without burning out.
            max_steps: 16,
            temperature: 0.2,
            num_ctx: 8192,
            context_reserve: 1024,
            compaction_threshold: 0.75,
            keep_recent: 4,
            session_id: None,
            run_id: None,
            task_id: None,
            attempt_id: None,
            workspace_root: None,
            process_working_directory: None,
            agent_id: default_agent_id(),
            briefing: None,
            project_context: None,
            data_class: DataClass::RepositorySource,
            disclosure_tier: DisclosureTier::Auditable,
            specialist_role: None,
            system_overlay: None,
            model_tier: None,
            explain_turn: false,
            empty_tool_retry_limit: 2,
            no_progress_limit: 6,
            search_miss_streak_limit: 3,
            write_repeat_limit: 6,
            inherited_workspace_version: None,
            draft_model: None,
            draft_count: None,
            limits: EngineLimits::default(),
        }
    }
}
