//! Specialist roles as pack overlays (M9). Spawn/handoff stay in the orchestrator.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[cfg(test)]
use std::sync::Arc;

use tetonic_tools::Tools;

/// Pack-defined specialist identity. Not an orchestrator enum.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RoleId(pub String);

impl RoleId {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Product pack: role names, overlays, and tool subsets.
pub trait SpecialistPack: Send + Sync {
    fn parse(&self, s: &str) -> Option<RoleId>;
    fn default_role(&self) -> RoleId;
    fn critic_role(&self) -> RoleId;
    fn revision_role(&self) -> RoleId;
    fn overlay(&self, role: &RoleId) -> String;
    fn allowed_tools(&self, role: &RoleId) -> Option<Vec<String>>;
    fn spawn_allowed_tools(&self, role: &RoleId) -> Vec<String>;
    fn should_run_critic(&self, role: &RoleId) -> bool;
    fn max_steps(&self, role: &RoleId, base: usize) -> usize;
    fn explain_turn(&self, role: &RoleId, base: bool) -> bool;
    /// Product compile of Single-door / `role: None` explain mode. Current heuristic, not PRESERVE.
    fn root_explain_turn(&self, user_text: &str) -> bool;

    fn apply_tool_filter(&self, role: &RoleId, tools: Tools) -> Tools {
        match self.allowed_tools(role) {
            None => tools,
            Some(names) => {
                let set: HashSet<String> = names.into_iter().collect();
                tools.with_allowed_tools(set)
            }
        }
    }

    fn apply_spawn_tool_filter(&self, role: &RoleId, tools: Tools) -> Tools {
        let set: HashSet<String> = self.spawn_allowed_tools(role).into_iter().collect();
        tools.with_allowed_tools(set)
    }
}

/// Dynamic generic agent specification for synthesized agents, router specialists, and DAG nodes (OPT-401).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicAgentSpec {
    pub role_name: String,
    pub system_overlay: String,
    pub allowed_tools: Option<Vec<String>>,
    pub node_id: Option<String>,
    pub parent_agent_id: Option<String>,
}

impl DynamicAgentSpec {
    pub fn new(role_name: impl Into<String>, system_overlay: impl Into<String>) -> Self {
        Self {
            role_name: role_name.into(),
            system_overlay: system_overlay.into(),
            allowed_tools: None,
            node_id: None,
            parent_agent_id: None,
        }
    }

    pub fn from_role(pack: &dyn SpecialistPack, role: &RoleId) -> Self {
        Self {
            role_name: role.as_str().to_string(),
            system_overlay: pack.overlay(role),
            allowed_tools: pack.allowed_tools(role),
            node_id: None,
            parent_agent_id: None,
        }
    }

    pub fn with_node_id(mut self, node_id: impl Into<String>) -> Self {
        self.node_id = Some(node_id.into());
        self
    }

    pub fn with_parent_agent_id(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_agent_id = Some(parent_id.into());
        self
    }

    pub fn with_allowed_tools(mut self, tools: Vec<String>) -> Self {
        self.allowed_tools = Some(tools);
        self
    }

    pub fn apply_tool_filter(&self, tools: Tools) -> Tools {
        match &self.allowed_tools {
            None => tools,
            Some(names) => {
                let set: HashSet<String> = names.iter().cloned().collect();
                tools.with_allowed_tools(set)
            }
        }
    }
}

/// Local test pack (string names OK). Production table lives in `lokai-app`.
#[cfg(test)]
#[derive(Debug, Default, Clone, Copy)]
pub struct TestCodingPack;

#[cfg(test)]
impl TestCodingPack {
    pub fn arc() -> Arc<dyn SpecialistPack> {
        Arc::new(TestCodingPack)
    }
}

#[cfg(test)]
impl SpecialistPack for TestCodingPack {
    fn parse(&self, s: &str) -> Option<RoleId> {
        match s.to_ascii_lowercase().as_str() {
            "planner" | "plan" => Some(RoleId::new("planner")),
            "coder" | "code" => Some(RoleId::new("coder")),
            "debugger" | "debug" => Some(RoleId::new("debugger")),
            "reviewer" | "review" => Some(RoleId::new("reviewer")),
            "critic" => Some(RoleId::new("critic")),
            _ => None,
        }
    }

    fn default_role(&self) -> RoleId {
        RoleId::new("coder")
    }

    fn critic_role(&self) -> RoleId {
        RoleId::new("critic")
    }

    fn revision_role(&self) -> RoleId {
        RoleId::new("coder")
    }

    fn overlay(&self, role: &RoleId) -> String {
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

    fn allowed_tools(&self, role: &RoleId) -> Option<Vec<String>> {
        match role.as_str() {
            "coder" => None,
            "debugger" => Some(mutating_subset()),
            "planner" | "reviewer" | "critic" => Some(read_subset()),
            _ => Some(read_subset()),
        }
    }

    fn spawn_allowed_tools(&self, role: &RoleId) -> Vec<String> {
        match role.as_str() {
            "coder" | "debugger" => mutating_subset(),
            _ => read_subset(),
        }
    }

    fn should_run_critic(&self, role: &RoleId) -> bool {
        !matches!(role.as_str(), "reviewer" | "critic" | "planner")
    }

    fn max_steps(&self, role: &RoleId, base: usize) -> usize {
        match role.as_str() {
            "planner" | "reviewer" | "critic" => 8,
            _ => base,
        }
    }

    fn explain_turn(&self, role: &RoleId, base: bool) -> bool {
        match role.as_str() {
            "planner" | "reviewer" | "critic" => true,
            _ => base,
        }
    }

    fn root_explain_turn(&self, user_text: &str) -> bool {
        test_pack_root_explain_turn(user_text)
    }
}

#[cfg(test)]
fn test_pack_root_explain_turn(user_text: &str) -> bool {
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
    !ACTION.iter().any(|kw| lower.contains(kw))
}

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tetonic_tools::{Tools, Workspace};

    #[test]
    fn parses_role_aliases() {
        let pack = TestCodingPack;
        assert_eq!(pack.parse("planner").unwrap().as_str(), "planner");
        assert_eq!(pack.parse("CODE").unwrap().as_str(), "coder");
        assert_eq!(pack.parse("debug").unwrap().as_str(), "debugger");
        assert_eq!(pack.parse("review").unwrap().as_str(), "reviewer");
        assert!(pack.parse("unknown").is_none());
    }

    #[test]
    fn planner_filter_blocks_shell_and_edit() {
        let dir = TempDir::new().unwrap();
        let ws = Workspace::new(dir.path()).unwrap();
        let pack = TestCodingPack;
        let role = pack.parse("planner").unwrap();
        let tools = pack.apply_tool_filter(&role, Tools::new(ws, true));
        let names: Vec<String> = tools
            .defs()
            .into_iter()
            .map(|d| d.name.to_string())
            .collect();
        assert!(names.iter().any(|n| n == "read_file"));
        assert!(names.iter().any(|n| n == "finish"));
        assert!(!names.iter().any(|n| n == "run_shell"));
        assert!(!names.iter().any(|n| n == "edit_file"));
        let args = serde_json::json!({"command":"echo hi"});
        let out = tools.execute("run_shell", &args);
        assert!(!out.ok);
    }

    #[test]
    fn coder_keeps_full_catalogue() {
        let dir = TempDir::new().unwrap();
        let ws = Workspace::new(dir.path()).unwrap();
        let pack = TestCodingPack;
        let role = pack.default_role();
        let base = Tools::new(ws, true);
        let base_count = base.defs().len();
        let filtered = pack.apply_tool_filter(&role, base);
        assert_eq!(filtered.defs().len(), base_count);
    }

    #[test]
    fn spawned_coder_blocks_shell_and_spawn() {
        let dir = TempDir::new().unwrap();
        let ws = Workspace::new(dir.path()).unwrap();
        let pack = TestCodingPack;
        let role = pack.default_role();
        let tools = pack.apply_spawn_tool_filter(&role, Tools::new(ws, true));
        let names: Vec<String> = tools
            .defs()
            .into_iter()
            .map(|d| d.name.to_string())
            .collect();
        assert!(names.iter().any(|n| n == "edit_file"));
        assert!(!names.iter().any(|n| n == "run_shell"));
        assert!(!names.iter().any(|n| n == "spawn_agent"));
        let args = serde_json::json!({"command":"echo hi"});
        let out = tools.execute("run_shell", &args);
        assert!(!out.ok);
    }
}
