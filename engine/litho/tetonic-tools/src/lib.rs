//! Coding tools (v1 slice) — see contracts/coding-tools-v1.md.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;

mod catalog;
mod host;
mod orchestration;
mod retrieval;
mod memory;
pub use memory::MemoryCredentialCheck;
mod sink;
mod types;
mod workspace;

pub mod exec;
pub mod lsp;
pub mod mutation;
pub mod process_executor;
pub mod sandbox_bridge;
pub mod verify;
pub mod worktree;

pub use catalog::{
    catalog_prompt_lines, index_tool_defs, memory_tool_defs, operator_card, tool_defs,
};
pub use lsp::{lsp_available_for_workspace, lsp_tool_defs};
pub use mutation::RepositoryMutationService;
pub use orchestration::{orchestration_tool_defs, parse_spawn_agent_args, SpawnAgentArgs};
pub use process_executor::{
    coding_executor, CodingProcessValidator, EnforcementLevel, NonCodingProcessValidator,
    ProcessExecutor, ProcessResourceValidator, ProcessRunResult,
};
pub use sink::{tool_outcome_from_execution, verify_result_from_execution};
pub use types::*;
pub use verify::{
    check_python_syntax, detect_verify_command, format_post_edit_snapshot, resolve_verify_cmd,
    summarize_verify_failure, VerifyConfidence, VerifyDetect,
};
pub use workspace::{
    create_dir_all_nofollow, read_to_string_nofollow, remove_nofollow, write_bytes_nofollow,
    Workspace,
};
pub use worktree::{ensure_session_worktree, remove_session_worktree, session_worktree_enabled};

const DEFAULT_GREP_RESULTS: usize = 100;

fn truncate(s: &str) -> String {
    exec::truncate_output(s)
}

thread_local! {
    static INDEX_CACHE: RefCell<HashMap<PathBuf, Rc<dyn tetonic_domain::CodeIndex>>> =
        RefCell::new(HashMap::new());
}

#[derive(Clone)]
pub struct Tools {
    ws: Workspace,
    allow_shell: bool,
    executor: ProcessExecutor,
    mutation: RepositoryMutationService,
    /// Path to the code-intelligence `index.db`, if retrieval tools are enabled.
    index_db: Option<PathBuf>,
    /// Injected opener (product pack). Required together with `index_db`.
    code_index_open: Option<Arc<dyn tetonic_domain::CodeIndexOpen>>,
    /// Path to `lokai.db` for episodic recall (T8).
    memory_db: Option<PathBuf>,
    /// Trusted host binding; never populated from model tool arguments.
    recall_scope: Option<memory::ScopedMemory>,
    /// Current session id (excluded from recall hits).
    session_id: Option<String>,
    /// Language-server tools (rust-analyzer / pyright subprocess).
    lsp_enabled: bool,
    /// Injected LSP opener (product pack).
    lsp_open: Option<Arc<dyn tetonic_domain::LspSessionOpen>>,
    /// Runtime-owned capability handle. Clones retain the same binding until drained.
    lsp_session: Arc<lsp::SessionSlot>,
    /// When set, only these tool names are advertised and executable (D11 specialists).
    allowed_tools: Option<std::collections::HashSet<String>>,
    /// Root-agent orchestration tools (`spawn_agent`) when swarm mode is on.
    orchestration_enabled: bool,
    /// When set, workspace-touching tools require an issued capability (R6-1).
    capability_consumer: Option<Arc<dyn tetonic_domain::CapabilityConsumer>>,
    /// Repository tools are refused. `finish` and `recall` remain available.
    repository_disabled: bool,
    /// Control-database files. Tool reads and writes must not return their bytes.
    reserved_files: Vec<PathBuf>,
}

pub fn store_sidecar_paths(path: &Path) -> Vec<PathBuf> {
    let text = path.display().to_string();
    ["", "-wal", "-shm"]
        .into_iter()
        .map(|suffix| {
            let candidate = if suffix.is_empty() {
                path.to_path_buf()
            } else {
                PathBuf::from(format!("{text}{suffix}"))
            };
            std::fs::canonicalize(&candidate).unwrap_or(candidate)
        })
        .collect()
}

pub fn path_is_reserved(reserved: &[PathBuf], path: &Path) -> bool {
    let candidate = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    reserved
        .iter()
        .any(|item| paths_match(item, &candidate))
}

fn paths_match(left: &Path, right: &Path) -> bool {
    let normalize = |path: &Path| {
        let text = path.to_string_lossy();
        let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
        text.replace('\\', "/").to_ascii_lowercase()
    };
    normalize(left) == normalize(right)
}

fn command_has_parent_traversal(command: &str) -> bool {
    command
        .split(|c: char| c.is_whitespace() || c == '"' || c == '\'')
        .any(|token| token == ".." || token.starts_with("../") || token.contains("/../"))
}

fn command_escapes_workspace(workspace: &Path, command: &str) -> bool {
    let command = command.replace('\\', "/");
    if command_has_parent_traversal(&command) {
        return true;
    }
    let workspace = {
        let text = workspace.to_string_lossy().replace('\\', "/");
        text.trim_end_matches('/').to_ascii_lowercase()
    };
    command
        .split(|c: char| c.is_whitespace() || c == '"' || c == '\'')
        .any(|token| {
            if token.is_empty() {
                return false;
            }
            if token == "~" || token.starts_with("~/") {
                return true;
            }
            let token = token
                .strip_prefix("//?/")
                .unwrap_or(token)
                .to_ascii_lowercase();
            if !is_absolute_command_token(&token) {
                return false;
            }
            let token = token.trim_end_matches('/');
            token != workspace && !token.starts_with(&format!("{workspace}/"))
        })
}

fn command_tokens(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .map(|token| {
            let token = token.trim_matches(|c| c == '"' || c == '\'');
            let token = token.rsplit(['/', '\\']).next().unwrap_or(token);
            token.to_ascii_lowercase()
        })
        .collect()
}

fn command_runs_inline_code(command: &str) -> bool {
    let tokens = command_tokens(command);
    let interpreter = tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "python"
                | "python.exe"
                | "python3"
                | "python3.exe"
                | "node"
                | "node.exe"
                | "ruby"
                | "perl"
                | "deno"
                | "deno.exe"
                | "powershell"
                | "powershell.exe"
                | "pwsh"
                | "pwsh.exe"
                | "bash"
                | "bash.exe"
                | "sh"
                | "sh.exe"
                | "wsl"
                | "wsl.exe"
        )
    });
    let flag = tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "-c" | "--command" | "-command" | "-e" | "--eval" | "-encodedcommand" | "-enc"
        )
    });
    interpreter && flag
}

/// A credential file or directory inside the workspace, not a parent path
/// outside it. The workspace root itself is not treated as a credential store.
pub(crate) fn path_is_credential_store(root: &Path, path: &Path) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components().any(|component| {
        let text = component.as_os_str().to_string_lossy();
        credential_store_name(&text)
    })
}

fn credential_store_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        ".ssh"
            | ".aws"
            | ".gnupg"
            | ".kube"
            | ".git-credentials"
            | ".netrc"
            | "_netrc"
            | "id_rsa"
            | "id_dsa"
            | "id_ecdsa"
            | "id_ed25519"
    )
}

fn command_reads_credential_store(command: &str) -> bool {
    let tokens = command_tokens(command);
    if tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "cmdkey" | "cmdkey.exe" | "vaultcmd" | "vaultcmd.exe" | "secret-tool"
        )
    }) {
        return true;
    }
    let git = tokens.iter().any(|token| token == "git" || token == "git.exe");
    git && tokens.iter().any(|token| token == "credential")
}

fn is_absolute_command_token(token: &str) -> bool {
    token.starts_with('/')
        || (token.len() >= 3
            && token.as_bytes()[0].is_ascii_alphabetic()
            && token.as_bytes()[1] == b':'
            && token.as_bytes()[2] == b'/')
}

impl Tools {
    pub fn new(ws: Workspace, allow_shell: bool) -> Self {
        let root = ws.root().to_path_buf();
        let txn = Arc::new(
            tetonic_transaction::WorkspaceTransactionService::new(
                &root,
                tetonic_transaction::WorkspaceTxnConfig::default(),
            )
            .expect("workspace transaction service"),
        );
        Self {
            ws,
            allow_shell,
            executor: coding_executor(root, EnforcementLevel::Sandboxed),
            mutation: RepositoryMutationService::new(txn),
            index_db: None,
            code_index_open: None,
            memory_db: None,
            recall_scope: None,
            session_id: None,
            lsp_enabled: false,
            lsp_open: None,
            lsp_session: Arc::new(lsp::SessionSlot::default()),
            allowed_tools: None,
            orchestration_enabled: false,
            capability_consumer: None,
            repository_disabled: false,
            reserved_files: Vec::new(),
        }
    }

    /// No caller-supplied repository. File, search, and shell tools fail closed.
    pub fn without_repository() -> std::io::Result<Self> {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("tetonic-norepo-{n}"));
        std::fs::create_dir_all(&root)?;
        let mut tools = Self::new(Workspace::new(&root)?, false);
        tools.repository_disabled = true;
        Ok(tools)
    }

    pub fn with_enforcement_level(mut self, level: EnforcementLevel) -> Self {
        self.reset_lsp_binding();
        self.executor = self.executor.clone().with_level(level);
        self
    }

    pub fn enforcement_level(&self) -> EnforcementLevel {
        self.executor.level()
    }

    pub fn executor(&self) -> &ProcessExecutor {
        &self.executor
    }

    /// The audit database and its SQLite sidecars are not a workspace grant.
    pub fn protect_store_file(mut self, path: impl AsRef<Path>) -> Self {
        for reserved in store_sidecar_paths(path.as_ref()) {
            if !self
                .reserved_files
                .iter()
                .any(|existing| paths_match(existing, &reserved))
            {
                self.reserved_files.push(reserved);
            }
        }
        self
    }

    fn reserved_store_inside_workspace(&self) -> bool {
        let root = self.ws.root();
        self.reserved_files.iter().any(|path| path.starts_with(root))
    }

    fn command_targets_reserved_store(&self, command: &str) -> bool {
        if self.reserved_files.is_empty() {
            return false;
        }
        let command = command.replace('\\', "/").to_ascii_lowercase();
        // Parent traversal is how a workspace shell reaches a store that sits
        // beside the workspace. File tools already refuse `..`.
        if command_has_parent_traversal(&command) {
            return true;
        }
        self.reserved_files.iter().any(|path| {
            let text = path.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
            let text = text.strip_prefix("//?/").unwrap_or(&text);
            if !text.is_empty() && command.contains(text) {
                return true;
            }
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    let name = name.to_ascii_lowercase();
                    !name.is_empty() && command.contains(&name)
                })
        })
    }

    fn deny_reserved(&self, path: &Path) -> Result<(), ToolError> {
        if path_is_reserved(&self.reserved_files, path) {
            Err(ToolError::Other(
                "file is outside this execution grant".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn deny_credential_store(&self, path: &Path) -> Result<(), ToolError> {
        if path_is_credential_store(self.ws.root(), path) {
            Err(ToolError::Other(
                "file is outside this execution grant".into(),
            ))
        } else {
            Ok(())
        }
    }

    pub(crate) fn deny_sqlite_database(&self, path: &Path) -> Result<(), ToolError> {
        if workspace::file_starts_with_sqlite_header(path) {
            Err(ToolError::Other(
                "file is outside this execution grant".into(),
            ))
        } else {
            Ok(())
        }
    }

    pub fn with_mutation_service(mut self, mutation: RepositoryMutationService) -> Self {
        self.mutation = mutation;
        self
    }

    pub fn with_capability_consumer(
        mut self,
        consumer: Arc<dyn tetonic_domain::CapabilityConsumer>,
    ) -> Self {
        self.reset_lsp_binding();
        self.capability_consumer = Some(consumer.clone());
        self.mutation = self
            .mutation
            .clone()
            .with_capability_consumer(consumer.clone());
        self.executor = self.executor.with_capability_consumer(consumer);
        self
    }

    pub fn is_read_only(name: &str) -> bool {
        matches!(
            name,
            "read_file"
                | "list_dir"
                | "grep"
                | "glob"
                | "outline"
                | "find_definition"
                | "find_mentions"
                | "search_code"
                | "recall"
                | "lsp_goto_definition"
                | "lsp_find_references"
                | "lsp_diagnostics"
                | "expand_context"
        )
    }

    fn tool_requires_capability(name: &str) -> bool {
        matches!(
            name,
            "read_file" | "list_dir" | "grep" | "glob" | "edit_file" | "write_file" | "run_shell"
        )
    }

    pub fn process_executor(&self) -> &ProcessExecutor {
        &self.executor
    }

    pub fn mutation_service(&self) -> &RepositoryMutationService {
        &self.mutation
    }

    /// Commit staged workspace mutations (M2-4).
    pub fn commit_staged(&self) -> Result<tetonic_domain::CommitResult, ToolError> {
        self.mutation
            .commit_staged(tetonic_domain::DataClass::RepositorySource)
    }

    /// Preview the active staged transaction, if any (M2-4 user-visible diff/conflicts).
    pub fn staged_transaction_preview(
        &self,
    ) -> Result<Option<tetonic_domain::TransactionPreview>, ToolError> {
        self.mutation.preview_active_if_any()
    }

    /// Bind user approval to the active staged patch (M2-4).
    pub fn bind_patch_approval(
        &self,
        approval: tetonic_domain::PatchApproval,
    ) -> Result<(), ToolError> {
        self.mutation.bind_patch_approval(approval)
    }

    /// Bind the active staged transaction to the claiming task/attempt, if any.
    pub fn bind_effect_identity(
        &self,
        task_id: tetonic_domain::TaskId,
        attempt_id: tetonic_domain::AttemptId,
    ) -> Result<(), ToolError> {
        self.mutation.bind_effect_identity(task_id, attempt_id)
    }

    /// Commit staged mutations when an active transaction has writes; no-op otherwise.
    pub fn commit_staged_if_any(&self) -> Result<Option<tetonic_domain::CommitResult>, ToolError> {
        self.mutation.commit_staged_if_any()
    }

    /// Abort staged mutations without commit (R6-3 cancel/fail path).
    pub fn abort_staged_if_any(&self) -> Result<bool, ToolError> {
        self.mutation.abort_staged_if_any()
    }

    /// Materialized verification overlay when staged mutations exist.
    pub fn verification_overlay_if_staged(&self) -> Result<Option<std::path::PathBuf>, ToolError> {
        self.mutation.verification_overlay_if_staged()
    }

    pub fn finish_verification_run(
        &self,
        command: &str,
        success: bool,
        output: &str,
        exit_status: Option<i32>,
    ) -> Result<(), ToolError> {
        self.mutation
            .finish_verification_run(command, success, output, exit_status)
    }

    /// Enable index-backed tools by pointing them at an `index.db`. Requires
    /// [`Self::with_code_index_open`] so the generic loop never names `lokai-index`.
    pub fn with_index(mut self, index_db: impl Into<PathBuf>) -> Self {
        self.index_db = Some(index_db.into());
        self
    }

    pub fn with_code_index_open(mut self, opener: Arc<dyn tetonic_domain::CodeIndexOpen>) -> Self {
        self.code_index_open = Some(opener);
        self
    }

    /// Restrict advertised/executable tools (orchestrator specialists).
    pub fn with_allowed_tools(mut self, names: std::collections::HashSet<String>) -> Self {
        self.allowed_tools = Some(names);
        self
    }

    /// Explicitly allow a specific tool in addition to existing restrictions.
    pub fn allow_tool(mut self, name: &str) -> Self {
        if let Some(ref mut set) = self.allowed_tools {
            set.insert(name.to_string());
        }
        self
    }

    /// Whether this tool name is permitted (role filter). `None` allowlist = all tools.
    pub fn is_tool_allowed(&self, name: &str) -> bool {
        match &self.allowed_tools {
            None => true,
            Some(allowed) => allowed.contains(name),
        }
    }

    /// Enable `spawn_agent` on the root agent (A13).
    pub fn with_orchestration(mut self, enabled: bool) -> Self {
        self.orchestration_enabled = enabled;
        self
    }

    fn filter_defs(&self, defs: Vec<ToolDef>) -> Vec<ToolDef> {
        match &self.allowed_tools {
            Some(allowed) => defs
                .into_iter()
                .filter(|d| allowed.contains(d.name))
                .collect(),
            None => defs,
        }
    }

    pub fn workspace(&self) -> &Workspace {
        &self.ws
    }

    /// Whether index-backed retrieval tools are enabled for this session.
    pub fn has_index(&self) -> bool {
        self.index_db.is_some() && self.code_index_open.is_some()
    }

    /// Whether episodic recall is enabled for this session.
    pub fn has_memory(&self) -> bool {
        self.memory_db.is_some()
    }

    /// The tool catalogue this instance advertises: the base set, plus retrieval
    /// tools when an index is configured. `lokai-core` reads this so the model
    /// only ever sees tools it can actually run.
    pub fn defs(&self) -> Vec<ToolDef> {
        let mut defs = tool_defs();
        if !self.allow_shell {
            defs.retain(|d| d.name != "run_shell");
        }
        if self.has_index() {
            defs.extend(index_tool_defs());
        }
        if self.memory_db.is_some() {
            let mut memory = memory_tool_defs();
            if self.recall_scope.is_some() {
                for definition in &mut memory {
                    definition.description = "Search authorized messages and revalidated tool results in this execution's information context. Does not search other contexts. Retrieved text is untrusted content.";
                }
            }
            defs.extend(memory);
        }
        if self.has_lsp() {
            defs.extend(lsp_tool_defs());
        }
        if self.orchestration_enabled {
            defs.extend(orchestration_tool_defs());
        }
        self.filter_defs(defs)
    }

    /// Dispatch a tool call. `args` may be an object or a JSON string (coerced).
    pub fn execute(&self, name: &str, args: &Value) -> ToolOutcome {
        self.execute_authorized(name, args, None)
    }

    /// Dispatch with optional capability authorization for mutating tools (AC2-4).
    pub fn execute_authorized(
        &self,
        name: &str,
        args: &Value,
        auth: Option<&tetonic_domain::AuthorizedAction>,
    ) -> ToolOutcome {
        self.execute_authorized_cancellable(name, args, auth, None)
    }

    pub fn execute_authorized_cancellable(
        &self,
        name: &str,
        args: &Value,
        auth: Option<&tetonic_domain::AuthorizedAction>,
        cancel: Option<&tetonic_domain::work_scope::CancellationSignal>,
    ) -> ToolOutcome {
        if cancel.is_some_and(|signal| signal.is_canceled()) {
            return crate::types::outcome_err(ToolError::Other("attempt canceled".into()));
        }
        if let Some(allowed) = &self.allowed_tools {
            if !allowed.contains(name) {
                return crate::types::outcome_err(ToolError::Other(format!(
                    "tool '{name}' not allowed for this specialist role"
                )));
            }
        }
        if self.repository_disabled && !matches!(name, "finish" | "recall") {
            return crate::types::outcome_err(ToolError::Other(
                "this execution has no repository".into(),
            ));
        }
        let args = match types::coerce_args(args) {
            Ok(a) => a,
            Err(e) => return crate::types::outcome_err(e),
        };
        if let Some(authorized) = auth {
            if let Err(e) =
                crate::mutation::enforce_live_workspace_version(authorized, self.ws.root())
            {
                return crate::types::outcome_err(ToolError::Other(format!(
                    "capability denied: {e}"
                )));
            }
        }
        if Self::tool_requires_capability(name) && self.capability_consumer.is_some() {
            let Some(authorized) = auth else {
                return crate::types::outcome_err(ToolError::Other(format!(
                    "capability required for '{name}' (R6-1)"
                )));
            };
            match name {
                "edit_file" | "write_file" => {
                    return self.mutation.run_authorized(&self.ws, authorized);
                }
                "run_shell" => {
                    return tool_outcome_from_execution(
                        self.executor
                            .run_process_sync_cancellable(authorized, cancel),
                    );
                }
                "read_file" | "list_dir" | "grep" | "glob" => {
                    if let Some(consumer) = self.capability_consumer.as_ref() {
                        if let Err(e) = consumer.authorize(authorized) {
                            return crate::types::outcome_err(ToolError::Other(format!(
                                "capability denied for '{name}': {e}"
                            )));
                        }
                    }
                }
                _ => {}
            }
        } else if let Some(authorized) = auth {
            match name {
                "edit_file" | "write_file" => {
                    return self.mutation.run_authorized(&self.ws, authorized);
                }
                "run_shell" => {
                    return tool_outcome_from_execution(
                        self.executor
                            .run_process_sync_cancellable(authorized, cancel),
                    );
                }
                _ => {}
            }
        }
        let result = match name {
            "read_file" => self.read_file(args),
            "list_dir" => self.list_dir(args),
            "grep" => self.grep(args),
            "glob" => self.glob(args),
            "edit_file" => self.edit_file(args, auth),
            "write_file" => self.write_file(args, auth),
            "run_shell" => self.run_shell(args, cancel),
            "finish" => match Self::parse::<FinishArgs>(args) {
                Ok(a) => Ok(ToolOutcome::ok(
                    a.summary.clone(),
                    format!("Task complete: {}", a.summary),
                )),
                Err(e) => Err(e),
            },
            "find_definition" => self.find_definition(args),
            "search_code" => self.search_code(args),
            "outline" => self.outline(args),
            "find_mentions" | "find_references" => self.find_mentions(args),
            "recall" => self.recall(args),
            "lsp_goto_definition" => self.lsp_goto_definition(args, cancel),
            "lsp_find_references" => self.lsp_find_references(args, cancel),
            "lsp_diagnostics" => self.lsp_diagnostics(args, cancel),
            "spawn_agent" => Err(ToolError::Other(
                "spawn_agent is handled by the orchestrator host".into(),
            )),
            other => Err(ToolError::Other(format!("unknown tool '{other}'"))),
        };
        match result {
            Ok(o) => o,
            Err(e) => crate::types::outcome_err(e),
        }
    }

    fn open_index(&self) -> Result<(Rc<dyn tetonic_domain::CodeIndex>, String), ToolError> {
        let path = self
            .index_db
            .as_ref()
            .ok_or_else(|| ToolError::Other("code index not enabled for this session".into()))?;
        let opener = self.code_index_open.as_ref().ok_or_else(|| {
            ToolError::Other("code index opener not injected for this session".into())
        })?;
        let idx = INDEX_CACHE.with(
            |c| -> Result<Rc<dyn tetonic_domain::CodeIndex>, ToolError> {
                let mut map = c.borrow_mut();
                if let Some(i) = map.get(path) {
                    return Ok(i.clone());
                }
                let opened: Rc<dyn tetonic_domain::CodeIndex> = Rc::from(
                    opener
                        .open(path)
                        .map_err(|e| ToolError::Other(format!("opening index: {e}")))?,
                );
                map.insert(path.clone(), opened.clone());
                Ok(opened)
            },
        )?;
        Ok((idx, self.ws.root().display().to_string()))
    }

    fn find_definition(&self, args: Value) -> Result<ToolOutcome, ToolError> {
        let (idx, ws) = self.open_index()?;
        retrieval::find_definition(idx.as_ref(), &ws, args)
    }

    fn search_code(&self, args: Value) -> Result<ToolOutcome, ToolError> {
        let (idx, ws) = self.open_index()?;
        retrieval::search_code(idx.as_ref(), &ws, args)
    }

    fn outline(&self, args: Value) -> Result<ToolOutcome, ToolError> {
        let (idx, ws) = self.open_index()?;
        retrieval::outline(idx.as_ref(), &ws, args)
    }

    fn find_mentions(&self, args: Value) -> Result<ToolOutcome, ToolError> {
        let (idx, ws) = self.open_index()?;
        retrieval::find_mentions(idx.as_ref(), &ws, args)
    }

    fn parse<T: for<'de> Deserialize<'de>>(args: Value) -> Result<T, ToolError> {
        serde_json::from_value(args).map_err(|e| ToolError::BadArgs(e.to_string()))
    }

    fn read_file(&self, args: Value) -> Result<ToolOutcome, ToolError> {
        let mut a: ReadFileArgs = Self::parse(args)?;
        if a.start_line.is_none() && a.end_line.is_none() && a.path.contains(':') {
            if let Some((p, range)) = a.path.split_once(':') {
                if let Some((s_str, e_str)) = range.split_once('-') {
                    if let (Ok(s), Ok(e)) = (s_str.parse::<usize>(), e_str.parse::<usize>()) {
                        a.path = p.to_string();
                        a.start_line = Some(s);
                        a.end_line = Some(e);
                    }
                } else if let Ok(s) = range.parse::<usize>() {
                    a.path = p.to_string();
                    a.start_line = Some(s);
                    a.end_line = Some(s + 50);
                }
            }
        }
        let path = self.ws.resolve(&a.path)?;
        self.deny_reserved(&path)?;
        self.deny_credential_store(&path)?;
        if tetonic_transaction::fs_ops::is_symlink_or_reparse(&path) || !path.is_file() {
            let sug = workspace::find_similar_paths(self.ws.root(), &a.path).unwrap_or_default();
            return Err(ToolError::not_found(a.path, sug));
        }
        let text = workspace::read_to_string_nofollow(&path)?;
        let _ = self
            .mutation
            .transaction_service()
            .with_active_if_any(|txn| txn.record_read(&a.path));
        let byte_len = text.len();
        let content = match (a.start_line, a.end_line) {
            // Whole-file read: borrow `text` directly instead of cloning it.
            (None, None) => truncate(&text),
            (start, end) => {
                let s = start.unwrap_or(1).max(1);
                let e = end.unwrap_or(usize::MAX);
                let selected = text
                    .lines()
                    .enumerate()
                    .filter(|(i, _)| {
                        let ln = i + 1;
                        ln >= s && ln <= e
                    })
                    .map(|(i, l)| format!("{:>6}| {l}", i + 1))
                    .collect::<Vec<_>>()
                    .join("\n");
                truncate(&selected)
            }
        };
        Ok(ToolOutcome::ok(
            format!("read {} ({} bytes)", a.path, byte_len),
            content,
        ))
    }

    fn list_dir(&self, args: Value) -> Result<ToolOutcome, ToolError> {
        let a: ListDirArgs = Self::parse(args)?;
        let rel = a.path.unwrap_or_else(|| ".".to_string());
        let dir = self.ws.resolve(&rel)?;
        self.deny_credential_store(&dir)?;
        if !dir.is_dir() {
            let sug = workspace::find_similar_paths(self.ws.root(), &rel).unwrap_or_default();
            return Err(ToolError::not_found(rel, sug));
        }
        let mut entries: Vec<String> = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|e| ToolError::Io(e.to_string()))? {
            let entry = entry.map_err(|e| ToolError::Io(e.to_string()))?;
            if self.deny_reserved(&entry.path()).is_err()
                || self.deny_credential_store(&entry.path()).is_err()
            {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            entries.push(if is_dir { format!("{name}/") } else { name });
        }
        entries.sort();
        Ok(ToolOutcome::ok(
            format!("{} entries in {rel}", entries.len()),
            truncate(&entries.join("\n")),
        ))
    }

    fn grep(&self, args: Value) -> Result<ToolOutcome, ToolError> {
        let a: GrepArgs = Self::parse(args)?;
        let base = self.ws.resolve(a.path.as_deref().unwrap_or("."))?;
        let re = regex::Regex::new(&a.pattern).map_err(|e| ToolError::BadArgs(e.to_string()))?;
        let limit = a.max_results.unwrap_or(DEFAULT_GREP_RESULTS);

        let (tx, rx) = std::sync::mpsc::channel::<Vec<(String, usize, String)>>();
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let total_hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let ws = &self.ws;
        let reserved = self.reserved_files.clone();
        let re = &re;
        let cancelled_flag = cancelled.clone();
        let total_hits_flag = total_hits.clone();

        struct GrepCollector {
            tx: std::sync::mpsc::Sender<Vec<(String, usize, String)>>,
            local: Vec<(String, usize, String)>,
        }
        impl Drop for GrepCollector {
            fn drop(&mut self) {
                if !self.local.is_empty() {
                    let _ = self.tx.send(std::mem::take(&mut self.local));
                }
            }
        }

        ignore::WalkBuilder::new(&base)
            .follow_links(false)
            .build_parallel()
            .run(|| {
                let mut collector = GrepCollector {
                    tx: tx.clone(),
                    local: Vec::new(),
                };
                let cancelled = cancelled_flag.clone();
                let total_hits = total_hits_flag.clone();
                let reserved = reserved.clone();

                Box::new(move |result| {
                    if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                        return ignore::WalkState::Quit;
                    }
                    let Ok(entry) = result else {
                        return ignore::WalkState::Continue;
                    };
                    if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                        return ignore::WalkState::Continue;
                    }
                    if tetonic_transaction::fs_ops::is_symlink_or_reparse(entry.path()) {
                        return ignore::WalkState::Continue;
                    }
                    if path_is_reserved(&reserved, entry.path())
                        || path_is_credential_store(ws.root(), entry.path())
                    {
                        return ignore::WalkState::Continue;
                    }
                    let Ok(text) = workspace::read_to_string_nofollow(entry.path()) else {
                        return ignore::WalkState::Continue;
                    };
                    let rel = ws.display_rel(entry.path());
                    for (i, line) in text.lines().enumerate() {
                        if re.is_match(line) {
                            collector
                                .local
                                .push((rel.clone(), i + 1, line.trim_end().to_string()));
                        }
                    }
                    if collector.local.len() >= 64 {
                        let current = total_hits
                            .fetch_add(collector.local.len(), std::sync::atomic::Ordering::Relaxed)
                            + collector.local.len();
                        let _ = collector.tx.send(std::mem::take(&mut collector.local));
                        if current >= limit {
                            cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                            return ignore::WalkState::Quit;
                        }
                    }
                    ignore::WalkState::Continue
                })
            });
        drop(tx);

        let mut hits = Vec::new();
        while let Ok(batch) = rx.recv() {
            hits.extend(batch);
        }

        hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        hits.truncate(limit);
        let lines: Vec<String> = hits
            .iter()
            .map(|(rel, n, line)| format!("{rel}:{n}: {line}"))
            .collect();
        Ok(ToolOutcome::ok(
            format!("{} match(es) for /{}/", lines.len(), a.pattern),
            truncate(&lines.join("\n")),
        ))
    }

    fn glob(&self, args: Value) -> Result<ToolOutcome, ToolError> {
        let a: GlobArgs = Self::parse(args)?;
        let glob = globset::Glob::new(&a.pattern)
            .map_err(|e| ToolError::BadArgs(e.to_string()))?
            .compile_matcher();

        let (tx, rx) = std::sync::mpsc::channel::<Vec<String>>();
        let ws = &self.ws;
        let reserved = self.reserved_files.clone();
        let glob = &glob;

        struct GlobCollector {
            tx: std::sync::mpsc::Sender<Vec<String>>,
            local: Vec<String>,
        }
        impl Drop for GlobCollector {
            fn drop(&mut self) {
                if !self.local.is_empty() {
                    let _ = self.tx.send(std::mem::take(&mut self.local));
                }
            }
        }

        ignore::WalkBuilder::new(self.ws.root())
            .follow_links(false)
            .build_parallel()
            .run(|| {
                let mut collector = GlobCollector {
                    tx: tx.clone(),
                    local: Vec::new(),
                };
                let reserved = reserved.clone();
                Box::new(move |result| {
                    let Ok(entry) = result else {
                        return ignore::WalkState::Continue;
                    };
                    if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                        return ignore::WalkState::Continue;
                    }
                    if tetonic_transaction::fs_ops::is_symlink_or_reparse(entry.path()) {
                        return ignore::WalkState::Continue;
                    }
                    if path_is_reserved(&reserved, entry.path())
                        || path_is_credential_store(ws.root(), entry.path())
                    {
                        return ignore::WalkState::Continue;
                    }
                    let rel = ws.display_rel(entry.path());
                    if glob.is_match(&rel) {
                        collector.local.push(rel);
                        if collector.local.len() >= 64 {
                            let _ = collector.tx.send(std::mem::take(&mut collector.local));
                        }
                    }
                    ignore::WalkState::Continue
                })
            });
        drop(tx);

        let mut matches = Vec::new();
        while let Ok(batch) = rx.recv() {
            matches.extend(batch);
        }

        matches.sort();
        Ok(ToolOutcome::ok(
            format!("{} file(s) match '{}'", matches.len(), a.pattern),
            truncate(&matches.join("\n")),
        ))
    }

    fn edit_file(
        &self,
        args: Value,
        auth: Option<&tetonic_domain::AuthorizedAction>,
    ) -> Result<ToolOutcome, ToolError> {
        if let Some(path) = args.get("path").and_then(|value| value.as_str()) {
            let path = self.ws.resolve(path)?;
            self.deny_reserved(&path)?;
            self.deny_sqlite_database(&path)?;
            self.deny_credential_store(&path)?;
        }
        self.mutation.edit_file(&self.ws, auth, args)
    }

    fn write_file(
        &self,
        args: Value,
        auth: Option<&tetonic_domain::AuthorizedAction>,
    ) -> Result<ToolOutcome, ToolError> {
        if let Some(path) = args.get("path").and_then(|value| value.as_str()) {
            let path = self.ws.resolve(path)?;
            self.deny_reserved(&path)?;
            self.deny_sqlite_database(&path)?;
            self.deny_credential_store(&path)?;
        }
        self.mutation.write_file(&self.ws, auth, args)
    }

    /// Bind verify cwd to the staged verification overlay when a txn is open (R10).
    /// Returns the overlay path when applied.
    ///
    /// Callers must bind **before** capability issue: consume recomputes the
    /// canonical digest, so rewriting `working_directory` after issue is
    /// `ScopeMismatch` (M5 D-6).
    pub fn apply_verify_overlay(
        &self,
        authorized: &mut tetonic_domain::AuthorizedAction,
    ) -> Option<std::path::PathBuf> {
        let overlay = self.verification_overlay_if_staged().ok().flatten()?;
        authorized.action.parameters.working_directory = Some(overlay.display().to_string());
        Some(overlay)
    }

    /// Run verify through the process sink when a capability is issued (AC2-3).
    /// When staged mutations exist, runs against the staged verify-view (R10).
    /// Overlay cwd must already be on the issued action (see `apply_verify_overlay`).
    pub fn run_verify_sink(&self, authorized: &tetonic_domain::AuthorizedAction) -> (bool, String) {
        let overlay = self.verification_overlay_if_staged().ok().flatten();
        let outcome = self.executor.run_process_sync(authorized);
        let (ok, output) = verify_result_from_execution(outcome);
        if overlay.is_some() {
            let cmd = authorized
                .action
                .parameters
                .executable_identity
                .clone()
                .unwrap_or_else(|| "verify".into());
            let args = authorized.action.parameters.arguments.join(" ");
            let command = if args.is_empty() {
                cmd
            } else {
                format!("{cmd} {args}")
            };
            let _ = self.finish_verification_run(
                &command,
                ok,
                &output,
                if ok { Some(0) } else { Some(1) },
            );
        }
        (ok, output)
    }

    /// Run a trusted, host-supplied command in the workspace and return
    /// `(success, combined truncated output)`. Used by the agent's
    /// verify-before-finish gate when no capability issuer is configured.
    pub fn run_command(&self, command: &str) -> (bool, String) {
        self.run_command_cancellable(command, None)
    }

    pub fn run_command_cancellable(
        &self,
        command: &str,
        cancel: Option<&tetonic_domain::work_scope::CancellationSignal>,
    ) -> (bool, String) {
        if cancel.is_some_and(|s| s.is_canceled()) {
            return (false, "command canceled".into());
        }
        if self.command_targets_reserved_store(command) {
            return (
                false,
                "command cannot read a protected store file".into(),
            );
        }
        if command_escapes_workspace(self.ws.root(), command) {
            return (
                false,
                "command cannot use a path outside this workspace".into(),
            );
        }
        let overlay = self.verification_overlay_if_staged().ok().flatten();
        let cwd = overlay
            .clone()
            .unwrap_or_else(|| self.ws.root().to_path_buf());
        let exec = ProcessExecutor::new(
            cwd,
            self.executor.enforcement_level(),
            self.executor.validator().clone(),
        );
        let r = exec.run_verify_with_signal(command, cancel);
        if overlay.is_some() {
            let _ = self.finish_verification_run(
                command,
                r.success,
                &r.output,
                if r.success { Some(0) } else { Some(1) },
            );
        }
        (r.success, r.output)
    }

    /// Validate tool arguments without executing (D7 repair pass).
    pub fn validate_tool_args(&self, name: &str, args: &Value) -> Result<(), String> {
        catalog::validate_tool_args(name, args)
    }
}

#[cfg(test)]
mod norepo_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn without_repository_serves_recall_and_refuses_files() {
        let tools = Tools::without_repository().unwrap();
        let denied = tools.execute("read_file", &json!({"path":"secret.txt"}));
        assert!(!denied.ok);
        assert!(denied.content.contains("no repository"));
        let done = tools.execute("finish", &json!({"summary":"noted"}));
        assert!(done.ok);
    }
}

#[cfg(test)]
#[path = "integration_tests.rs"]
mod integration_tests;

#[cfg(test)]
mod cancellation_tests;
