//! Clap CLI surface for `lokai`.

use crate::help;
use clap::Parser;
#[derive(Parser, Debug)]
#[command(
    name = "lokai",
    about = "Local-only agentic coding assistant (Phase A)",
    long_about = None,
    after_help = help::CLI_EXAMPLES
)]
pub struct Args {
    /// The task for the agent (the rest of the command line). If omitted (and
    /// not viewing history), starts an interactive multi-turn chat.
    #[arg(trailing_var_arg = true)]
    pub(crate) prompt: Vec<String>,

    /// Start an interactive multi-turn chat (one persisted session). This is the
    /// default when no task is given.
    #[arg(long, default_value_t = false)]
    pub(crate) chat: bool,

    /// List recent persisted sessions and exit (read-only; no model needed).
    #[arg(long, default_value_t = false)]
    pub(crate) sessions: bool,

    /// Show the transcript of a specific session id and exit (read-only).
    #[arg(long)]
    pub(crate) session: Option<String>,

    /// Create a named checkpoint at the workspace's current position, so you can
    /// jump back here later with `--restore <label>`. No model needed.
    #[arg(long, value_name = "LABEL")]
    pub(crate) checkpoint: Option<String>,

    /// List the workspace's checkpoints and current position, then exit.
    #[arg(long, default_value_t = false)]
    pub(crate) checkpoints: bool,

    /// Restore the workspace to a checkpoint (by id or label), replaying or
    /// reverting file changes as needed. Pairs with `--dry-run`.
    #[arg(long, value_name = "REF")]
    pub(crate) restore: Option<String>,

    /// Step the workspace back to its previous checkpoint / session boundary,
    /// restoring files to that point. Repeatable. Pairs with `--dry-run`.
    #[arg(long, default_value_t = false)]
    pub(crate) undo: bool,

    /// Re-apply the changes most recently undone (move forward to the pre-undo
    /// position). Pairs with `--dry-run`.
    #[arg(long, default_value_t = false)]
    pub(crate) redo: bool,

    /// With time-travel commands: print what would change without touching files.
    #[arg(long, default_value_t = false)]
    pub(crate) dry_run: bool,

    /// (Re)build the local code index for the workspace, then exit. Incremental:
    /// only changed files are re-parsed. No model needed.
    #[arg(long, default_value_t = false)]
    pub(crate) index: bool,

    /// Print index coverage for the workspace, then exit.
    #[arg(long, default_value_t = false)]
    pub(crate) index_status: bool,

    /// Find where a symbol is defined (structural; free), then exit.
    #[arg(long, value_name = "NAME")]
    pub(crate) def: Option<String>,

    /// Find best-effort references to a symbol (keyword), then exit.
    #[arg(long, value_name = "NAME")]
    pub(crate) refs: Option<String>,

    /// Print the symbol outline of a file (workspace-relative path), then exit.
    #[arg(long, value_name = "PATH")]
    pub(crate) outline: Option<String>,

    /// Keyword search (FTS5/BM25) across the indexed workspace, then exit.
    #[arg(long, value_name = "QUERY")]
    pub(crate) search: Option<String>,

    /// After (re)building the index, embed its chunks for semantic search via the
    /// local runtime. Embeddings stay on this machine (egress-guarded). Then exit.
    #[arg(long, default_value_t = false)]
    pub(crate) embed: bool,

    /// Embedding model served by the local runtime (for --embed and --semantic).
    #[arg(long, default_value = "nomic-embed-text")]
    pub(crate) embed_model: String,

    /// Make --search a semantic (vector) search instead of keyword. Requires the
    /// workspace to have been embedded (`lokai --index --embed`).
    #[arg(long, default_value_t = false)]
    pub(crate) semantic: bool,

    /// Prune index.db of workspaces whose directory no longer exists, then exit.
    /// The index is disposable, so this is always safe to run.
    #[arg(long, default_value_t = false)]
    pub(crate) gc_index: bool,

    /// Project directory the agent operates in.
    #[arg(long, default_value = ".")]
    pub(crate) workspace: String,

    /// Model to use (must support tool calling). Explicit values override saved profiles.
    #[arg(long)]
    pub(crate) model: Option<String>,

    /// Model for the hard tier, selected explicitly or by automatic routing.
    /// Defaults to an explicit --model, otherwise the saved hard-tier default.
    #[arg(long, value_name = "NAME")]
    pub(crate) model_hard: Option<String>,

    /// Small draft companion model for accelerated speculative decoding (e.g. qwen2.5-coder:1.5b).
    #[arg(long, value_name = "NAME")]
    pub(crate) draft_model: Option<String>,

    /// Number of speculative tokens to generate per forward pass (defaults to 3).
    #[arg(long, value_name = "COUNT")]
    pub(crate) draft_count: Option<u32>,

    /// Capability tier for this run: `fast` (default, uses --model) or `hard`
    /// (uses --model-hard for tougher, multi-file/refactor work).
    #[arg(long, value_name = "TIER", default_value = "fast")]
    pub(crate) model_tier: String,

    /// Ollama base URL (loopback only by default).
    #[arg(long, default_value = "http://localhost:11434")]
    pub(crate) ollama: String,

    /// Anthropic API key for hosted Claude models (or set ANTHROPIC_API_KEY).
    #[arg(long)]
    pub(crate) anthropic_key: Option<String>,

    /// OpenAI API key for hosted OpenAI models (or set OPENAI_API_KEY).
    #[arg(long)]
    pub(crate) openai_key: Option<String>,

    /// Custom hosted inference endpoint URL (e.g. corporate proxy or private VPC gateway).
    #[arg(long)]
    pub(crate) endpoint: Option<String>,

    /// Maximum agent turns (effort cap). A no-progress guard stops stuck loops
    /// early, so this cap mainly bounds genuinely multi-step work.
    #[arg(long, default_value_t = 16)]
    pub(crate) max_steps: usize,

    /// Context window to request from the runtime (tokens).
    #[arg(long, default_value_t = 8192)]
    pub(crate) num_ctx: usize,

    /// Path to a local HuggingFace `tokenizer.json` for exact token counts.
    /// If omitted, a heuristic estimator is used (context report shows `~`).
    #[arg(long)]
    pub(crate) tokenizer: Option<String>,

    /// Allow run_shell commands without prompting (default: shell is denied).
    /// Requires `--i-understand-unapproved-shell` (SEC-014).
    #[arg(long, default_value_t = false)]
    pub(crate) allow_shell: bool,

    /// Acknowledge that `--allow-shell` runs model-directed commands without approval.
    #[arg(long, default_value_t = false, hide = true)]
    pub(crate) i_understand_unapproved_shell: bool,

    /// Verify-before-finish command, run in the workspace when the agent calls
    /// finish (e.g. "pytest -q" or "cargo check"). Use `auto` to detect from the
    /// workspace; omit to auto-detect when confidence is high. Use `none` to disable.
    #[arg(long, value_name = "CMD")]
    pub(crate) verify: Option<String>,

    /// Disable verify-before-finish even when the workspace has a detectable test runner.
    #[arg(long, default_value_t = false)]
    pub(crate) no_verify: bool,

    /// Print project memory status for the workspace, then exit (D4).
    #[arg(long, default_value_t = false)]
    pub(crate) project_status: bool,

    /// Append a note to project memory, then exit (D4).
    #[arg(long, value_name = "TEXT")]
    pub(crate) project_note: Option<String>,

    /// Watch the workspace and incrementally re-index on file changes (D6).
    #[arg(long, default_value_t = false)]
    pub(crate) watch_index: bool,

    /// Orchestration mode: `single` (default root agent) or `auto` (keyword router + specialists).
    #[arg(long, value_name = "MODE", default_value = "single")]
    pub(crate) orchestrate: String,

    /// Disable the post-edit critic pass (only applies with `--orchestrate auto`).
    #[arg(long, default_value_t = false)]
    pub(crate) no_critic: bool,

    /// Use one-line LLM router before keyword fallback (orchestration auto).
    #[arg(long, default_value_t = false)]
    pub(crate) llm_router: bool,

    /// Force read-only / explain mode: no edits, shell, or verify gate (overrides heuristics).
    #[arg(long, default_value_t = false)]
    pub(crate) explain: bool,

    /// Enable verbose logging and show detailed error traces in the UI.
    #[arg(long, short = 'd', default_value_t = false)]
    pub(crate) debug: bool,
}
