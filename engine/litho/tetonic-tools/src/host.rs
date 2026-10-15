//! [`tetonic_domain::ToolHost`] impl for [`Tools`].

use serde_json::Value;

use tetonic_domain::{
    ActionKind, AuthorizedAction, ToolAdvertisement, ToolHost, ToolOutcome, ToolProposal,
};

use crate::Tools;

impl ToolHost for Tools {
    fn prepare_parameters(
        &self,
        name: &str,
        parameters: tetonic_domain::execution::CanonicalActionParameters,
    ) -> Result<tetonic_domain::execution::CanonicalActionParameters, String> {
        self.prepare_shell_view(name, parameters)
    }
    fn limit_observation(&self, name: &str, args: &Value, bytes: usize) -> Option<Value> {
        if !self.is_tool_allowed(name) {
            return None;
        }
        let (requested, minimum) = match name {
            "read_file" => (
                serde_json::from_value::<crate::ReadFileArgs>(args.clone())
                    .ok()?
                    .max_bytes,
                4,
            ),
            "read_files" => (
                serde_json::from_value::<crate::ReadFilesArgs>(args.clone())
                    .ok()?
                    .max_bytes,
                16,
            ),
            "inspect_changes" => (
                serde_json::from_value::<crate::InspectChangesArgs>(args.clone())
                    .ok()?
                    .max_bytes,
                4,
            ),
            "grep" => (
                serde_json::from_value::<crate::GrepArgs>(args.clone())
                    .ok()?
                    .max_bytes,
                256,
            ),
            _ => return None,
        };
        if requested.is_some_and(|n| n < minimum) {
            return None;
        }
        let mut bounded = args.clone();
        bounded["max_bytes"] = serde_json::json!(requested.unwrap_or(2048).min(bytes.max(minimum)));
        Some(bounded)
    }

    fn checkpoint_ready(&self) -> bool {
        // WorkScope separately proves that no tool call is in flight. These
        // checks attest that rebuilding this host loses no retained tool state.
        // Shell/LSP may retain subprocess state; they are not resumable yet.
        !self.allow_shell
            && !self.lsp_enabled
            && !self.orchestration_enabled
            && matches!(self.staged_transaction_preview(), Ok(None))
    }

    fn clone_box(&self) -> Box<dyn ToolHost> {
        Box::new(self.clone())
    }

    fn propose(&self, name: &str, args: &Value) -> Option<ToolProposal> {
        let kind = match name {
            "edit_file" | "write_file" => ActionKind::WriteFile,
            "run_shell" | "run_checks" => ActionKind::ExecuteShell,
            "read_file" | "read_files" | "inspect_changes" | "list_dir" | "grep" | "glob" => {
                ActionKind::ReadFile
            }
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
        Tools::tool_requires_capability(name)
    }

    fn requires_user_approval(&self, name: &str) -> bool {
        matches!(name, "run_shell" | "run_checks") || name.starts_with("lsp_")
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
        _cancel: &tetonic_domain::work_scope::CancellationSignal,
    ) -> ToolOutcome {
        Tools::execute_authorized_cancellable(self, name, args, auth, Some(_cancel))
    }
}

#[cfg(test)]
mod checkpoint_tests {
    use super::*;

    #[test]
    fn observation_limit_only_narrows_valid_granted_pages() {
        let directory = tempfile::tempdir().unwrap();
        let tools = Tools::new(crate::Workspace::new(directory.path()).unwrap(), false);
        let args = serde_json::json!({"path":"../outside","offset_bytes":23,"expected_sha256":"old","max_bytes":800});
        let capped = tools.limit_observation("read_file", &args, 512).unwrap();
        assert_eq!(capped["max_bytes"], 512);
        for key in ["path", "offset_bytes", "expected_sha256"] {
            assert_eq!(capped[key], args[key]);
        }
        assert!(
            !tools.execute("read_file", &capped).ok,
            "paging is not authorization"
        );
        assert!(tools.limit_observation("write_file", &args, 512).is_none());
        let search = serde_json::json!({"pattern":"needle", "path":"../outside", "max_results":3,"context_lines":4});
        let capped = tools.limit_observation("grep", &search, 512).unwrap();
        for key in ["pattern", "path", "max_results", "context_lines"] {
            assert_eq!(capped[key], search[key]);
        }
        assert_eq!(capped["max_bytes"], 512);
        assert!(!tools.execute("grep", &capped).ok);
        assert!(tools
            .limit_observation(
                "grep",
                &serde_json::json!({"pattern":"x", "max_bytes":0}),
                512
            )
            .is_none());
        for invalid in [
            serde_json::json!({"path":"a","max_bytes":"bad"}),
            serde_json::json!({"path":"a","max_bytes":0}),
        ] {
            assert!(tools
                .limit_observation("read_file", &invalid, 512)
                .is_none());
        }
        let tools = tools.with_allowed_tools(std::collections::HashSet::new());
        assert!(tools.limit_observation("read_file", &args, 512).is_none());
        assert!(tools.limit_observation("grep", &search, 512).is_none());
    }

    #[test]
    fn checkpoint_refuses_staged_mutations_and_retained_subprocess_state() {
        let directory = tempfile::tempdir().unwrap();
        let tools = Tools::new(crate::Workspace::new(directory.path()).unwrap(), false);
        assert!(tools.checkpoint_ready());
        assert!(
            !tools
                .execute("read_file", &serde_json::json!({"path":"missing"}))
                .ok
        );
        assert!(tools.checkpoint_ready());
        let shared = tools.clone();
        let outcome = tools.execute(
            "write_file",
            &serde_json::json!({"path":"output.txt", "content":"staged"}),
        );
        assert!(outcome.ok, "{outcome:?}");
        assert!(!tools.checkpoint_ready());
        assert!(!shared.checkpoint_ready(), "clones share transaction state");
        assert!(!directory.path().join("output.txt").exists());
        tools.abort_staged_if_any().unwrap();
        assert!(shared.checkpoint_ready());
        assert!(!tools.clone().with_lsp(true).checkpoint_ready());
        assert!(!tools.clone().with_orchestration(true).checkpoint_ready());
        assert!(
            !Tools::new(crate::Workspace::new(directory.path()).unwrap(), true).checkpoint_ready()
        );
    }
}
