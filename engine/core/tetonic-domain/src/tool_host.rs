//! Pack interface: named tool → [`ProposedAction`]. Not an authority (`02`).
//!
//! `execute_authorized` is catalog dispatch **after** ActionBroker / consume.
//! Implementations must not `Command::new` (ProcessAuthority stays ProcessBroker).

use serde_json::Value;

use crate::execution::{ActionKind, AuthorizedAction};

/// Thin tool result for the generic loop (not a coding-crate type).
#[derive(Debug, Clone)]
pub struct ToolOutcome {
    pub ok: bool,
    pub summary: String,
    pub content: String,
    pub error_kind: Option<String>,
    pub change: Option<FileChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Edit,
    Create,
    Delete,
}

impl ChangeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChangeKind::Edit => "edit",
            ChangeKind::Create => "create",
            ChangeKind::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
    pub before: Option<String>,
    pub after: Option<String>,
}

impl ToolOutcome {
    pub fn ok(summary: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            ok: true,
            summary: summary.into(),
            content: content.into(),
            error_kind: None,
            change: None,
        }
    }

    pub fn fail(message: impl std::fmt::Display, kind: impl Into<String>) -> Self {
        let message = message.to_string();
        Self {
            ok: false,
            summary: format!("error: {message}"),
            content: format!("ERROR: {message}"),
            error_kind: Some(kind.into()),
            change: None,
        }
    }

    pub fn to_model_string(&self) -> String {
        if self.content.is_empty() {
            self.summary.clone()
        } else {
            self.content.clone()
        }
    }

    pub fn with_change(mut self, change: FileChange) -> Self {
        self.change = Some(change);
        self
    }
}

/// Capability kind + path hint. `propose` does not evaluate, issue, or spawn.
#[derive(Debug, Clone)]
pub struct ToolProposal {
    pub kind: ActionKind,
    pub resolved_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolAdvertisement {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// Named tool → [`ToolProposal`]; execute after the broker. Send + Sync + clone for concurrency.
pub trait ToolHost: Send + Sync {
    fn clone_box(&self) -> Box<dyn ToolHost>;

    fn propose(&self, name: &str, args: &Value) -> Option<ToolProposal>;
    fn is_tool_allowed(&self, name: &str) -> bool;
    fn is_read_only(&self, name: &str) -> bool;
    /// Whether this named tool must be routed through the action broker.
    fn requires_action_broker(&self, name: &str) -> bool {
        let _ = name;
        false
    }
    /// Whether this named tool requires an interactive user-approval hook when
    /// no action broker is present. Recipe table lives in the implementor.
    fn requires_user_approval(&self, name: &str) -> bool {
        let _ = name;
        false
    }
    fn advertisements(&self) -> Vec<ToolAdvertisement>;
    fn validate_tool_args(&self, name: &str, args: &Value) -> Result<(), String>;
    /// Cancellation is invocation-local; implementations must not store it as shared host state.
    fn execute_authorized(
        &self,
        name: &str,
        args: &Value,
        auth: Option<&AuthorizedAction>,
        cancel: &crate::work_scope::CancellationSignal,
    ) -> ToolOutcome;
}

impl Clone for Box<dyn ToolHost> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::ActionKind;
    use serde_json::json;

    struct DummyHost;

    impl ToolHost for DummyHost {
        fn clone_box(&self) -> Box<dyn ToolHost> {
            Box::new(DummyHost)
        }
        fn propose(&self, _name: &str, _args: &Value) -> Option<ToolProposal> {
            Some(ToolProposal {
                kind: ActionKind::ReadFile,
                resolved_path: None,
            })
        }
        fn is_tool_allowed(&self, _name: &str) -> bool {
            true
        }
        fn is_read_only(&self, _name: &str) -> bool {
            true
        }
        fn advertisements(&self) -> Vec<ToolAdvertisement> {
            vec![]
        }
        fn validate_tool_args(&self, _name: &str, _args: &Value) -> Result<(), String> {
            Ok(())
        }
        fn execute_authorized(
            &self,
            _name: &str,
            _args: &Value,
            _auth: Option<&AuthorizedAction>,
            _cancel: &crate::work_scope::CancellationSignal,
        ) -> ToolOutcome {
            ToolOutcome::ok("ok", "ok")
        }
    }

    #[test]
    fn tool_host_is_a_rustc_trait() {
        let host: Box<dyn ToolHost> = Box::new(DummyHost);
        assert!(host.propose("read_file", &json!({})).is_some());
        let _ = host.clone();
    }
}
