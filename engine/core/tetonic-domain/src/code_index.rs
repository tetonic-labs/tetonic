//! Injected code-index / skeleton seams. Impl lives in `lokai-index` (product).

use std::path::Path;

#[derive(Debug, Clone)]
pub struct CodeDefinition {
    pub kind: String,
    pub rel: String,
    pub start_line: u32,
    pub signature: String,
}

#[derive(Debug, Clone)]
pub struct CodeSearchHit {
    pub rel: String,
    pub start_line: u32,
    pub end_line: u32,
    pub symbol_name: String,
    pub preview: String,
    pub score: f32,
}

#[derive(Debug, Clone)]
pub struct CodeOutlineRow {
    pub depth: usize,
    pub kind: String,
    pub name: String,
    pub start_line: u32,
}

#[derive(Debug, Clone)]
pub struct CodeIndexStatus {
    pub summary: String,
}

/// Per-open index handle. Not [`Sync`] (matches today's SQLite index).
pub trait CodeIndex: Send {
    fn workspace_key(&self, root: &Path) -> String;
    fn status(&self, ws_key: &str) -> Result<CodeIndexStatus, String>;
    fn find_definition_in(
        &self,
        ws_key: &str,
        name: &str,
        path: Option<&str>,
    ) -> Result<Vec<CodeDefinition>, String>;
    fn search(&self, ws_key: &str, query: &str, limit: u32) -> Result<Vec<CodeSearchHit>, String>;
    fn outline(&self, ws_key: &str, rel: &str) -> Result<Vec<CodeOutlineRow>, String>;
    fn find_mentions(
        &self,
        ws_key: &str,
        name: &str,
        limit: u32,
    ) -> Result<Vec<CodeSearchHit>, String>;
}

/// Opens an index at a path. [`Send`] + [`Sync`] so assembly can hold it.
pub trait CodeIndexOpen: Send + Sync {
    fn open(&self, path: &Path) -> Result<Box<dyn CodeIndex>, String>;
}

/// Optional retrieve-time skeletonizer (coding intel). Default is identity.
pub trait TextSkeleton: Send + Sync {
    fn skeletonize(&self, path: &str, text: &str) -> String;
}
