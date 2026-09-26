//! LSP-backed agent tools. Session impl lives in the product pack.

use std::sync::{Arc, Mutex};

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use tetonic_domain::{LspSession, LspSessionOpen};

use crate::{ToolError, ToolOutcome, Tools};

/// Shared only by clones of one tool binding, never by workspace identity.
#[derive(Default)]
pub(crate) struct SessionSlot(pub Mutex<Option<Arc<dyn LspSession>>>);

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LspPositionArgs {
    /// Workspace-relative file path.
    pub path: String,
    /// 1-based line number (matches read_file line numbers).
    pub line: u32,
    /// 0-based UTF-16 character offset on that line (default: 0 = line start).
    pub character: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LspPathArgs {
    pub path: String,
}

fn schema_of<T: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).expect("schema")
}

fn open_lsp(tools: &Tools) -> Result<Arc<dyn LspSession>, ToolError> {
    if !tools.has_lsp() {
        return Err(ToolError::Other(
            "LSP is disabled for this tool binding".into(),
        ));
    }
    let opener = tools
        .lsp_opener()
        .ok_or_else(|| ToolError::Other("LSP opener not injected for this session".into()))?;
    let root = tools.workspace().root().to_path_buf();
    let mut guard = tools
        .lsp_session
        .0
        .lock()
        .map_err(|_| ToolError::Other("LSP cache mutex poisoned".into()))?;
    if let Some(p) = guard.as_ref() {
        return Ok(p.clone());
    }
    tools
        .executor()
        .note_lsp_service_start("language-server")
        .map_err(|e| ToolError::Other(format!("LSP service policy rejected: {e}")))?;
    let opened: Arc<dyn LspSession> = Arc::from(
        opener
            .open(&root)
            .map_err(|e| ToolError::Other(format!("LSP unavailable: {e}")))?,
    );
    *guard = Some(opened.clone());
    Ok(opened)
}

fn char_arg(a: &LspPositionArgs) -> u32 {
    a.character.unwrap_or(0)
}

/// LSP tool catalogue (advertised when LSP is enabled for the session).
pub fn lsp_tool_defs() -> Vec<crate::ToolDef> {
    vec![
        crate::ToolDef {
            name: "lsp_goto_definition",
            description: "Go to the definition at a position using the language server (rust-analyzer / pyright / typescript-language-server). \
More precise than find_definition for typed refs. `character` is UTF-16; byte offsets from grep are auto-converted when needed.",
            parameters: schema_of::<LspPositionArgs>(),
            mutating: false,
        },
        crate::ToolDef {
            name: "lsp_find_references",
            description: "Find references to the symbol at a position via the language server.",
            parameters: schema_of::<LspPositionArgs>(),
            mutating: false,
        },
        crate::ToolDef {
            name: "lsp_diagnostics",
            description: "Fetch compiler/linter diagnostics for a file from the language server. \
Use before finish to catch compile/type errors.",
            parameters: schema_of::<LspPathArgs>(),
            mutating: false,
        },
    ]
}

pub fn lsp_available_for_workspace(
    opener: Option<&dyn LspSessionOpen>,
    root: &std::path::Path,
) -> bool {
    opener.map(|o| o.available(root)).unwrap_or(false)
}

impl Tools {
    /// Enable LSP-backed tools when a language server is available on PATH.
    pub fn with_lsp(mut self, enabled: bool) -> Self {
        if !enabled {
            self.lsp_session = Arc::new(SessionSlot::default());
        }
        self.lsp_enabled = enabled;
        self
    }

    /// Install after session/authority configuration. Replacing the opener creates
    /// a fresh session binding; existing clones retain their original ownership.
    pub fn with_lsp_open(mut self, opener: Arc<dyn tetonic_domain::LspSessionOpen>) -> Self {
        self.lsp_session = Arc::new(SessionSlot::default());
        self.lsp_open = Some(opener);
        self.lsp_enabled = true;
        self
    }

    // An opener can capture authority itself. Changing authority requires a fresh
    // injected opener, not merely clearing a cached session under the old one.
    pub(crate) fn reset_lsp_binding(&mut self) {
        self.lsp_session = Arc::new(SessionSlot::default());
        self.lsp_open = None;
        self.lsp_enabled = false;
    }

    pub fn has_lsp(&self) -> bool {
        self.lsp_enabled && self.lsp_open.is_some()
    }

    pub(crate) fn lsp_opener(&self) -> Option<&dyn LspSessionOpen> {
        self.lsp_open.as_deref()
    }

    pub(crate) fn lsp_goto_definition(
        &self,
        args: Value,
        cancel: Option<&tetonic_domain::work_scope::CancellationSignal>,
    ) -> Result<ToolOutcome, ToolError> {
        let a: LspPositionArgs = Tools::parse(args)?;
        let path = self.ws.resolve(&a.path)?;
        self.deny_reserved(&path)?;
        self.deny_sqlite_database(&path)?;
        self.deny_credential_store(&path)?;
        let session = open_lsp(self)?;
        drive_lsp(&session, cancel, || {
            session
                .goto_definition(&a.path, a.line, char_arg(&a))
                .map_err(ToolError::Other)
        })
    }

    pub(crate) fn lsp_find_references(
        &self,
        args: Value,
        cancel: Option<&tetonic_domain::work_scope::CancellationSignal>,
    ) -> Result<ToolOutcome, ToolError> {
        let a: LspPositionArgs = Tools::parse(args)?;
        let path = self.ws.resolve(&a.path)?;
        self.deny_reserved(&path)?;
        self.deny_sqlite_database(&path)?;
        self.deny_credential_store(&path)?;
        let session = open_lsp(self)?;
        drive_lsp(&session, cancel, || {
            session
                .find_references(&a.path, a.line, char_arg(&a))
                .map_err(ToolError::Other)
        })
    }

    pub(crate) fn lsp_diagnostics(
        &self,
        args: Value,
        cancel: Option<&tetonic_domain::work_scope::CancellationSignal>,
    ) -> Result<ToolOutcome, ToolError> {
        let a: LspPathArgs = Tools::parse(args)?;
        let path = self.ws.resolve(&a.path)?;
        self.deny_reserved(&path)?;
        self.deny_sqlite_database(&path)?;
        self.deny_credential_store(&path)?;
        let session = open_lsp(self)?;
        drive_lsp(&session, cancel, || {
            session.diagnostics(&a.path).map_err(ToolError::Other)
        })
    }
}

fn drive_lsp<T>(
    session: &Arc<dyn tetonic_domain::LspSession>,
    cancel: Option<&tetonic_domain::work_scope::CancellationSignal>,
    call: impl FnOnce() -> T,
) -> T {
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watcher = cancel.map(|signal| {
        let signal = signal.clone();
        let session = Arc::clone(session);
        let done = done.clone();
        std::thread::spawn(move || {
            while !done.load(std::sync::atomic::Ordering::Relaxed) {
                if signal.is_canceled() {
                    session.request_stop();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        })
    });
    let result = call();
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(watcher) = watcher {
        let _ = watcher.join();
    }
    result
}

#[cfg(test)]
#[path = "lsp_ownership_tests.rs"]
mod ownership_tests;
