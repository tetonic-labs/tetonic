//! Coding product definition (CODE-01 / SUB-01).
//!
//! Owns the production pack compiler (roles, overlays, tool subsets, per-role
//! `explain_turn` / `max_steps` / `should_run_critic`), live instruction
//! rendering, root-door `explain_turn`, and empty-tool nudge compile. This is
//! not an `AgentIdentity` store and not the kernel loop.
//!
//! Coding product definition (CODE-01 / SUB-01).
//!
//! Owns the production pack compiler (roles, overlays, tool subsets, per-role
//! `explain_turn` / `max_steps` / `should_run_critic`), live instruction
//! rendering, root-door `explain_turn`, and empty-tool nudge compile. This is
//! not an `AgentIdentity` store and not the kernel loop.
//!
//! Kernel prompt/task overlays are deleted (`DEL-V4-001`, `DEL-V4-002`).
//! Verify/commit policy is the dated app completion coordinator
//! (`DEL-V4-016`, WORK-FIN-01 CONVERGE). Root explain and empty-tool phrases
//! are current product heuristics, not PRESERVE destination law.

use std::path::Path;

use lokai_core::AgentConfig;
use lokai_domain::{
    AgentIdentity, AgentInvocation, AgentJobSpec, IdentityId, LoopDiscipline, LoopDisciplineLimits,
    LoopNotes,
};
use lokai_orchestrator::RoleId;
use lokai_run::job_input_digest;
use lokai_tools::Tools;

pub const CODING_IDENTITY_ID: &str = "id_coding_production";
const PRODUCTION_VERSION: &str = "code-01-pack";
const PRODUCTION_ROLES: &[&str] = &["planner", "coder", "debugger", "reviewer", "critic"];
const DEFAULT_PRIVILEGE_CLASS: &str = "default";

/// Versioned coding pack recipe. Compiles to `AgentConfig` role fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodingAgentDefinition {
    version: &'static str,
}

impl CodingAgentDefinition {
    pub fn production() -> Self {
        Self {
            version: PRODUCTION_VERSION,
        }
    }

    pub fn version(&self) -> &'static str {
        self.version
    }

    pub fn parse(&self, s: &str) -> Option<RoleId> {
        match s.to_ascii_lowercase().as_str() {
            "planner" | "plan" => Some(RoleId::new("planner")),
            "coder" | "code" => Some(RoleId::new("coder")),
            "debugger" | "debug" => Some(RoleId::new("debugger")),
            "reviewer" | "review" => Some(RoleId::new("reviewer")),
            "critic" => Some(RoleId::new("critic")),
            _ => None,
        }
    }

    pub fn default_role(&self) -> RoleId {
        RoleId::new("coder")
    }

    pub fn critic_role(&self) -> RoleId {
        RoleId::new("critic")
    }

    pub fn revision_role(&self) -> RoleId {
        RoleId::new("coder")
    }

    pub fn overlay(&self, role: &RoleId) -> String {
        match role.as_str() {
            "planner" => "You are the **planner** specialist. Investigate the codebase with read-only \
tools (search_code, read_file, grep, etc.). For questions, explanations, or architecture inquiries: \
read the relevant code first, then provide a thorough, comprehensive answer. For planning, design, \
or refactoring tasks: produce a structured step-by-step plan. Do NOT edit files — call `finish` \
with your complete answer or plan in `summary`."
                .into(),
            "coder" => "You are the **coder** specialist. Implement the requested change with precise \
edits. For Python: produce syntactically complete modules (py_compile must pass). \
For parsers: recursive descent only — never eval/exec/ast; avoid fragile regex. \
Prefer one complete write_file over many partial overwrites to the same path."
                .into(),
            "debugger" => "You are the **debugger** specialist. Reproduce the failure, read relevant \
code, form a hypothesis, then apply a minimal fix. Explain root cause briefly in `finish`."
                .into(),
            "reviewer" | "critic" => "You are the **reviewer/critic** specialist. Read the changed \
files and recent context. Do NOT edit files. Call `finish` with either `APPROVE` or `REVISE: <issues>`."
                .into(),
            _ => format!("You are the **{}** specialist.", role.as_str()),
        }
    }

    pub fn allowed_tools(&self, role: &RoleId) -> Option<Vec<String>> {
        match role.as_str() {
            "coder" => None,
            "debugger" => Some(mutating_subset()),
            "planner" | "reviewer" | "critic" => Some(read_subset()),
            _ => Some(read_subset()),
        }
    }

    pub fn spawn_allowed_tools(&self, role: &RoleId) -> Vec<String> {
        match role.as_str() {
            "coder" | "debugger" => mutating_subset(),
            _ => read_subset(),
        }
    }

    pub fn should_run_critic(&self, role: &RoleId) -> bool {
        !matches!(role.as_str(), "reviewer" | "critic" | "planner")
    }

    pub fn max_steps(&self, role: &RoleId, base: usize) -> usize {
        match role.as_str() {
            "planner" | "reviewer" | "critic" => 8,
            _ => base,
        }
    }

    pub fn explain_turn(&self, role: &RoleId, base: bool) -> bool {
        match role.as_str() {
            "planner" | "reviewer" | "critic" => true,
            _ => base,
        }
    }

    /// Single-door / `role: None` explain compile. Current heuristic, not PRESERVE.
    pub fn root_explain_turn(&self, user_text: &str) -> bool {
        compile_root_explain_turn(user_text)
    }

    /// ACTION-phrase empty-tool eligibility. Caller must force `false` when
    /// `explain_turn` is already true. Current heuristic, not PRESERVE.
    /// Must not be compiled as `!explain_turn` alone.
    pub fn empty_tool_nudge(&self, user_text: &str) -> bool {
        looks_like_action(user_text)
    }

    /// Render live loop instructions from host card, catalog, briefing, overlay.
    pub fn render_instructions(
        &self,
        operator_card: &str,
        catalog_prompt_lines: &str,
        workspace_root: &Path,
        briefing: Option<&str>,
        project_context: Option<&str>,
        overlay: Option<&str>,
    ) -> String {
        let root = workspace_root.display();
        let mut out = format!(
            "{operator_card}\n\n\
You are operating inside the user's workspace at:\n  {root}\n\n\
You accomplish tasks by calling tools. Rules:\n\
- Investigate before editing: use read_file, list_dir, grep, and glob to gather context.\n\
{catalog_prompt_lines}\
- Make precise edits with edit_file (the old_string must appear EXACTLY ONCE). Use write_file for new files or to replace a whole file in one shot — do not overwrite the same file many times with fragments.\n\
- After an edit, re-read the changed region to confirm it is complete and correct: no leftover stubs (TODO/NotImplementedError), no references to names you never defined, balanced brackets/quotes.\n\
- run_shell requires user approval and may be unavailable; prefer the file tools.\n\
- Locate code by searching, not scanning: prefer find_definition/search_code/outline (or grep) and read only the specific region you need (start_line/end_line). Do not re-read whole files you have already read this session — their contents are still above.\n\
- Never invent file contents — read first.\n\
- Before calling `finish`, double-check your change actually solves the task and handles the obvious edge cases (empty input, zero, negatives, boundaries). Do not declare done over code you have not validated.\n\
- When the task is complete, call the `finish` tool. For questions, explanations, or plans, provide your complete, detailed answer in summary; for code edits, summarize the changes. Do not keep going after finishing.\n\
- If a tool returns an ERROR (or a failed verification), read it carefully and correct your next call instead of repeating the same one.\n\
Be concise."
        );
        if let Some(brief) = briefing.filter(|s| !s.is_empty()) {
            out.push_str("\n\n");
            out.push_str(brief);
        }
        if let Some(ctx) = project_context.filter(|s| !s.is_empty()) {
            out.push_str("\n\n## Project memory\n");
            out.push_str(ctx);
        }
        if let Some(overlay) = overlay.filter(|s| !s.is_empty()) {
            out.push_str("\n\n");
            out.push_str(overlay);
        }
        out
    }

    pub fn compile_invocation(
        &self,
        operator_card: &str,
        catalog_prompt_lines: &str,
        workspace_root: &Path,
        config: &AgentConfig,
        user_input: &str,
    ) -> AgentInvocation {
        let explain_turn = config.explain_turn;
        let empty_tool_nudge = if explain_turn {
            false
        } else {
            self.empty_tool_nudge(user_input)
        };
        AgentInvocation {
            instructions: self.render_instructions(
                operator_card,
                catalog_prompt_lines,
                workspace_root,
                config.briefing.as_deref(),
                config.project_context.as_deref(),
                config.system_overlay.as_deref(),
            ),
            user_input: user_input.to_string(),
            explain_turn,
            empty_tool_nudge,
            max_steps: config.max_steps,
            completion_tool: "finish".into(),
            discipline: coding_loop_discipline(explain_turn, empty_tool_nudge),
        }
    }

    /// Production compile from capability `Tools` inherent APIs (not `dyn ToolHost`).
    pub fn compile_invocation_from_tools(
        &self,
        tools: &Tools,
        config: &AgentConfig,
        user_input: &str,
    ) -> AgentInvocation {
        self.compile_invocation(
            &lokai_tools::operator_card(tools.has_index(), tools.has_memory(), tools.has_lsp()),
            &lokai_tools::catalog_prompt_lines(
                tools.has_index(),
                tools.has_memory(),
                tools.has_lsp(),
            ),
            tools.workspace().root(),
            config,
            user_input,
        )
    }

    pub fn apply_role(&self, base: &AgentConfig, role: &RoleId, agent_id: &str) -> AgentConfig {
        AgentConfig {
            agent_id: agent_id.to_string(),
            specialist_role: Some(role.as_str().to_string()),
            system_overlay: Some(self.overlay(role)),
            max_steps: self.max_steps(role, base.max_steps),
            explain_turn: self.explain_turn(role, base.explain_turn),
            ..base.clone()
        }
    }

    pub fn identity_policy(&self) -> CodingIdentityPolicy {
        CodingIdentityPolicy::from_definition(self)
    }

    /// Digest of `version()` plus the canonical identity policy. Not a persist API.
    pub fn definition_digest(&self) -> String {
        let policy = self.identity_policy();
        let mut canon = format!(
            "version={}\nprivilege={}\n",
            self.version(),
            policy.privilege_class()
        );
        for actor in policy.actors() {
            canon.push_str(&format!("actor={actor}\n"));
        }
        for ts in policy.toolsets() {
            let allowed = match &ts.allowed_tools {
                Some(tools) => tools.join(","),
                None => "*".into(),
            };
            canon.push_str(&format!(
                "toolset={} allowed={} spawn={}\n",
                ts.actor,
                allowed,
                ts.spawn_allowed_tools.join(",")
            ));
        }
        for binding in policy.bindings() {
            canon.push_str(&format!("binding={}\n", binding.as_str()));
        }
        job_input_digest(&canon)
    }

    /// Product compile of the standing coding identity. Persist stays on `lokai-run`.
    pub fn coding_identity_record(&self) -> AgentIdentity {
        let policy = self.identity_policy();
        AgentIdentity {
            id: IdentityId::new(CODING_IDENTITY_ID),
            owning_application: "coding".into(),
            bound_definition_digest: self.definition_digest(),
            privilege_class: policy.privilege_class().to_string(),
            toolset_subscriptions: policy.actors().to_vec(),
            context_bindings: policy
                .bindings()
                .iter()
                .map(|binding| binding.as_str().to_string())
                .collect(),
            recovery_id: CODING_IDENTITY_ID.into(),
        }
    }
}

/// In-memory coding identity *policy*. Not Session, not `AgentIdentity`, not a store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingIdentityPolicy {
    actors: Vec<String>,
    toolsets: Vec<ActorToolset>,
    bindings: Vec<ContextBinding>,
    privilege_class: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorToolset {
    pub actor: String,
    pub allowed_tools: Option<Vec<String>>,
    pub spawn_allowed_tools: Vec<String>,
}

/// Named context bindings. `Memory` is session-filtered today; Session is not the identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextBinding {
    Briefing,
    ProjectContext,
    Memory,
}

impl CodingIdentityPolicy {
    pub fn from_definition(def: &CodingAgentDefinition) -> Self {
        let actors = PRODUCTION_ROLES
            .iter()
            .map(|role| (*role).to_string())
            .collect::<Vec<_>>();
        let toolsets = actors
            .iter()
            .map(|actor| {
                let role = RoleId::new(actor.clone());
                ActorToolset {
                    actor: actor.clone(),
                    allowed_tools: def.allowed_tools(&role),
                    spawn_allowed_tools: def.spawn_allowed_tools(&role),
                }
            })
            .collect();
        Self {
            actors,
            toolsets,
            bindings: vec![
                ContextBinding::Briefing,
                ContextBinding::ProjectContext,
                ContextBinding::Memory,
            ],
            privilege_class: DEFAULT_PRIVILEGE_CLASS,
        }
    }

    pub fn actors(&self) -> &[String] {
        &self.actors
    }

    pub fn toolsets(&self) -> &[ActorToolset] {
        &self.toolsets
    }

    pub fn bindings(&self) -> &[ContextBinding] {
        &self.bindings
    }

    pub fn privilege_class(&self) -> &'static str {
        self.privilege_class
    }
}

impl ContextBinding {
    pub fn as_str(self) -> &'static str {
        match self {
            ContextBinding::Briefing => "briefing",
            ContextBinding::ProjectContext => "project_context",
            ContextBinding::Memory => "memory",
        }
    }
}

fn mutating_subset() -> Vec<String> {
    [
        "read_file",
        "list_dir",
        "grep",
        "glob",
        "edit_file",
        "write_file",
        "find_definition",
        "search_code",
        "outline",
        "find_mentions",
        "find_references",
        "lsp_goto_definition",
        "lsp_find_references",
        "lsp_diagnostics",
        "finish",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn read_subset() -> Vec<String> {
    [
        "read_file",
        "list_dir",
        "grep",
        "glob",
        "find_definition",
        "search_code",
        "outline",
        "find_mentions",
        "find_references",
        "lsp_goto_definition",
        "lsp_find_references",
        "lsp_diagnostics",
        "finish",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

/// Current coding-product loop table. Heuristic, not PRESERVE.
pub fn coding_loop_discipline(explain_turn: bool, empty_tool_nudge: bool) -> LoopDiscipline {
    LoopDiscipline {
        finish_min_chars: explain_turn.then_some(20),
        empty_tool_nudge_text: empty_tool_nudge.then(|| {
            "You must use tools to complete this task — do not reply with plain \
text only. Read the relevant file(s), make the required changes, then call \
the completion tool when done."
                .to_string()
        }),
        spawn_tool: Some("spawn_agent".into()),
        expand_tool: Some("expand_context".into()),
        whole_file_tools: vec!["read_file".into()],
        search_tools: vec!["search_code".into()],
        compaction_system_prompt: Some(
            "You compress a coding agent's session log. Produce a terse brief capturing: \
files read or edited, concrete changes made, key facts learned about the codebase, and \
any remaining TODOs or open questions. Use short bullet points. No preamble, no fluff."
                .into(),
        ),
        notes: LoopNotes {
            finish_too_short: Some(
                "ERROR: completion requires a non-empty `summary` with your \
answer (at least a few sentences). Do not call the completion tool with empty arguments."
                    .into(),
            ),
            mutate_feedback: Some(
                "ERROR: `{name}` is not allowed on read-only / explain turns. \
Answer from what you have read, or call the completion tool with your answer — \
do not modify the workspace or run shell commands."
                    .into(),
            ),
            reread_feedback: Some(
                "ERROR: you already read `{path}` in full this turn. Use the content \
above or a sliced read — do not re-read the whole file."
                    .into(),
            ),
            size_feedback: Some(
                "ERROR: `{path}` is {size_kb} KB — too large for a whole-file read on explain turns. \
Use a sliced read."
                    .into(),
            ),
            retrieval_nudge: Some(
                "\n\n[note] You already read this file in full earlier this \
session — its contents are still in the conversation above. Avoid re-reading whole files."
                    .into(),
            ),
            search_miss_nudge: Some(
                "\n\n[note] Keyword search is returning nothing — the index may be thin or \
your query too narrow. Try another search tool or a known path, then call the completion tool \
with what you know."
                    .into(),
            ),
            write_repeat_feedback: Some(
                "ERROR: you have overwritten `{path}` {n} times. Stop fragmenting. \
Read the current file, then write ONE complete valid file (or apply a small targeted edit)."
                    .into(),
            ),
            explain_cap_nudge: Some(
                "Summarize what you have learned so far in plain text, then call \
the completion tool with your answer."
                    .into(),
            ),
            explain_last_nudge: Some(
                "Step budget almost exhausted. Call the completion tool now \
with your answer — do not read more files."
                    .into(),
            ),
            spawn_disabled: Some("ERROR: {name} is not enabled for this agent.".into()),
            expand_failed: Some("ERROR: {name} failed: {error}".into()),
            expand_disabled: Some("ERROR: {name} is not enabled for this agent.".into()),
        },
        limits: LoopDisciplineLimits {
            max_whole_file_bytes: Some(16 * 1024),
            search_miss_streak: Some(3),
            write_repeat: Some(6),
        },
    }
}

fn looks_like_action(user_text: &str) -> bool {
    let lower = user_text.to_ascii_lowercase();
    const ACTION: &[&str] = &[
        "implement",
        "fix",
        "edit",
        "refactor",
        "create",
        "write",
        "add ",
        "change",
        "update",
        "replace",
        "build ",
        "correct",
        "bug",
        "todo",
        "notimplemented",
    ];
    ACTION.iter().any(|kw| lower.contains(kw))
}

fn compile_root_explain_turn(user_text: &str) -> bool {
    let lower = user_text.to_ascii_lowercase();
    const EXPLAIN: &[&str] = &[
        "explain ",
        "what does ",
        "what is ",
        "what are ",
        "how does ",
        "how do ",
        "describe ",
        "walk me through",
        "tell me about",
        "tell me how",
        "tell me a ",
        "summary of",
        "summary about",
        "brief summary",
        "summarize ",
        "read only",
        "read-only",
        "do not edit",
        "don't edit",
        "dont edit",
        "without editing",
        "no edits",
        "just tell me",
        "help me understand",
        "i don't understand",
        "i dont understand",
        "i do not understand",
        "don't really understand",
        "dont really understand",
    ];
    const READ_ONLY: &[&str] = &[
        "read only",
        "read-only",
        "do not edit",
        "don't edit",
        "dont edit",
        "without editing",
        "no edits",
        "not edit",
    ];
    if READ_ONLY.iter().any(|k| lower.contains(k)) {
        return true;
    }
    let explainish = EXPLAIN.iter().any(|k| lower.contains(k))
        || (lower.contains("tell me") && lower.contains("about"));
    if !explainish {
        return false;
    }
    !looks_like_action(user_text)
}

pub fn coding_identity_and_job_spec(job_input: &str) -> (AgentIdentity, AgentJobSpec) {
    let identity = CodingAgentDefinition::production().coding_identity_record();
    let spec = AgentJobSpec {
        identity_id: identity.id.clone(),
        definition_digest: identity.bound_definition_digest.clone(),
        input_digest: job_input_digest(job_input),
        capability_bindings: Vec::new(),
        artifact_bindings: Vec::new(),
        recovery_id: identity.recovery_id.clone(),
    };
    (identity, spec)
}

/// Coding policy is compiled and supplied by the product, never constructed by manager.
pub(crate) fn validate_coding_execution(
    identity: Option<&lokai_domain::AgentIdentity>,
    spec: &lokai_domain::AgentJobSpec,
    role: Option<&str>,
    advertised: &[String],
    max_steps: usize,
) -> Result<(), String> {
    let role_str = role.unwrap_or("coder");
    if let Some(identity) = identity {
        if !identity.toolset_subscriptions.iter().any(|s| s == role_str) {
            return Err(format!(
                "role '{role_str}' is not granted in identity toolset subscriptions"
            ));
        }
    }
    let def = CodingAgentDefinition::production();
    if spec.definition_digest == def.definition_digest() {
        let role_id = RoleId::new(role_str);
        if let Some(allowed) = def.allowed_tools(&role_id) {
            for tool in advertised {
                if !allowed.contains(tool) {
                    return Err(format!("overprivileged tool host: tool '{tool}' not permitted for role '{role_str}'"));
                }
            }
        }
        let limit = def.max_steps(&role_id, max_steps);
        if max_steps > limit {
            return Err(format!("invocation max_steps {max_steps} exceeds allowed limit {limit} for role '{role_str}'"));
        }
    }
    Ok(())
}
