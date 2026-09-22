//! [`lokai_domain::ToolHost`] impl for [`Tools`].

use serde_json::Value;

use lokai_domain::{
    ActionKind, AuthorizedAction, ToolAdvertisement, ToolHost, ToolOutcome, ToolProposal,
};

use crate::Tools;

impl ToolHost for Tools {
    fn clone_box(&self) -> Box<dyn ToolHost> {
        Box::new(self.clone())
    }

    fn propose(&self, name: &str, args: &Value) -> Option<ToolProposal> {
        let kind = match name {
            "edit_file" | "write_file" => ActionKind::WriteFile,
            "run_shell" => ActionKind::ExecuteShell,
            "read_file" | "list_dir" | "grep" | "glob" => ActionKind::ReadFile,
            _ => return None,
        };
        let resolved_path = args
            .get("path")
            .or_else(|| args.get("pattern"))
            .and_then(|v| v.as_str())
            .map(String::from);
        Some(ToolProposal {
            kind,
            resolved_path,
        })
    }

    fn is_tool_allowed(&self, name: &str) -> bool {
        Tools::is_tool_allowed(self, name)
    }

    fn is_read_only(&self, name: &str) -> bool {
        Tools::is_read_only(name)
    }

    fn requires_action_broker(&self, name: &str) -> bool {
        matches!(
            name,
            "read_file" | "list_dir" | "grep" | "glob" | "edit_file" | "write_file" | "run_shell"
        )
    }

    fn requires_user_approval(&self, name: &str) -> bool {
        matches!(name, "run_shell") || name.starts_with("lsp_")
    }

    fn advertisements(&self) -> Vec<ToolAdvertisement> {
        self.defs()
            .into_iter()
            .map(|d| ToolAdvertisement {
                name: d.name.to_string(),
                description: d.description.to_string(),
                parameters: d.parameters,
            })
            .collect()
    }

    fn validate_tool_args(&self, name: &str, args: &Value) -> Result<(), String> {
        Tools::validate_tool_args(self, name, args)
    }

    fn execute_authorized(
        &self,
        name: &str,
        args: &Value,
        auth: Option<&AuthorizedAction>,
        _cancel: &lokai_domain::work_scope::CancellationSignal,
    ) -> ToolOutcome {
        Tools::execute_authorized_cancellable(self, name, args, auth, Some(_cancel))
    }
}
