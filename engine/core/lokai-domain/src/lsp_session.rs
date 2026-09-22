//! Injected LSP session seam. Impl lives in the product pack (`lokai-app` / `lokai-lsp`).

use std::path::Path;

use crate::tool_host::ToolOutcome;

/// One workspace LSP pool. Thread-safe (`Send + Sync`).
pub trait LspSession: Send + Sync {
    fn goto_definition(&self, path: &str, line: u32, character: u32)
        -> Result<ToolOutcome, String>;
    fn find_references(&self, path: &str, line: u32, character: u32)
        -> Result<ToolOutcome, String>;
    fn diagnostics(&self, path: &str) -> Result<ToolOutcome, String>;
}

pub trait LspSessionOpen: Send + Sync {
    fn open(&self, root: &Path) -> Result<Box<dyn LspSession>, String>;
    fn available(&self, root: &Path) -> bool;
}
