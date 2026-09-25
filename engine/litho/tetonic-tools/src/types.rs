//! Shared tool types, errors, and argument schemas.

use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use serde_json::{json, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("path '{path}' does not exist{suggestion}")]
    NotFound { path: String, suggestion: String },
    #[error("path '{0}' escapes the workspace root")]
    OutsideWorkspace(String),
    #[error("{0}")]
    NoMatch(String),
    #[error("old_string is ambiguous: matched {0} times (make it unique)")]
    Ambiguous(usize),
    #[error("invalid arguments: {0}")]
    BadArgs(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("shell command not approved (re-run with approval to allow)")]
    ShellNotApproved,
    #[error("{0}")]
    Other(String),
}

impl ToolError {
    pub fn not_found(path: impl Into<String>, suggestion: impl Into<String>) -> Self {
        Self::NotFound {
            path: path.into(),
            suggestion: suggestion.into(),
        }
    }

    /// Stable, machine-facing kind (for the audit trail / approvals).
    pub fn kind(&self) -> &'static str {
        match self {
            ToolError::NotFound { .. } => "not_found",
            ToolError::OutsideWorkspace(_) => "outside_workspace",
            ToolError::NoMatch(_) => "no_match",
            ToolError::Ambiguous(_) => "ambiguous",
            ToolError::BadArgs(_) => "bad_args",
            ToolError::Io(_) => "io",
            ToolError::ShellNotApproved => "denied",
            ToolError::Other(_) => "other",
        }
    }

    pub fn into_outcome(self) -> tetonic_domain::ToolOutcome {
        let kind = self.kind();
        tetonic_domain::ToolOutcome::fail(self, kind)
    }
}

pub fn outcome_err(e: ToolError) -> ToolOutcome {
    e.into_outcome()
}

pub use tetonic_domain::{ChangeKind, FileChange, ToolOutcome};

#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
    pub mutating: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadFileArgs {
    pub path: String,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListDirArgs {
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GrepArgs {
    pub pattern: String,
    pub path: Option<String>,
    pub max_results: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GlobArgs {
    pub pattern: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EditFileArgs {
    pub path: String,
    pub old_string: String,
    pub new_string: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteFileArgs {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunShellArgs {
    pub command: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FinishArgs {
    pub summary: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindDefinitionArgs {
    pub name: String,
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchCodeArgs {
    pub query: String,
    pub max_results: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OutlineArgs {
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindMentionsArgs {
    pub name: String,
    pub max_results: Option<usize>,
}

pub type FindReferencesArgs = FindMentionsArgs;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecallArgs {
    pub query: String,
    pub max_results: Option<usize>,
}

pub(crate) fn schema_of<T: JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).expect("schema serializes")
}

pub(crate) fn coerce_args(args: &Value) -> Result<Value, ToolError> {
    match args {
        Value::String(s) => serde_json::from_str(s)
            .map_err(|e| ToolError::BadArgs(format!("invalid JSON tool args: {e}"))),
        Value::Null => Ok(json!({})),
        other => Ok(other.clone()),
    }
}
