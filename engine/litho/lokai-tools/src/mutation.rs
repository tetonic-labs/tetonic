//! Repository mutation service — routes all writes through workspace transactions (M2-4).

use std::sync::Arc;

use async_trait::async_trait;
use lokai_domain::{
    ActionKind, AuthorizedAction, CapabilityConsumer, DataClass, ExecutionId, ExecutionOutcome,
    MutationSink, WorkspacePath,
};
use lokai_transaction::WorkspaceTransactionService;
use serde_json::Value;

use crate::verify::check_python_syntax;
use crate::workspace::{refresh_mutating_path, Workspace};
use crate::{ChangeKind, EditFileArgs, FileChange, ToolError, ToolOutcome, WriteFileArgs};

/// R08: capability must carry a workspace version that still matches the live tree.
pub(crate) fn enforce_live_workspace_version(
    authorized: &AuthorizedAction,
    root: &std::path::Path,
) -> Result<(), lokai_domain::CapabilityError> {
    use lokai_domain::CapabilityError;
    let Some(expected) = authorized.capability.workspace_version.as_ref() else {
        return Err(CapabilityError::WorkspaceVersionMismatch);
    };
    let relevant: Vec<WorkspacePath> = authorized
        .action
        .parameters
        .resolved_path
        .as_ref()
        .map(|p| WorkspacePath::new(p.clone()))
        .into_iter()
        .collect();
    let live = lokai_transaction::version::capture_workspace_version(root, &relevant)
        .map_err(|_| CapabilityError::WorkspaceVersionMismatch)?;
    if &live != expected {
        return Err(CapabilityError::WorkspaceVersionMismatch);
    }
    Ok(())
}

/// Owns write/edit lifecycle; all mutations stage in an active transaction.
#[derive(Clone)]
pub struct RepositoryMutationService {
    consumer: Option<Arc<dyn CapabilityConsumer>>,
    transactions: Arc<WorkspaceTransactionService>,
}

impl RepositoryMutationService {
    pub fn new(transactions: Arc<WorkspaceTransactionService>) -> Self {
        Self {
            consumer: None,
            transactions,
        }
    }

    pub fn with_capability_consumer(mut self, consumer: Arc<dyn CapabilityConsumer>) -> Self {
        self.consumer = Some(consumer);
        self
    }

    pub fn transaction_service(&self) -> &Arc<WorkspaceTransactionService> {
        &self.transactions
    }

    pub(crate) fn authorize(&self, auth: Option<&AuthorizedAction>) -> Result<(), ToolError> {
        let Some(authorized) = auth else {
            return Ok(());
        };
        enforce_live_workspace_version(authorized, self.transactions.root())
            .map_err(|e| ToolError::Other(format!("mutation denied: {e}")))?;
        if let Some(consumer) = &self.consumer {
            consumer
                .authorize(authorized)
                .map_err(|e| ToolError::Other(format!("mutation denied: {e}")))?;
        }
        Ok(())
    }

    /// Run an authorized mutation (stage only) and return the agent-facing outcome.
    pub fn run_authorized(&self, ws: &Workspace, authorized: &AuthorizedAction) -> ToolOutcome {
        if let Err(e) = self.authorize(Some(authorized)) {
            return crate::types::outcome_err(ToolError::Other(format!("capability denied: {e}")));
        }
        match &authorized.action.kind {
            ActionKind::WriteFile => {
                let args = authorized
                    .action
                    .parameters
                    .tool_arguments
                    .clone()
                    .unwrap_or(serde_json::json!({}));
                if args.get("old_string").is_some() {
                    match self.edit_file(ws, None, args) {
                        Ok(o) => o,
                        Err(e) => crate::types::outcome_err(e),
                    }
                } else {
                    match self.write_file(ws, None, args) {
                        Ok(o) => o,
                        Err(e) => crate::types::outcome_err(e),
                    }
                }
            }
            ActionKind::ReadFile => crate::types::outcome_err(ToolError::Other(
                "RepositoryMutationService cannot run tool ReadFile".into(),
            )),
            _ => crate::types::outcome_err(ToolError::Other(format!(
                "RepositoryMutationService cannot run {:?}",
                authorized.action.kind
            ))),
        }
    }

    fn apply_mutation_sync(&self, authorized: &AuthorizedAction) -> ExecutionOutcome {
        let execution_id = ExecutionId::new(format!("mut_{}", authorized.action.action_id));
        let ws_root = authorized
            .action
            .parameters
            .working_directory
            .clone()
            .unwrap_or_else(|| self.transactions.root().display().to_string());
        let ws = match Workspace::new(&ws_root) {
            Ok(ws) => ws,
            Err(e) => {
                return ExecutionOutcome::Failed {
                    execution_id,
                    reason: format!("workspace: {e}"),
                };
            }
        };
        lokai_telemetry::fault::inject_fault("during_workspace_mutation_staging");
        let outcome = self.run_authorized(&ws, authorized);
        lokai_telemetry::fault::inject_fault("after_mutation");
        if outcome.ok {
            ExecutionOutcome::Completed {
                execution_id,
                ok: true,
                summary: outcome.summary,
            }
        } else {
            ExecutionOutcome::Completed {
                execution_id,
                ok: false,
                summary: outcome.summary,
            }
        }
    }

    pub fn edit_file(
        &self,
        ws: &Workspace,
        auth: Option<&AuthorizedAction>,
        args: Value,
    ) -> Result<ToolOutcome, ToolError> {
        self.authorize(auth)?;
        let a: EditFileArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::Other(format!("invalid edit_file args: {e}")))?;
        if a.old_string.is_empty() {
            return Err(ToolError::BadArgs(
                "old_string must not be empty (use write_file to create or replace a whole file)"
                    .into(),
            ));
        }
        let path = refresh_mutating_path(ws, &ws.resolve(&a.path)?)?;
        if lokai_transaction::fs_ops::is_symlink_or_reparse(&path) || !path.is_file() {
            let sug = crate::workspace::find_similar_paths(ws.root(), &a.path).unwrap_or_default();
            return Err(ToolError::not_found(a.path, sug));
        }
        self.transactions
            .with_active(|txn| {
                let (before, updated) =
                    txn.stage_edit_file(&a.path, &a.old_string, &a.new_string)?;
                let syntax_root = txn.staged_overlay_path()?;
                if let Err(e) = check_python_syntax(&syntax_root, &a.path) {
                    return Ok(ToolOutcome {
                        ok: false,
                        summary: format!("syntax error in {} after staged edit", a.path),
                        content: format!(
                            "ERROR: Python syntax check failed for staged edit to `{}`:\n{e}",
                            a.path
                        ),
                        error_kind: Some("syntax_error".into()),
                        change: Some(FileChange {
                            path: a.path.clone(),
                            kind: ChangeKind::Edit,
                            before: Some(before),
                            after: Some(updated),
                        }),
                    });
                }
                let preview = txn.preview();
                Ok(ToolOutcome::ok(
                    format!("staged edit {}", a.path),
                    format!(
                        "Staged edit to {} (transaction {}). Patch digest: {}",
                        a.path, preview.transaction_id, preview.patch_digest.0
                    ),
                )
                .with_change(FileChange {
                    path: a.path.clone(),
                    kind: ChangeKind::Edit,
                    before: Some(before),
                    after: Some(updated),
                }))
            })
            .map_err(map_txn_err)
    }

    pub fn write_file(
        &self,
        ws: &Workspace,
        auth: Option<&AuthorizedAction>,
        args: Value,
    ) -> Result<ToolOutcome, ToolError> {
        self.authorize(auth)?;
        let a: WriteFileArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::Other(format!("invalid write_file args: {e}")))?;
        let resolved = ws.resolve(&a.path)?;
        let path = refresh_mutating_path(ws, &resolved)?;
        let before = if path.exists() {
            if lokai_transaction::fs_ops::is_symlink_or_reparse(&path) {
                return Err(ToolError::OutsideWorkspace(a.path.clone()));
            }
            Some(crate::workspace::read_to_string_nofollow(&path)?)
        } else {
            None
        };
        let existed = before.is_some();
        self.transactions
            .with_active(|txn| {
                txn.stage_write_file(&a.path, &a.content, existed)?;
                if existed {
                    let syntax_root = txn.staged_overlay_path()?;
                    if let Err(e) = check_python_syntax(&syntax_root, &a.path) {
                        return Ok(ToolOutcome {
                            ok: false,
                            summary: format!("syntax error in {} after staged write", a.path),
                            content: format!("ERROR: {e}"),
                            error_kind: Some("syntax_error".into()),
                            change: Some(FileChange {
                                path: a.path.clone(),
                                kind: ChangeKind::Edit,
                                before: before.clone(),
                                after: Some(a.content.clone()),
                            }),
                        });
                    }
                }
                let preview = txn.preview();
                Ok(ToolOutcome::ok(
                    format!(
                        "staged {} {} ({} bytes)",
                        if existed { "overwrite" } else { "create" },
                        a.path,
                        a.content.len()
                    ),
                    format!(
                        "Staged write to {} (transaction {}). Patch digest: {}",
                        a.path, preview.transaction_id, preview.patch_digest.0
                    ),
                )
                .with_change(FileChange {
                    path: a.path.clone(),
                    kind: if existed {
                        ChangeKind::Edit
                    } else {
                        ChangeKind::Create
                    },
                    before,
                    after: Some(a.content),
                }))
            })
            .map_err(map_txn_err)
    }

    pub fn commit_staged_if_any(&self) -> Result<Option<lokai_domain::CommitResult>, ToolError> {
        self.transactions
            .commit_active_if_any(
                &format!("lokai:{}", std::process::id()),
                DataClass::RepositorySource,
            )
            .map_err(map_txn_err)
    }

    /// Discard staged workspace mutations without committing (R6-3 cancel/fail).
    pub fn abort_staged_if_any(&self) -> Result<bool, ToolError> {
        self.transactions.abort_active_if_any().map_err(map_txn_err)
    }

    pub fn verification_overlay_if_staged(&self) -> Result<Option<std::path::PathBuf>, ToolError> {
        self.transactions
            .verification_view_if_active()
            .map_err(map_txn_err)
    }

    pub fn finish_verification_run(
        &self,
        command: &str,
        success: bool,
        output: &str,
        exit_status: Option<i32>,
    ) -> Result<(), ToolError> {
        let base = match self
            .transactions
            .with_active_if_any(|txn| Ok(txn.base_version.clone()))
            .map_err(map_txn_err)?
        {
            Some(v) => v,
            None => return Ok(()),
        };
        let record = lokai_domain::VerificationRecord {
            command: command.to_string(),
            sandbox_policy: "constrained".into(),
            workspace_version: base,
            exit_status,
            output_digest: lokai_transaction::digest_string(output),
            success,
            unexpected_mutations: vec![],
        };
        self.transactions
            .finish_verification_if_active(record)
            .map_err(map_txn_err)
    }

    pub fn commit_staged(
        &self,
        data_class: DataClass,
    ) -> Result<lokai_domain::CommitResult, ToolError> {
        let result = self
            .transactions
            .with_active(|txn| txn.commit(&format!("lokai:{}", std::process::id()), data_class))
            .map_err(map_txn_err)?;
        self.transactions.clear_active();
        Ok(result)
    }

    pub fn preview(&self) -> Result<lokai_domain::TransactionPreview, ToolError> {
        self.transactions
            .with_active(|txn| Ok(txn.preview()))
            .map_err(map_txn_err)
    }

    pub fn preview_active_if_any(
        &self,
    ) -> Result<Option<lokai_domain::TransactionPreview>, ToolError> {
        self.transactions
            .with_active_if_any(|txn| Ok(txn.preview()))
            .map_err(map_txn_err)
    }

    pub fn bind_patch_approval(
        &self,
        approval: lokai_domain::PatchApproval,
    ) -> Result<(), ToolError> {
        self.transactions
            .with_active(|txn| txn.bind_approval(approval))
            .map_err(map_txn_err)
    }

    pub fn bind_effect_identity(
        &self,
        task_id: lokai_domain::TaskId,
        attempt_id: lokai_domain::AttemptId,
    ) -> Result<(), ToolError> {
        self.transactions
            .with_active_if_any(|txn| {
                txn.task_id = Some(task_id.clone());
                txn.attempt_id = Some(attempt_id.clone());
                Ok(())
            })
            .map(|_| ())
            .map_err(map_txn_err)
    }
}

fn map_txn_err(e: lokai_transaction::TransactionError) -> ToolError {
    let msg = e.to_string();
    if msg.contains("not found") || msg.contains("ambiguous") || msg.contains("NoMatch") {
        ToolError::NoMatch(msg)
    } else if msg.contains("outside workspace") {
        ToolError::OutsideWorkspace(msg)
    } else {
        ToolError::Other(msg)
    }
}

#[async_trait]
impl MutationSink for RepositoryMutationService {
    async fn apply_mutation(&self, authorized: &AuthorizedAction) -> ExecutionOutcome {
        let execution_id = ExecutionId::new(format!("mut_{}", authorized.action.action_id));
        let authorized = authorized.clone();
        let service = self.clone();
        match tokio::task::spawn_blocking(move || service.apply_mutation_sync(&authorized)).await {
            Ok(outcome) => outcome,
            Err(_) => ExecutionOutcome::Failed {
                execution_id,
                reason: "mutation sink task panicked".into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use lokai_domain::{
        ActionId, ActionKind, AgentId, AuthorizedAction, DataClass, IssuedCapability,
        ProposedAction, SessionId,
    };
    use lokai_transaction::{WorkspaceTransactionService, WorkspaceTxnConfig};

    use super::*;

    fn tmp_service(tag: &str) -> (RepositoryMutationService, Workspace, PathBuf) {
        let dir = std::env::temp_dir().join(format!("lokai-mut-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ws = Workspace::new(&dir).unwrap();
        let txn = Arc::new(
            WorkspaceTransactionService::new(&dir, WorkspaceTxnConfig::default()).unwrap(),
        );
        (RepositoryMutationService::new(txn), ws, dir)
    }

    struct DenyConsumer;
    impl CapabilityConsumer for DenyConsumer {
        fn authorize(&self, _: &AuthorizedAction) -> Result<(), lokai_domain::CapabilityError> {
            Err(lokai_domain::CapabilityError::ScopeMismatch)
        }
    }

    #[test]
    fn stale_base_rejected() {
        let (svc, ws, dir) = tmp_service("stale");
        std::fs::write(dir.join("a.txt"), "hello world").unwrap();
        let err = svc
            .edit_file(
                &ws,
                None,
                serde_json::json!({
                    "path": "a.txt",
                    "old_string": "missing",
                    "new_string": "x"
                }),
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::NoMatch(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn denied_capability_blocks_write() {
        let (svc, ws, dir) = tmp_service("deny");
        let svc = svc.with_capability_consumer(Arc::new(DenyConsumer));
        let wv = lokai_transaction::version::capture_workspace_version(&dir, &[]).unwrap();
        let action = ProposedAction {
            action_id: ActionId::new("mut_1"),
            session_id: SessionId::new("s"),
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: Some(AgentId::new("a0")),
            workspace_version: Some(wv.clone()),
            data_class: DataClass::RepositorySource,
            kind: ActionKind::WriteFile,
            parameters: lokai_domain::execution::CanonicalActionParameters {
                digest: "test".into(),
                executable_identity: None,
                resolved_path: Some("b.txt".into()),
                arguments: vec![],
                shell_identity: None,
                shell_mode: None,
                script_bytes: None,
                working_directory: Some(dir.display().to_string()),
                env_vars: None,
                stdin_source_classification: None,
                filesystem_access_scope: None,
                network_policy: None,
                resource_limits: None,
                process_class: None,
                sandbox_profile: None,
                expected_output_limits: None,
                schema_version: 1,
                tool_arguments: Some(serde_json::json!({ "path": "b.txt", "content": "x" })),
            },
            requested_capabilities: Default::default(),
            trace_context: Default::default(),
        };
        let authorized = AuthorizedAction {
            capability: IssuedCapability {
                capability_id: lokai_domain::CapabilityId::new("cap_bad"),
                session_id: action.session_id.clone(),
                run_id: None,
                task_id: None,
                attempt_id: None,
                agent_id: action.agent_id.clone(),
                action_kind: action.kind.clone(),
                canonical_parameter_digest: action.parameters.digest.clone(),
                workspace_version: Some(wv),
                data_classification: action.data_class,
                issuance_timestamp: 0,
                expiration: u64::MAX,
                max_use_count: 1,
                current_use_count: 0,
                issuing_policy_version: "v1".into(),
                approval_record_id: None,
                revoked: false,
            },
            action,
        };
        let err = svc
            .write_file(
                &ws,
                Some(&authorized),
                serde_json::json!({ "path": "b.txt", "content": "secret" }),
            )
            .unwrap_err();
        assert!(err.to_string().contains("denied"));
        assert!(!dir.join("b.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn workspace_change_after_approval_rejects() {
        let (svc, ws, dir) = tmp_service("wv-stale");
        struct AllowOnce;
        impl CapabilityConsumer for AllowOnce {
            fn authorize(&self, _: &AuthorizedAction) -> Result<(), lokai_domain::CapabilityError> {
                Ok(())
            }
        }
        let svc = svc.with_capability_consumer(Arc::new(AllowOnce));
        let path = WorkspacePath::new("tracked.txt");
        std::fs::write(dir.join("tracked.txt"), "v1").unwrap();
        let wv = lokai_transaction::version::capture_workspace_version(
            &dir,
            std::slice::from_ref(&path),
        )
        .unwrap();
        let action = ProposedAction {
            action_id: ActionId::new("mut_wv"),
            session_id: SessionId::new("s"),
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: Some(AgentId::new("a0")),
            workspace_version: Some(wv.clone()),
            data_class: DataClass::RepositorySource,
            kind: ActionKind::WriteFile,
            parameters: lokai_domain::execution::CanonicalActionParameters {
                digest: "test".into(),
                executable_identity: None,
                resolved_path: Some("tracked.txt".into()),
                arguments: vec![],
                shell_identity: None,
                shell_mode: None,
                script_bytes: None,
                working_directory: Some(dir.display().to_string()),
                env_vars: None,
                stdin_source_classification: None,
                filesystem_access_scope: None,
                network_policy: None,
                resource_limits: None,
                process_class: None,
                sandbox_profile: None,
                expected_output_limits: None,
                schema_version: 1,
                tool_arguments: Some(serde_json::json!({ "path": "tracked.txt", "content": "v2" })),
            },
            requested_capabilities: Default::default(),
            trace_context: Default::default(),
        };
        let authorized = AuthorizedAction {
            capability: IssuedCapability {
                capability_id: lokai_domain::CapabilityId::new("cap_wv"),
                session_id: action.session_id.clone(),
                run_id: None,
                task_id: None,
                attempt_id: None,
                agent_id: action.agent_id.clone(),
                action_kind: action.kind.clone(),
                canonical_parameter_digest: action.parameters.digest.clone(),
                workspace_version: Some(wv),
                data_classification: action.data_class,
                issuance_timestamp: 0,
                expiration: u64::MAX,
                max_use_count: 1,
                current_use_count: 0,
                issuing_policy_version: "v1".into(),
                approval_record_id: None,
                revoked: false,
            },
            action,
        };
        // Mutate workspace after approval / capability issue.
        std::fs::write(dir.join("tracked.txt"), "changed-underfoot").unwrap();
        let err = svc
            .write_file(
                &ws,
                Some(&authorized),
                serde_json::json!({ "path": "tracked.txt", "content": "v2" }),
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("workspace_version") || err.to_string().contains("denied"),
            "got {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn mutation_sink_none_version_does_not_commit() {
        let (svc, ws, dir) = tmp_service("sink-none");
        let action = ProposedAction {
            action_id: ActionId::new("mut_sink"),
            session_id: SessionId::new("s"),
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: Some(AgentId::new("a0")),
            workspace_version: None,
            data_class: DataClass::RepositorySource,
            kind: ActionKind::WriteFile,
            parameters: lokai_domain::execution::CanonicalActionParameters {
                digest: "test".into(),
                executable_identity: None,
                resolved_path: Some("c.txt".into()),
                arguments: vec![],
                shell_identity: None,
                shell_mode: None,
                script_bytes: None,
                working_directory: Some(dir.display().to_string()),
                env_vars: None,
                stdin_source_classification: None,
                filesystem_access_scope: None,
                network_policy: None,
                resource_limits: None,
                process_class: None,
                sandbox_profile: None,
                expected_output_limits: None,
                schema_version: 1,
                tool_arguments: Some(serde_json::json!({ "path": "c.txt", "content": "hello" })),
            },
            requested_capabilities: Default::default(),
            trace_context: Default::default(),
        };
        let authorized = AuthorizedAction {
            capability: IssuedCapability {
                capability_id: lokai_domain::CapabilityId::new("cap_sink"),
                session_id: action.session_id.clone(),
                run_id: None,
                task_id: None,
                attempt_id: None,
                agent_id: action.agent_id.clone(),
                action_kind: action.kind.clone(),
                canonical_parameter_digest: action.parameters.digest.clone(),
                workspace_version: action.workspace_version.clone(),
                data_classification: action.data_class,
                issuance_timestamp: 0,
                expiration: 0,
                max_use_count: 1,
                current_use_count: 0,
                issuing_policy_version: "v1".into(),
                approval_record_id: None,
                revoked: false,
            },
            action,
        };
        let outcome = svc.apply_mutation(&authorized).await;
        assert!(
            matches!(outcome, ExecutionOutcome::Completed { ok: false, .. }),
            "None workspace_version must not authorize, got {outcome:?}"
        );
        assert!(!dir.join("c.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = ws;
    }

    #[tokio::test]
    async fn mutation_sink_stages_then_commits() {
        let (svc, ws, dir) = tmp_service("sink");
        let wv = lokai_transaction::version::capture_workspace_version(&dir, &[]).unwrap();
        let action = ProposedAction {
            action_id: ActionId::new("mut_sink"),
            session_id: SessionId::new("s"),
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: Some(AgentId::new("a0")),
            workspace_version: Some(wv.clone()),
            data_class: DataClass::RepositorySource,
            kind: ActionKind::WriteFile,
            parameters: lokai_domain::execution::CanonicalActionParameters {
                digest: "test".into(),
                executable_identity: None,
                resolved_path: Some("c.txt".into()),
                arguments: vec![],
                shell_identity: None,
                shell_mode: None,
                script_bytes: None,
                working_directory: Some(dir.display().to_string()),
                env_vars: None,
                stdin_source_classification: None,
                filesystem_access_scope: None,
                network_policy: None,
                resource_limits: None,
                process_class: None,
                sandbox_profile: None,
                expected_output_limits: None,
                schema_version: 1,
                tool_arguments: Some(serde_json::json!({ "path": "c.txt", "content": "hello" })),
            },
            requested_capabilities: Default::default(),
            trace_context: Default::default(),
        };
        let authorized = AuthorizedAction {
            capability: IssuedCapability {
                capability_id: lokai_domain::CapabilityId::new("cap_sink"),
                session_id: action.session_id.clone(),
                run_id: None,
                task_id: None,
                attempt_id: None,
                agent_id: action.agent_id.clone(),
                action_kind: action.kind.clone(),
                canonical_parameter_digest: action.parameters.digest.clone(),
                workspace_version: action.workspace_version.clone(),
                data_classification: action.data_class,
                issuance_timestamp: 0,
                expiration: 0,
                max_use_count: 1,
                current_use_count: 0,
                issuing_policy_version: "v1".into(),
                approval_record_id: None,
                revoked: false,
            },
            action,
        };
        let outcome = svc.apply_mutation(&authorized).await;
        assert!(matches!(
            outcome,
            ExecutionOutcome::Completed { ok: true, .. }
        ));
        assert!(!dir.join("c.txt").exists());
        svc.commit_staged(DataClass::RepositorySource).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("c.txt")).unwrap(), "hello");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = ws;
    }
}
