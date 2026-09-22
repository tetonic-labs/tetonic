//! Index types, errors, and language classification.

use std::path::Path;
use thiserror::Error;

pub const MAX_FILE_BYTES: u64 = 512 * 1024;
pub const MAX_CHUNK_CHARS: usize = 4000;
pub const MAX_SIG_CHARS: usize = 200;

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, IndexError>;

/// Languages we extract structure for. Everything else is still keyword-indexed
/// as plain text (`Text`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Python,
    TypeScript,
    Tsx,
    JavaScript,
    Text,
}

impl Lang {
    pub fn from_path(p: &Path) -> Lang {
        match p
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("rs") => Lang::Rust,
            Some("py") | Some("pyi") => Lang::Python,
            Some("ts") => Lang::TypeScript,
            Some("tsx") => Lang::Tsx,
            Some("js") | Some("jsx") | Some("mjs") | Some("cjs") => Lang::JavaScript,
            _ => Lang::Text,
        }
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::Python => "python",
            Lang::TypeScript => "typescript",
            Lang::Tsx => "tsx",
            Lang::JavaScript => "javascript",
            Lang::Text => "text",
        }
    }
}

/// A defined symbol (function/struct/class/...), as returned by lookups.
#[derive(Debug, Clone)]
pub struct SymbolRow {
    pub kind: String,
    pub name: String,
    pub signature: String,
    pub start_line: i64,
    pub end_line: i64,
    /// Workspace-relative path (forward slashes).
    pub rel: String,
}

/// One line of a file outline (depth = nesting under impl/class/...).
#[derive(Debug, Clone)]
pub struct OutlineRow {
    pub depth: usize,
    pub kind: String,
    pub name: String,
    pub signature: String,
    pub start_line: i64,
}

/// A keyword/structural retrieval hit (line-ranged, with a preview).
#[derive(Debug, Clone)]
pub struct Hit {
    pub rel: String,
    pub start_line: i64,
    pub end_line: i64,
    pub score: f64,
    pub symbol_name: String,
    pub preview: String,
}

/// A chunk awaiting embedding (its keyword-indexed text + identity).
#[derive(Debug, Clone)]
pub struct PendingChunk {
    pub chunk_id: i64,
    pub content: String,
}

/// Coverage snapshot for a workspace (for the Context Inspector / `--index-status`).
#[derive(Debug, Clone, Default)]
pub struct IndexStatus {
    pub files: i64,
    pub symbols: i64,
    pub chunks: i64,
    pub by_lang: Vec<(String, i64)>,
    pub last_indexed: Option<String>,
}

/// Result of an indexing pass.
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    pub seen: usize,
    pub indexed: usize,
    pub unchanged: usize,
    pub skipped: usize,
    pub deleted: usize,
    pub symbols: usize,
    pub elapsed_ms: u128,
}
