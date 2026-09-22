//! Single-agent tool loop: model ? tools until finish or cap.

use std::sync::Arc;

use lokai_inference::{ChatRequest, FabricCallMeta, InferenceProvider, Message, ToolSchema};
use serde_json::Value;
use tetonic_domain::{
    finalize_parameters, prepare_proposed_action, ActionId, ActionKind, AgentInvocation,
    AuthorizedAction, CandidateOutcome, CompletionKind, ContextCompileRequest, ContextCompiler,
    LimitKind, ProposedAction, ToolHost, ToolOutcome,
};
use tetonic_policy::PolicyEngine;

use crate::config::AgentConfig;
use crate::context::ContextReport;
use crate::conversation::Conversation;
use crate::error::AgentError;
use crate::hooks::{
    AbortStaged, ApprovalHook, ApprovalRequest, AuditSink, CaptureWorkspaceVersion,
    PostEditSnapshot, ResolveUnderRoot, SpawnHook, SpawnRequest,
};
use crate::monitor::HeuristicMonitor;
use crate::step::{format_usage, Step};
use crate::tokenizer::{HeuristicTokenizer, Tokenizer};
use crate::turn::{TurnOpsEvent, TurnOpsHook, TurnState};

pub struct Agent {
    work_scope: tetonic_domain::work_scope::WorkScope,
    provider: Arc<dyn InferenceProvider>,
    tools: Box<dyn ToolHost>,
    schemas: Vec<ToolSchema>,
    /// Optional stable model-facing catalog. Execution authority remains in
    /// `schemas` and the filtered ToolHost, including managed capability checks.
    inference_catalog: Option<Vec<ToolSchema>>,
    turn_instructions: Option<String>,
    /// Precomputed token cost of the model-facing catalog.
    tools_token_count: usize,
    config: AgentConfig,
    tokenizer: Box<dyn Tokenizer>,
    audit: Option<Box<dyn AuditSink>>,
    approval: Option<ApprovalHook>,
    abort_staged: Option<AbortStaged>,
    post_edit_snapshot: Option<PostEditSnapshot>,
    resolve_under_root: Option<ResolveUnderRoot>,
    capture_workspace_version: Option<CaptureWorkspaceVersion>,
    spawn: Option<SpawnHook>,
    policy: Option<Arc<PolicyEngine>>,
    action_broker: Option<Arc<dyn tetonic_domain::sinks::ActionBroker>>,
    process_broker: Option<Arc<dyn tetonic_domain::sinks::ProcessBroker>>,
    turn_ops: Option<TurnOpsHook>,
    context_compiler: Option<Arc<dyn ContextCompiler>>,
}

// Thread safety must follow from every field's trait bounds; never override
// the compiler here when introducing a new host hook or capability.
#[test]
fn agent_is_send_and_sync_without_unsafe_overrides() {
    fn assert_thread_safe<T: Send + Sync>() {}
    assert_thread_safe::<Agent>();
}

impl Agent {
    /// Rebind an idle agent without changing tools, identity, approvals, policy,
    /// or conversation content. Exclusive borrowing prevents a concurrent turn.
    /// The runtime must supply an admitted provider.
    pub fn replace_inference(
        &mut self,
        conversation: &mut Conversation,
        binding: crate::AgentInferenceBinding,
    ) -> Result<(), crate::InvalidInferenceBinding> {
        if binding.model.is_empty()
            || binding
                .model
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
            || binding.num_ctx <= self.config.context_reserve
            || binding.num_ctx > u32::MAX as usize
        {
            return Err(crate::InvalidInferenceBinding);
        }
        let tools_token_count = binding
            .tokenizer
            .count(&serde_json::to_string(self.inference_schemas()).unwrap_or_default());
        conversation.invalidate_token_counts();
        self.provider = binding.provider;
        self.config.model = binding.model;
        self.config.num_ctx = binding.num_ctx;
        self.config.model_tier = None;
        self.config.draft_model = None;
        self.config.draft_count = None;
        self.tokenizer = binding.tokenizer;
        self.tools_token_count = tools_token_count;
        Ok(())
    }
    /// Stamp managed Infer correlation onto existing `AgentConfig` fields.
    /// Not an `AgentIdentity` store.
    pub fn stamp_managed_run(&mut self, run_id: &str, task_id: &str, attempt_id: &str) {
        self.config.run_id = Some(run_id.to_string());
        self.config.task_id = Some(task_id.to_string());
        self.config.attempt_id = Some(attempt_id.to_string());
    }

    pub fn managed_run_id(&self) -> Option<&str> {
        self.config.run_id.as_deref()
    }

    pub fn stamp_attempt_id(&mut self, attempt_id: &str) {
        self.config.attempt_id = Some(attempt_id.to_string());
    }

    pub fn bound_attempt_id(&self) -> Option<&str> {
        self.config.attempt_id.as_deref()
    }

    pub fn execution_agent_id(&self) -> &str {
        &self.config.agent_id
    }

    pub fn execution_role(&self) -> Option<&str> {
        self.config.specialist_role.as_deref()
    }

    pub fn execution_step_limit(&self) -> usize {
        self.config.max_steps
    }

    /// Executable capabilities for managed admission, never the broader wire catalog.
    pub fn advertised_tool_names(&self) -> Vec<String> {
        self.schemas
            .iter()
            .map(|s| s.function.name.clone())
            .collect()
    }

    fn inference_schemas(&self) -> &[ToolSchema] {
        self.inference_catalog.as_deref().unwrap_or(&self.schemas)
    }

    /// Keep the wire catalog stable across agents sharing a conversation, without
    /// granting any additional executable capabilities. The product supplies
    /// turn-local instructions; the core records them in append-only history.
    pub fn with_stable_tool_catalog(
        mut self,
        mut catalog: Vec<ToolSchema>,
        instructions: String,
    ) -> Result<Self, AgentError> {
        catalog.sort_by(|a, b| a.function.name.cmp(&b.function.name));
        if instructions.trim().is_empty()
            || catalog
                .windows(2)
                .any(|w| w[0].function.name == w[1].function.name)
            || self.schemas.iter().any(|active| {
                !catalog.iter().any(|entry| {
                    entry.function.name == active.function.name
                        && serde_json::to_value(entry).ok() == serde_json::to_value(active).ok()
                })
            })
        {
            return Err(AgentError::Capability(
                "invalid stable inference tool catalog".into(),
            ));
        }
        self.tools_token_count = self
            .tokenizer
            .count(&serde_json::to_string(&catalog).unwrap_or_default());
        self.inference_catalog = Some(catalog);
        self.turn_instructions = Some(instructions);
        Ok(self)
    }

    pub fn placeholder() -> Self {
        Self::new(
            Arc::new(NoopInferenceProvider),
            EmptyToolHost,
            AgentConfig::default(),
        )
    }

    pub fn new(
        provider: Arc<dyn InferenceProvider>,
        tools: impl ToolHost + 'static,
        config: AgentConfig,
    ) -> Self {
        Self::with_tokenizer(provider, tools, config, Box::new(HeuristicTokenizer))
    }
}

impl Default for Agent {
    fn default() -> Self {
        Self::placeholder()
    }
}

#[derive(Clone, Default)]
pub struct NoopInferenceProvider;

#[async_trait::async_trait]
impl InferenceProvider for NoopInferenceProvider {
    async fn chat(
        &self,
        _req: ChatRequest,
        _on_token: &mut lokai_inference::TokenSink<'_>,
    ) -> Result<lokai_inference::ChatResponse, lokai_inference::InferenceError> {
        Err(lokai_inference::InferenceError::Provider(
            "noop provider".into(),
        ))
    }
}

#[derive(Clone, Default)]
pub struct EmptyToolHost;

impl ToolHost for EmptyToolHost {
    fn clone_box(&self) -> Box<dyn ToolHost> {
        Box::new(self.clone())
    }
    fn propose(
        &self,
        _name: &str,
        _args: &Value,
    ) -> Option<tetonic_domain::tool_host::ToolProposal> {
        None
    }
    fn is_tool_allowed(&self, _name: &str) -> bool {
        false
    }
    fn is_read_only(&self, _name: &str) -> bool {
        true
    }
    fn advertisements(&self) -> Vec<tetonic_domain::tool_host::ToolAdvertisement> {
        Vec::new()
    }
    fn validate_tool_args(&self, _name: &str, _args: &Value) -> Result<(), String> {
        Ok(())
    }
    fn execute_authorized(
        &self,
        name: &str,
        _args: &Value,
        _auth: Option<&AuthorizedAction>,
        _cancel: &tetonic_domain::work_scope::CancellationSignal,
    ) -> ToolOutcome {
        ToolOutcome {
            ok: false,
            summary: format!("{name} not supported"),
            content: "empty host has no tools".into(),
            error_kind: Some("not_found".into()),
            change: None,
        }
    }
}

impl Agent {
    pub fn with_tokenizer(
        provider: Arc<dyn InferenceProvider>,
        tools: impl ToolHost + 'static,
        config: AgentConfig,
        tokenizer: Box<dyn Tokenizer>,
    ) -> Self {
        let tools: Box<dyn ToolHost> = Box::new(tools);
        let mut schemas: Vec<ToolSchema> = tools
            .advertisements()
            .into_iter()
            .map(|d| ToolSchema::function(&d.name, &d.description, d.parameters))
            .collect();
        schemas.sort_by(|a, b| a.function.name.cmp(&b.function.name));
        let tools_token_count =
            tokenizer.count(&serde_json::to_string(&schemas).unwrap_or_default());
        Self {
            provider,
            tools,
            schemas,
            inference_catalog: None,
            turn_instructions: None,
            tools_token_count,
            config,
            tokenizer,
            audit: None,
            approval: None,
            abort_staged: None,
            post_edit_snapshot: None,
            resolve_under_root: None,
            capture_workspace_version: None,
            spawn: None,
            policy: None,
            action_broker: None,
            process_broker: None,
            turn_ops: None,
            context_compiler: None,
            work_scope: Default::default(),
        }
    }

    /// Persist operational turn state for crash recovery (AC2-6).
    pub fn with_turn_ops(mut self, hook: TurnOpsHook) -> Self {
        self.turn_ops = Some(hook);
        self
    }

    pub fn with_context_compiler(mut self, compiler: Arc<dyn ContextCompiler>) -> Self {
        self.context_compiler = Some(compiler);
        self
    }

    /// Whether production context compilation is attached (R4-1).
    pub fn has_context_compiler(&self) -> bool {
        self.context_compiler.is_some()
    }

    fn persist_turn_state(&self, convo: &Conversation, state: TurnState, payload: Option<String>) {
        if let Some(ref ops) = self.turn_ops {
            if let (Some(sid), Some(tid)) = (self.config.session_id.as_deref(), convo.turn_id()) {
                ops(TurnOpsEvent::Persist {
                    session_id: sid.to_string(),
                    turn_id: tid.to_string(),
                    state,
                    payload,
                });
            }
        }
    }

    /// Attach a best-effort audit sink (records sessions/messages/tool calls).
    pub fn with_audit(mut self, sink: Box<dyn AuditSink>) -> Self {
        self.audit = Some(sink);
        self
    }

    /// Attach an async approval gate for tools the host marks as
    /// `requires_user_approval`. With a hook set, the agent asks before running
    /// those tools; without one it defers to the tool layer's own policy. See
    /// [`ApprovalHook`].
    pub fn with_approval(mut self, hook: ApprovalHook) -> Self {
        self.approval = Some(hook);
        self
    }

    /// Optional staged-mutation abort (composition; not `ToolHost`).
    pub fn with_abort_staged(mut self, hook: AbortStaged) -> Self {
        self.abort_staged = Some(hook);
        self
    }

    /// Optional post-edit snapshot formatter. Missing hook appends nothing.
    pub fn with_post_edit_snapshot(mut self, hook: PostEditSnapshot) -> Self {
        self.post_edit_snapshot = Some(hook);
        self
    }

    /// Optional resolved-path length hook for the size gate. Missing hook skips it.
    pub fn with_resolve_under_root(mut self, hook: ResolveUnderRoot) -> Self {
        self.resolve_under_root = Some(hook);
        self
    }

    /// Optional workspace-version capture. Each call site interprets `Result` itself.
    pub fn with_capture_workspace_version(mut self, hook: CaptureWorkspaceVersion) -> Self {
        self.capture_workspace_version = Some(hook);
        self
    }

    /// Attach an async spawn hook. The invocation table names the tool.
    pub fn with_spawn(mut self, hook: SpawnHook) -> Self {
        self.spawn = Some(hook);
        self
    }

    /// Attach policy engine for tool allow/deny before execution (SEC-016).
    pub fn with_policy(mut self, policy: Arc<PolicyEngine>) -> Self {
        self.policy = Some(policy);
        self
    }

    pub fn with_action_broker(
        mut self,
        broker: Arc<dyn tetonic_domain::sinks::ActionBroker>,
    ) -> Self {
        self.action_broker = Some(broker);
        self
    }

    pub fn with_process_broker(
        mut self,
        broker: Arc<dyn tetonic_domain::sinks::ProcessBroker>,
    ) -> Self {
        self.process_broker = Some(broker);
        self
    }

    /// Bind in-process worker ownership supplied by the attempt lifecycle owner.
    pub fn bind_work_scope(
        &mut self,
        scope: tetonic_domain::work_scope::WorkScope,
    ) -> Result<(), AgentError> {
        if !self.work_scope.is_quiescent() {
            return Err(AgentError::Capability(
                "previous attempt still has active workers".into(),
            ));
        }
        self.work_scope = scope;
        Ok(())
    }

    /// Run synchronous tool work on Tokio's blocking pool (keeps async workers free).
    async fn run_tools_blocking<F, R>(&self, f: F) -> Result<R, AgentError>
    where
        F: FnOnce(&dyn ToolHost) -> R + Send + 'static,
        R: Send + 'static,
    {
        let lease = self
            .work_scope
            .try_enter()
            .ok_or_else(|| AgentError::Capability("attempt is canceled".into()))?;
        let scope = self.work_scope.clone();
        let tools = self.tools.clone_box();
        tokio::task::spawn_blocking(move || {
            let _lease = lease;
            if scope.is_canceled() {
                return Err(AgentError::Capability("attempt is canceled".into()));
            }
            Ok(f(&*tools))
        })
        .await
        .map_err(|_| AgentError::ToolExecutionPanicked)?
    }

    /// Discard uncommitted staged workspace mutations (R6-3 cancel/fail).
    async fn abort_staged_mutations(&self) {
        if let Some(hook) = &self.abort_staged {
            let hook = hook.clone();
            let _ = tokio::task::spawn_blocking(move || hook()).await;
        }
    }

    /// Route a tool call through the approval gate when it is gated and a hook
    /// is present. A denial becomes a `denied` outcome (so the model can
    /// self-correct); otherwise we run the tool as usual.
    async fn execute_gated(&self, name: &str, args: &Value, call_id: &str) -> ToolOutcome {
        if !self.tools.is_tool_allowed(name) {
            return self.run_tool_execute(name, args, None).await;
        }
        if self.tools.requires_action_broker(name) && self.action_broker.is_none() {
            return ToolOutcome {
                ok: false,
                summary: format!("{name} denied"),
                content: format!("ERROR: capability required for '{name}' (no action broker)"),
                error_kind: Some("denied".into()),
                change: None,
            };
        }
        let broker_handles_approval = self.action_broker.is_some();
        if !broker_handles_approval && self.tools.requires_user_approval(name) {
            if let Some(hook) = &self.approval {
                let is_shell = self
                    .tools
                    .propose(name, args)
                    .map(|p| p.kind == ActionKind::ExecuteShell)
                    .unwrap_or(false);
                let req = ApprovalRequest {
                    call_id: call_id.to_string(),
                    kind: name.to_string(),
                    tool: name.to_string(),
                    args: args.clone(),
                    attempt_id: self.config.attempt_id.clone(),
                    ..Default::default()
                };
                tetonic_telemetry::fault::inject_fault("during_approval_wait");
                if !hook(req).await {
                    let (summary, content) = if is_shell {
                        (
                            format!("{name} denied by user"),
                            "ERROR: shell command not approved (denied by user)".to_string(),
                        )
                    } else {
                        (
                            format!("{name} denied by user"),
                            format!("ERROR: {name} not approved (denied by user)"),
                        )
                    };
                    return ToolOutcome {
                        ok: false,
                        summary,
                        content,
                        error_kind: Some("denied".into()),
                        change: None,
                    };
                }
            }
        }
        let auth = match self.issue_sink_capability(name, args, call_id).await {
            Ok(auth) => auth,
            Err(outcome) => return *outcome,
        };
        self.run_tool_execute(name, args, auth.as_ref()).await
    }

    async fn run_tool_execute(
        &self,
        name: &str,
        args: &Value,
        auth: Option<&AuthorizedAction>,
    ) -> ToolOutcome {
        let _tool_stage = tetonic_telemetry::enter_stage_child("tool");
        let name = name.to_string();
        let args = args.clone();
        let auth = auth.cloned();
        let name_for_err = name.clone();
        let cancel = self.work_scope.cancellation_signal();
        match self
            .run_tools_blocking(move |tools| {
                tools.execute_authorized(&name, &args, auth.as_ref(), &cancel)
            })
            .await
        {
            Ok(outcome) => outcome,
            Err(AgentError::ToolExecutionPanicked) => ToolOutcome {
                ok: false,
                summary: format!("{name_for_err} failed"),
                content: "ERROR: tool execution task panicked".into(),
                error_kind: Some("internal".into()),
                change: None,
            },
            Err(e) => ToolOutcome {
                ok: false,
                summary: format!("{name_for_err} denied"),
                content: format!("ERROR: {e}"),
                error_kind: Some("denied".into()),
                change: None,
            },
        }
    }

    async fn issue_sink_capability(
        &self,
        name: &str,
        args: &Value,
        call_id: &str,
    ) -> Result<Option<AuthorizedAction>, Box<ToolOutcome>> {
        let Some(proposal) = self.tools.propose(name, args) else {
            return Ok(None);
        };
        let kind = proposal.kind;
        let Some(ref broker) = self.action_broker else {
            return Ok(None);
        };
        let is_shell = kind == ActionKind::ExecuteShell;
        let is_process_action = matches!(
            kind,
            ActionKind::ExecuteShell
                | ActionKind::ExecuteProcess
                | ActionKind::GitOperation
                | ActionKind::StartInternalService
        );
        let workspace_root = self.config.workspace_root.clone();
        let working_directory = if is_process_action {
            let candidate = self
                .config
                .process_working_directory
                .as_ref()
                .or(workspace_root.as_ref());
            let Some(dir) = candidate else {
                return Err(Box::new(ToolOutcome {
                    ok: false,
                    summary: format!("{name} denied"),
                    content: "ERROR: process working directory is missing".into(),
                    error_kind: Some("denied".into()),
                    change: None,
                }));
            };
            if dir.as_os_str().is_empty() || !dir.is_absolute() {
                return Err(Box::new(ToolOutcome {
                    ok: false,
                    summary: format!("{name} denied"),
                    content: "ERROR: process working directory must be a non-empty absolute path"
                        .into(),
                    error_kind: Some("denied".into()),
                    change: None,
                }));
            }
            Some(dir.display().to_string())
        } else {
            workspace_root
                .as_ref()
                .map(|root| root.display().to_string())
        };
        let path = proposal.resolved_path;
        let params = finalize_parameters(
            &kind,
            tetonic_domain::execution::CanonicalActionParameters {
                digest: String::new(),
                executable_identity: None,
                resolved_path: path.clone(),
                arguments: vec![],
                shell_identity: if is_shell {
                    Some(default_shell_identity())
                } else {
                    None
                },
                shell_mode: if is_shell {
                    Some("one_shot".into())
                } else {
                    None
                },
                script_bytes: if is_shell {
                    Some(
                        args.get("command")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .as_bytes()
                            .to_vec(),
                    )
                } else {
                    None
                },
                working_directory: working_directory.clone(),
                env_vars: None,
                stdin_source_classification: None,
                filesystem_access_scope: None,
                network_policy: None,
                resource_limits: None,
                process_class: if is_shell {
                    Some(tetonic_domain::execution::ProcessClass::ModelRequestedShell)
                } else {
                    None
                },
                sandbox_profile: None,
                expected_output_limits: None,
                schema_version: tetonic_domain::CANONICAL_SCHEMA_VERSION,
                tool_arguments: Some(args.clone()),
            },
        );
        let relevant: Vec<tetonic_domain::WorkspacePath> = path
            .as_ref()
            .map(|p| tetonic_domain::WorkspacePath::new(p.clone()))
            .into_iter()
            .collect();
        let workspace_version = match &workspace_root {
            Some(root) => match &self.capture_workspace_version {
                Some(hook) => Some(hook(root, &relevant).map_err(|e| {
                    Box::new(ToolOutcome {
                        ok: false,
                        summary: format!("{name} denied"),
                        content: format!("ERROR: workspace version: {e}"),
                        error_kind: Some("denied".into()),
                        change: None,
                    })
                })?),
                None => None,
            },
            None => None,
        };
        let action = prepare_proposed_action(ProposedAction {
            action_id: ActionId::new(format!("mut_{call_id}")),
            session_id: tetonic_domain::ids::SessionId::new(
                self.config.session_id.as_deref().unwrap_or("local"),
            ),
            run_id: self
                .config
                .run_id
                .as_deref()
                .map(tetonic_domain::ids::RunId::new),
            task_id: self
                .config
                .task_id
                .as_deref()
                .map(tetonic_domain::ids::TaskId::new),
            attempt_id: self
                .config
                .attempt_id
                .as_deref()
                .map(tetonic_domain::ids::AttemptId::new),
            agent_id: Some(tetonic_domain::ids::AgentId::new(&self.config.agent_id)),
            workspace_version,
            data_class: self.config.data_class,
            kind,
            parameters: params,
            requested_capabilities: Default::default(),
            trace_context: Default::default(),
        });

        let capability = broker.evaluate_and_issue(&action).await.map_err(|e| {
            Box::new(ToolOutcome {
                ok: false,
                summary: format!("{name} denied"),
                content: format!("ERROR: {e}"),
                error_kind: Some("denied".into()),
                change: None,
            })
        })?;
        Ok(Some(AuthorizedAction { capability, action }))
    }

    fn audit(&self) -> Option<&dyn AuditSink> {
        self.audit.as_deref()
    }

    fn message_tokens(&self, m: &Message) -> usize {
        // Memoized per message: a message is immutable once pushed, so we count it
        // once and reuse the result every subsequent turn instead of re-tokenizing
        // the whole history (which was O(n²) over a session).
        m.cached_tokens(|| {
            // content + any tool-call payload + a small per-message envelope overhead.
            let mut t = self.tokenizer.count(&m.content) + 4;
            if let Some(tc) = &m.tool_calls {
                if let Ok(s) = serde_json::to_string(tc) {
                    t += self.tokenizer.count(&s);
                }
            }
            t
        })
    }

    fn budget(&self) -> usize {
        self.config
            .num_ctx
            .saturating_sub(self.config.context_reserve)
    }

    /// Token cost of the tool schemas. The schemas are fixed for the agent's life,
    /// so this is computed once at construction and returned from cache here.
    fn tools_tokens(&self) -> usize {
        self.tools_token_count
    }

    /// Build the prompt as **protected prefix + volatile suffix (most recent
    /// conversation that fits)**. The protected prefix is `messages[0..prefix_len]`
    /// — the system prompt only — and is never trimmed. Compaction summaries
    /// live in the volatile suffix and may be dropped under budget pressure
    /// (SEC2-E2-020).
    fn build_context(
        &self,
        messages: &[Message],
        prefix_len: usize,
    ) -> (Vec<Message>, ContextReport) {
        let prefix = &messages[..prefix_len];
        let system_tokens: usize = prefix.iter().map(|m| self.message_tokens(m)).sum();
        let tools_tokens = self.tools_tokens();

        let budget = self.budget();
        let avail = budget.saturating_sub(system_tokens + tools_tokens);

        // Keep the most recent messages (after the prefix) that fit; always keep
        // at least the last one so the model has something to act on.
        let mut kept_rev: Vec<Message> = Vec::new();
        let mut convo_tokens = 0usize;
        for m in messages[prefix_len..].iter().rev() {
            let t = self.message_tokens(m);
            if convo_tokens + t > avail && !kept_rev.is_empty() {
                break;
            }
            convo_tokens += t;
            kept_rev.push(m.clone());
        }
        kept_rev.reverse();

        let dropped = (messages.len() - prefix_len) - kept_rev.len();
        let sent = if dropped == 0 {
            messages.to_vec()
        } else {
            let mut s = Vec::with_capacity(kept_rev.len() + prefix_len);
            s.extend_from_slice(prefix);
            s.extend(kept_rev);
            s
        };

        let report = ContextReport {
            system_tokens,
            tools_tokens,
            conversation_tokens: convo_tokens,
            total_tokens: system_tokens + tools_tokens + convo_tokens,
            budget,
            dropped_messages: dropped,
            estimated: self.tokenizer.estimated(),
            data_class: Some(self.config.data_class),
        };
        (sent, report)
    }

    /// Claude-Code-style auto-compaction. When the full working set crosses the
    /// configured fraction of the budget, summarize everything between the
    /// protected prefix and the last `keep_recent` turns into a single brief,
    /// then splice the history down to `[system, summary, ...recent]`. The
    /// summary stays in the volatile suffix (`prefix_len` remains 1).
    /// Returns the number of messages folded into the summary (if compaction happened).
    async fn maybe_compact(
        &self,
        messages: &mut Vec<Message>,
        prefix_len: &mut usize,
        compaction_threshold: f32,
        compaction_prompt: Option<&str>,
    ) -> anyhow::Result<Option<usize>> {
        let total: usize = messages
            .iter()
            .map(|m| self.message_tokens(m))
            .sum::<usize>()
            + self.tools_tokens();
        let threshold = (self.budget() as f32 * compaction_threshold) as usize;
        if total <= threshold {
            return Ok(None);
        }
        let Some(compaction_prompt) = compaction_prompt else {
            return Ok(None);
        };

        // Region to fold: everything after the system prompt, except the most recent turns.
        let start = 1usize.max(*prefix_len);
        let keep_recent = self.config.keep_recent;
        if messages.len() <= start + keep_recent {
            // Not enough older history to compact; build_context will trim.
            return Ok(None);
        }
        let end = messages.len() - keep_recent;

        let existing_summary = messages
            .get(1)
            .filter(|m| m.role == "assistant" && m.content.contains("[Summary of earlier work"))
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let to_fold: Vec<Message> = messages[start..end].to_vec();
        let summary = self
            .summarize(&existing_summary, &to_fold, compaction_prompt)
            .await?;

        let system = messages[0].clone();
        let recent: Vec<Message> = messages[end..].to_vec();
        let mut rebuilt = Vec::with_capacity(2 + recent.len());
        rebuilt.push(system);
        rebuilt.push(Message::assistant(format!(
            "[Summary of earlier work in this session]\n{summary}"
        )));
        rebuilt.extend(recent);

        *messages = rebuilt;
        *prefix_len = 1;
        Ok(Some(to_fold.len()))
    }

    /// Summarize folded turns (plus any prior summary) into a terse brief, using a
    /// non-streaming, tool-free model call.
    async fn summarize(
        &self,
        prior: &str,
        msgs: &[Message],
        compaction_prompt: &str,
    ) -> anyhow::Result<String> {
        let mut transcript = String::new();
        if !prior.is_empty() {
            transcript.push_str("Prior summary:\n");
            transcript.push_str(prior);
            transcript.push_str("\n\n");
        }
        for m in msgs {
            let body = if m.content.trim().is_empty() {
                m.tool_calls
                    .as_ref()
                    .and_then(|tc| serde_json::to_string(tc).ok())
                    .unwrap_or_default()
            } else {
                m.content.clone()
            };
            transcript.push_str(&format!("[{}] {}\n", m.role, body));
        }

        let sys = Message::system(compaction_prompt);
        let user = Message::user(format!("Summarize the session so far:\n\n{transcript}"));

        let mut noop = |_: &str| {};
        let resp = self
            .provider
            .chat(
                ChatRequest {
                    model: self.config.model.clone(),
                    model_digest: None,
                    messages: vec![sys, user],
                    tools: Vec::new(),
                    temperature: 0.2,
                    num_ctx: Some(self.config.num_ctx as u32),
                    draft_model: self.config.draft_model.clone(),
                    draft_count: self.config.draft_count,
                    keep_alive: None,
                    fabric: Some(FabricCallMeta {
                        session_id: self.config.session_id.clone(),
                        agent_id: Some(self.config.agent_id.clone()),
                        data_class: self.config.data_class,
                        disclosure_tier: self.config.disclosure_tier,
                        model_tier: self.config.model_tier.clone(),
                        ..Default::default()
                    }),
                    response_format: None,
                    outbound_scan: Default::default(),
                },
                &mut noop,
            )
            .await?;
        Ok(resp.message.content)
    }

    /// Take one user turn within an ongoing [`Conversation`], invoking `on_step`
    /// for each event. The conversation's history is preserved for the next turn,
    /// so repeated calls form a multi-turn chat that remembers prior context.
    /// Compiled `invocation.instructions` are seeded once, on the first turn.
    #[tracing::instrument(skip_all, name = "agent.turn", fields(conversation_len = convo.messages.len()))]
    pub async fn turn<F>(
        &self,
        convo: &mut Conversation,
        invocation: AgentInvocation,
        on_step: F,
    ) -> CandidateOutcome
    where
        F: FnMut(Step) + Send,
    {
        if invocation.instructions.trim().is_empty() {
            return CandidateOutcome::Failed {
                message: "missing instructions".into(),
            };
        }
        let checkpoint = convo.checkpoint();
        let outcome = self.turn_inner(convo, invocation, on_step).await;
        match &outcome {
            CandidateOutcome::Failed { .. } | CandidateOutcome::Canceled { .. } => {
                convo.rollback_to(checkpoint);
                self.abort_staged_mutations().await;
            }
            CandidateOutcome::Limited { .. } => {
                self.abort_staged_mutations().await;
            }
            CandidateOutcome::Completed { .. } => {}
        }
        outcome
    }

    async fn turn_inner<F>(
        &self,
        convo: &mut Conversation,
        invocation: AgentInvocation,
        mut on_step: F,
    ) -> CandidateOutcome
    where
        F: FnMut(Step) + Send,
    {
        let user_input = invocation.user_input.as_str();
        // Propagate trace context to child spans; keep agent config business ids in sync.
        if let Some(parent_ctx) = tetonic_telemetry::extract_context() {
            let mut child = parent_ctx.child();
            if child.session_id.is_none() {
                child.session_id = self.config.session_id.clone();
            }
            if child.run_id.is_none() {
                child.run_id = self.config.run_id.clone();
            }
            if child.task_id.is_none() {
                child.task_id = self.config.task_id.clone();
            }
            if child.attempt_id.is_none() {
                child.attempt_id = self.config.attempt_id.clone();
            }
            tetonic_telemetry::inject_context(child);
        } else if let (Some(sid), Some(rid)) = (&self.config.session_id, &self.config.run_id) {
            tetonic_telemetry::inject_turn_context(sid, rid, self.config.task_id.as_deref());
        }
        let fabric_trace = tetonic_telemetry::extract_context()
            .map(|trace| tetonic_domain::TraceContext {
                trace_id: trace.trace_id.to_string(),
                span_id: trace.span_id.to_string(),
                run_id: self
                    .config
                    .run_id
                    .clone()
                    .or(trace.run_id.clone())
                    .map(tetonic_domain::RunId::new),
                task_id: self
                    .config
                    .task_id
                    .clone()
                    .or(trace.task_id.clone())
                    .map(tetonic_domain::TaskId::new),
                attempt_id: self
                    .config
                    .attempt_id
                    .clone()
                    .or(trace.attempt_id.clone())
                    .map(tetonic_domain::AttemptId::new),
                scheduler_decision_id: trace.scheduler_decision_id.clone(),
            })
            .unwrap_or_else(|| tetonic_domain::TraceContext {
                run_id: self.config.run_id.clone().map(tetonic_domain::RunId::new),
                task_id: self.config.task_id.clone().map(tetonic_domain::TaskId::new),
                attempt_id: self
                    .config
                    .attempt_id
                    .clone()
                    .map(tetonic_domain::AttemptId::new),
                ..Default::default()
            });
        let mut compiled_context_data_class = None;
        let mut compiled_workspace_version =
            if let Some(inherited) = self.config.inherited_workspace_version.clone() {
                Some(inherited)
            } else {
                match (&self.config.workspace_root, &self.capture_workspace_version) {
                    (Some(root), Some(hook)) => hook(root, &[]).ok(),
                    _ => None,
                }
            };
        let mut compiled_run_id = None;
        let mut compiled_task_id = None;
        if convo.messages.is_empty() {
            let mut system_prompt = invocation.instructions.clone();
            if let Some(compiler) = &self.context_compiler {
                let workspace_root = match &self.config.workspace_root {
                    Some(root) => root.clone(),
                    None => match std::env::current_dir() {
                        Ok(root) => root,
                        Err(error) => {
                            return CandidateOutcome::Failed {
                                message: format!("resolve workspace for context: {error}"),
                            };
                        }
                    },
                };
                let workspace_version = match &compiled_workspace_version {
                    Some(version) => version.clone(),
                    None => match &self.capture_workspace_version {
                        Some(hook) => match hook(&workspace_root, &[]) {
                            Ok(version) => version,
                            Err(error) => {
                                return CandidateOutcome::Failed {
                                    message: format!("capture workspace for context: {error}"),
                                };
                            }
                        },
                        None => {
                            return CandidateOutcome::Failed {
                                message: "capture workspace for context: no capture hook".into(),
                            };
                        }
                    },
                };
                let req = ContextCompileRequest {
                    session_id: tetonic_domain::ids::SessionId::new(
                        self.config
                            .session_id
                            .clone()
                            .unwrap_or_else(|| "default_session".into()),
                    ),
                    run_id: tetonic_domain::ids::RunId::new(
                        self.config
                            .run_id
                            .clone()
                            .unwrap_or_else(|| "local_run".into()),
                    ),
                    task_id: tetonic_domain::ids::TaskId::new(
                        self.config
                            .task_id
                            .clone()
                            .or_else(|| convo.turn_id().map(String::from))
                            .unwrap_or_else(|| "local_task".into()),
                    ),
                    objective: user_input.to_string(),
                    workspace_version,
                    data_class_ceiling: self.config.data_class,
                };
                if let Ok(pack) = compiler.compile(req).await {
                    compiled_context_data_class = Some(pack.data_class);
                    compiled_workspace_version = Some(pack.workspace_version.clone());
                    compiled_run_id = Some(pack.run_id.to_string());
                    compiled_task_id = Some(pack.task_id.to_string());
                    system_prompt.push_str("\n\n## Context Evidence\n");
                    for ev in pack.evidence {
                        if let Some(path) = &ev.repository_path {
                            system_prompt.push_str(&format!(
                                "### {} (from {})\n{}\n\n",
                                ev.evidence_id, path, ev.text
                            ));
                        } else {
                            system_prompt
                                .push_str(&format!("### {}\n{}\n\n", ev.evidence_id, ev.text));
                        }
                    }
                    if let Some(a) = self.audit() {
                        a.note("compiled context pack injected");
                    }
                }
            }
            if let Some(a) = self.audit() {
                a.message("system", &system_prompt, None);
            }
            convo.messages.push(Message::system(system_prompt));
        }
        if let Some(a) = self.audit() {
            a.message("user", user_input, None);
        }
        convo.messages.push(Message::user(user_input.to_string()));
        if let Some(instructions) = &self.turn_instructions {
            // Do not rewrite old role instructions: doing so changes the prefix
            // and makes the backend reprocess all subsequent conversation tokens.
            if let Some(a) = self.audit() {
                a.message("system", instructions, None);
            }
            convo.messages.push(Message::system(instructions.clone()));
        }
        tetonic_telemetry::fault::inject_fault("before_model_request_persistence");
        self.persist_turn_state(convo, TurnState::Generating, None);

        struct ClearTurnOps {
            hook: Option<TurnOpsHook>,
            session_id: String,
            turn_id: String,
        }
        impl Drop for ClearTurnOps {
            fn drop(&mut self) {
                if let Some(ref hook) = self.hook {
                    hook(TurnOpsEvent::Clear {
                        session_id: self.session_id.clone(),
                        turn_id: self.turn_id.clone(),
                    });
                }
            }
        }
        let _turn_ops_guard =
            self.config
                .session_id
                .as_ref()
                .zip(convo.turn_id())
                .map(|(sid, tid)| ClearTurnOps {
                    hook: self.turn_ops.clone(),
                    session_id: sid.clone(),
                    turn_id: tid.to_string(),
                });

        let explain_turn = invocation.explain_turn;
        let compaction_threshold = if explain_turn {
            0.92
        } else {
            self.config.compaction_threshold
        };

        let mut monitor = HeuristicMonitor::new(&self.config, &invocation.discipline);

        let mut step_index: u32 = 0;
        let mut steps_used: u32 = 0;
        let max_steps = invocation.max_steps.min(self.config.max_steps);

        'steps: for _ in 0..max_steps {
            steps_used += 1;
            if convo.is_canceled() {
                self.abort_staged_mutations().await;
                on_step(Step::Stopped("canceled".into()));
                return CandidateOutcome::Canceled {
                    reason: "canceled".into(),
                };
            }
            if !explain_turn {
                match self
                    .maybe_compact(
                        &mut convo.messages,
                        &mut convo.prefix_len,
                        compaction_threshold,
                        invocation.discipline.compaction_system_prompt.as_deref(),
                    )
                    .await
                {
                    Ok(Some(folded)) => {
                        let note = format!(
                            "auto-compacted {folded} older messages into a running summary"
                        );
                        if let Some(a) = self.audit() {
                            a.note(&note);
                        }
                        on_step(Step::Note(note));
                    }
                    Ok(None) => {}
                    Err(error) => {
                        return CandidateOutcome::Failed {
                            message: error.to_string(),
                        };
                    }
                }
            }

            let context_timer =
                tetonic_telemetry::StageTimer::start(tetonic_telemetry::PerfStage::Context);
            let (sent, report) = self.build_context(&convo.messages, convo.prefix_len);
            context_timer.finish(true);
            on_step(Step::Context(report));

            let mut req = ChatRequest {
                model: self.config.model.clone(),
                model_digest: None,
                messages: sent,
                tools: self.inference_schemas().to_vec(),
                temperature: self.config.temperature,
                num_ctx: Some(self.config.num_ctx as u32),
                draft_model: self.config.draft_model.clone(),
                draft_count: self.config.draft_count,
                keep_alive: None,
                fabric: Some(FabricCallMeta {
                    session_id: self.config.session_id.clone(),
                    agent_id: Some(self.config.agent_id.clone()),
                    step_index,
                    turn_id: convo.turn_id().map(String::from),
                    run_id: self
                        .config
                        .run_id
                        .clone()
                        .or_else(|| compiled_run_id.clone()),
                    task_id: self.config.task_id.clone().or_else(|| {
                        compiled_task_id
                            .clone()
                            .or_else(|| convo.turn_id().map(String::from))
                    }),
                    attempt_id: self.config.attempt_id.clone(),
                    data_class: self.config.data_class,
                    context_data_class: compiled_context_data_class,
                    workspace_version: compiled_workspace_version.clone(),
                    trace_context: fabric_trace.clone(),
                    disclosure_tier: self.config.disclosure_tier,
                    model_tier: self.config.model_tier.clone(),
                    ..Default::default()
                }),
                response_format: None,
                outbound_scan: Default::default(),
            };
            lokai_inference::stamp_request_classification(&mut req);
            step_index = step_index.saturating_add(1);
            let _infer_stage = tetonic_telemetry::enter_stage_child("infer");
            let resp = {
                let mut demuxer = crate::demuxer::TokenDemuxer::new();
                let mut on_token = |t: &str| {
                    for chunk in demuxer.push(t) {
                        match chunk {
                            crate::demuxer::DemuxedChunk::Prose(text) => {
                                on_step(Step::Token(text));
                            }
                            crate::demuxer::DemuxedChunk::Thought(thought) => {
                                on_step(Step::Thought(thought));
                            }
                            crate::demuxer::DemuxedChunk::ToolJson(_) => {}
                        }
                    }
                };
                let inference_timer =
                    tetonic_telemetry::StageTimer::start(tetonic_telemetry::PerfStage::Inference);
                let result = self.provider.chat(req, &mut on_token).await;
                inference_timer.finish(result.is_ok());
                for chunk in demuxer.finish() {
                    match chunk {
                        crate::demuxer::DemuxedChunk::Prose(text) => {
                            on_step(Step::Token(text));
                        }
                        crate::demuxer::DemuxedChunk::Thought(thought) => {
                            on_step(Step::Thought(thought));
                        }
                        crate::demuxer::DemuxedChunk::ToolJson(_) => {}
                    }
                }
                match result {
                    Ok(resp) => resp,
                    Err(error) => {
                        return CandidateOutcome::Failed {
                            message: error.to_string(),
                        };
                    }
                }
            };
            tetonic_telemetry::fault::inject_fault("after_model_request");

            // Surface generation throughput (prefill/decode) for this step, since
            // prefill is re-paid every step and dominates end-to-end latency.
            if resp.usage.reported() {
                if let Some(a) = self.audit() {
                    a.note(&format_usage(&resp.usage));
                }
                on_step(Step::Generation(resp.usage.clone()));
            }

            let mut msg = resp.message;
            lokai_inference::recover_message_tool_calls(&mut msg);
            let tool_calls = msg.tool_calls.clone().unwrap_or_default();
            if let Some(a) = self.audit() {
                let tcj = msg
                    .tool_calls
                    .as_ref()
                    .and_then(|t| serde_json::to_string(t).ok());
                a.message("assistant", &msg.content, tcj.as_deref());
            }
            convo.messages.push(msg);

            if tool_calls.is_empty() {
                let answer_text = convo
                    .messages
                    .last()
                    .map(|m| m.content.trim())
                    .unwrap_or("");
                if explain_turn && !answer_text.is_empty() {
                    on_step(Step::Answer(answer_text.to_string()));
                    on_step(Step::Stopped("answered".into()));
                    return CandidateOutcome::Completed {
                        summary: answer_text.to_string(),
                        kind: CompletionKind::Answer,
                    };
                }
                if explain_turn && steps_used >= max_steps.saturating_sub(2) as u32 {
                    if let Some(nudge) = &invocation.discipline.notes.explain_cap_nudge {
                        if let Some(a) = self.audit() {
                            a.note("explain turn: synthesis nudge before cap");
                            a.message("user", nudge, None);
                        }
                        on_step(Step::Note(
                            "approaching step cap — synthesize your answer".into(),
                        ));
                        convo.messages.push(Message::user(nudge.clone()));
                        continue 'steps;
                    }
                }
                if explain_turn && steps_used >= max_steps.saturating_sub(1) as u32 {
                    if let Some(nudge) = &invocation.discipline.notes.explain_last_nudge {
                        if let Some(a) = self.audit() {
                            a.note("explain turn: final synthesis nudge (tool calls present)");
                            a.message("user", nudge, None);
                        }
                        on_step(Step::Note(
                            "approaching step cap — call the completion tool with your answer"
                                .into(),
                        ));
                        convo.messages.push(Message::user(nudge.clone()));
                        continue 'steps;
                    }
                }
                if let Some(nudge) =
                    monitor.empty_tool_nudge(invocation.empty_tool_nudge, convo.messages.len())
                {
                    if let Some(a) = self.audit() {
                        a.note(&format!(
                            "empty tool calls (retry {}/{}): nudging model",
                            monitor.empty_tool_retries(),
                            self.config.empty_tool_retry_limit
                        ));
                        a.message("user", &nudge, None);
                    }
                    on_step(Step::Note(monitor.empty_tool_retry_label()));
                    convo.messages.push(Message::user(nudge.to_string()));
                    continue 'steps;
                }
                self.abort_staged_mutations().await;
                on_step(Step::Stopped("model answered with no tool calls".into()));
                return CandidateOutcome::Limited {
                    kind: LimitKind::EmptyTools,
                    message: "model answered with no tool calls".into(),
                };
            }

            // Calls can depend on earlier mutations and on per-call discipline.
            // Execute only after those checks, preserving the requested order.
            for tc in tool_calls {
                let name = tc.function.name.clone();
                let args = tc.function.arguments.clone();
                let call_id = format!("tc_{:x}_{}", convo.nonce, convo.call_no);
                convo.call_no += 1;
                let args_json = serde_json::to_string(&args).unwrap_or_default();

                // A catalog entry describes syntax, not authority. Enforce the
                // active role before even special in-loop tools (spawn/finish).
                if self.inference_catalog.is_some()
                    && (!self.schemas.iter().any(|s| s.function.name == name)
                        || !self.tools.is_tool_allowed(&name))
                {
                    let feedback = format!("Tool `{name}` is not available in this turn. Use only the current turn's permitted tools.");
                    if let Some(a) = self.audit() {
                        a.tool_call(
                            &call_id,
                            &name,
                            &args_json,
                            false,
                            &feedback,
                            Some("policy_denied"),
                        );
                        a.tool_message(&name, &call_id, &feedback);
                    }
                    on_step(Step::ToolResult {
                        call_id: call_id.clone(),
                        name: name.clone(),
                        ok: false,
                        summary: feedback.clone(),
                    });
                    convo
                        .messages
                        .push(Message::tool(name, feedback).with_tool_call_id(&call_id));
                    monitor.record_no_progress_event();
                    continue;
                }
                on_step(Step::ToolCall {
                    call_id: call_id.clone(),
                    name: name.clone(),
                    args: args.clone(),
                });

                if name == invocation.completion_tool {
                    let mut summary = args
                        .get("summary")
                        .and_then(|v| v.as_str())
                        .unwrap_or("(no summary)")
                        .to_string();

                    let assistant_content = convo
                        .messages
                        .iter()
                        .rev()
                        .find(|m| m.role == "assistant")
                        .map(|m| m.content.trim())
                        .unwrap_or("");
                    let min_chars = invocation.discipline.finish_min_chars.unwrap_or(0);
                    if !assistant_content.is_empty() {
                        if summary == "(no summary)" || summary.len() < min_chars {
                            summary = assistant_content.to_string();
                        } else if !assistant_content.contains(&summary)
                            && !summary.contains(assistant_content)
                        {
                            summary = format!("{assistant_content}\n\n{summary}");
                        }
                    }

                    let too_short = invocation
                        .discipline
                        .finish_min_chars
                        .is_some_and(|n| summary.len() < n);
                    if explain_turn
                        && (summary.trim().is_empty() || summary == "(no summary)" || too_short)
                    {
                        let feedback = invocation
                            .discipline
                            .notes
                            .finish_too_short
                            .as_deref()
                            .unwrap_or("ERROR: completion requires a non-empty `summary`.");
                        if let Some(a) = self.audit() {
                            a.tool_call(
                                &call_id,
                                &name,
                                &args_json,
                                false,
                                "empty finish summary on explain turn",
                                Some("empty_finish"),
                            );
                            a.tool_message(&name, &call_id, feedback);
                        }
                        on_step(Step::ToolResult {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            ok: false,
                            summary: "finish requires summary".into(),
                        });
                        convo.messages.push(
                            Message::tool(name.clone(), feedback.to_string())
                                .with_tool_call_id(&call_id),
                        );
                        monitor.record_no_progress_event();
                        continue;
                    }

                    if let Some(a) = self.audit() {
                        a.tool_call(&call_id, &name, &args_json, true, &summary, None);
                    }
                    on_step(Step::Answer(summary.clone()));
                    on_step(Step::Stopped(format!("finished: {summary}")));
                    return CandidateOutcome::Completed {
                        summary,
                        kind: CompletionKind::Finish,
                    };
                }

                if convo.is_canceled() {
                    self.abort_staged_mutations().await;
                    on_step(Step::Stopped("canceled".into()));
                    return CandidateOutcome::Canceled {
                        reason: "canceled".into(),
                    };
                }

                if invocation.discipline.spawn_tool.as_deref() == Some(name.as_str()) {
                    if let Some(hook) = &self.spawn {
                        let role = args
                            .get("role")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let task = args
                            .get("task")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let outcome = hook(
                            SpawnRequest {
                                role,
                                task,
                                parent_agent_id: self.config.agent_id.clone(),
                            },
                            convo,
                        )
                        .await;
                        let model_str = outcome.to_model_string();
                        if let Some(a) = self.audit() {
                            a.tool_call(
                                &call_id,
                                &name,
                                &args_json,
                                outcome.ok,
                                &outcome.summary,
                                outcome.error_kind.as_deref(),
                            );
                            a.tool_message(&name, &call_id, &model_str);
                        }
                        on_step(Step::ToolResult {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            ok: outcome.ok,
                            summary: outcome.summary.clone(),
                        });
                        convo.messages.push(Message::tool(name.clone(), model_str));
                        continue 'steps;
                    }
                    let feedback = invocation
                        .discipline
                        .notes
                        .spawn_disabled
                        .as_deref()
                        .unwrap_or("ERROR: {name} is not enabled for this agent.")
                        .replace("{name}", &name);
                    if let Some(a) = self.audit() {
                        a.tool_call(
                            &call_id,
                            &name,
                            &args_json,
                            false,
                            "spawn disabled",
                            Some("denied"),
                        );
                        a.tool_message(&name, &call_id, &feedback);
                    }
                    on_step(Step::ToolResult {
                        call_id: call_id.clone(),
                        name: name.clone(),
                        ok: false,
                        summary: "spawn disabled".into(),
                    });
                    convo.messages.push(Message::tool(name.clone(), feedback));
                    continue 'steps;
                }

                // D7: validate tool JSON before execute; one repair pass via tool result.
                if name != invocation.completion_tool {
                    if let Err(e) = self.tools.validate_tool_args(&name, &args) {
                        let feedback = format!(
                            "ERROR: invalid arguments for `{name}`: {e}. \
Fix the JSON to match the tool schema and call the tool again."
                        );
                        if let Some(a) = self.audit() {
                            a.tool_call(
                                &call_id,
                                &name,
                                &args_json,
                                false,
                                "invalid arguments",
                                Some("bad_args"),
                            );
                            a.tool_message(&name, &call_id, &feedback);
                        }
                        on_step(Step::ToolResult {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            ok: false,
                            summary: "invalid arguments".into(),
                        });
                        convo.messages.push(Message::tool(name.clone(), feedback));
                        continue 'steps;
                    }
                }

                if invocation.discipline.expand_tool.as_deref() == Some(name.as_str()) {
                    if let Some(compiler) = &self.context_compiler {
                        let handle_id =
                            args.get("handle_id").and_then(|v| v.as_str()).unwrap_or("");
                        let current_fp =
                            match (&self.config.workspace_root, &self.capture_workspace_version) {
                                (Some(root), Some(capture)) => {
                                    capture(root, &[]).map(|version| version.state_fingerprint())
                                }
                                _ => Err("context expansion requires a current workspace snapshot"
                                    .into()),
                            };
                        let expansion = match current_fp {
                            Ok(fp) => match (
                                &self.config.session_id,
                                &self.config.run_id,
                                &self.config.task_id,
                            ) {
                                (Some(session), Some(run), Some(task)) => {
                                    compiler
                                        .expand(&tetonic_domain::ContextExpansionRequest {
                                            handle_id: handle_id.to_string(),
                                            session_id: tetonic_domain::SessionId::new(
                                                session.clone(),
                                            ),
                                            run_id: tetonic_domain::RunId::new(run.clone()),
                                            task_id: tetonic_domain::TaskId::new(task.clone()),
                                            current_workspace_fp: fp,
                                        })
                                        .await
                                }
                                _ => Err(
                                    "context expansion requires an explicit session, run and task"
                                        .into(),
                                ),
                            },
                            Err(error) => Err(error),
                        };
                        match expansion {
                            Ok(evidence) => {
                                let summary =
                                    format!("Expanded handle: retrieved {} items", evidence.len());
                                let model_str =
                                    serde_json::to_string_pretty(&evidence).unwrap_or_default();
                                if let Some(a) = self.audit() {
                                    a.tool_call(&call_id, &name, &args_json, true, &summary, None);
                                    a.tool_message(&name, &call_id, &model_str);
                                }
                                on_step(Step::ToolResult {
                                    call_id: call_id.clone(),
                                    name: name.clone(),
                                    ok: true,
                                    summary,
                                });
                                convo.messages.push(Message::tool(name.clone(), model_str));
                                continue 'steps;
                            }
                            Err(e) => {
                                let feedback = invocation
                                    .discipline
                                    .notes
                                    .expand_failed
                                    .as_deref()
                                    .unwrap_or("ERROR: {name} failed: {error}")
                                    .replace("{name}", &name)
                                    .replace("{error}", &e);
                                if let Some(a) = self.audit() {
                                    a.tool_call(
                                        &call_id,
                                        &name,
                                        &args_json,
                                        false,
                                        &e,
                                        Some("expand_failed"),
                                    );
                                    a.tool_message(&name, &call_id, &feedback);
                                }
                                on_step(Step::ToolResult {
                                    call_id: call_id.clone(),
                                    name: name.clone(),
                                    ok: false,
                                    summary: e.clone(),
                                });
                                convo.messages.push(Message::tool(name.clone(), feedback));
                                continue 'steps;
                            }
                        }
                    } else {
                        let feedback = invocation
                            .discipline
                            .notes
                            .expand_disabled
                            .as_deref()
                            .unwrap_or("ERROR: {name} is not enabled for this agent.")
                            .replace("{name}", &name);
                        if let Some(a) = self.audit() {
                            a.tool_call(
                                &call_id,
                                &name,
                                &args_json,
                                false,
                                "context compiler not configured",
                                Some("denied"),
                            );
                            a.tool_message(&name, &call_id, &feedback);
                        }
                        on_step(Step::ToolResult {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            ok: false,
                            summary: "context compiler not configured".into(),
                        });
                        convo.messages.push(Message::tool(name.clone(), feedback));
                        continue 'steps;
                    }
                }

                let path_arg = args.get("path").and_then(|v| v.as_str());
                let has_slice_range = path_arg.map(|p| p.contains(':')).unwrap_or(false)
                    || args
                        .get("start_line")
                        .map(|v| !v.is_null())
                        .unwrap_or(false)
                    || args.get("end_line").map(|v| !v.is_null()).unwrap_or(false);
                let whole_file_read =
                    invocation.discipline.is_whole_file_tool(&name) && !has_slice_range;

                if explain_turn
                    && name != invocation.completion_tool
                    && !self.tools.is_read_only(&name)
                {
                    let feedback = invocation
                        .discipline
                        .notes
                        .mutate_feedback
                        .as_deref()
                        .unwrap_or("ERROR: `{name}` is not allowed on read-only / explain turns.")
                        .replace("{name}", &name)
                        .replace("{completion}", &invocation.completion_tool);
                    if let Some(a) = self.audit() {
                        a.tool_call(
                            &call_id,
                            &name,
                            &args_json,
                            false,
                            "mutating tool blocked (explain turn)",
                            Some("denied"),
                        );
                        a.tool_message(&name, &call_id, &feedback);
                    }
                    on_step(Step::ToolResult {
                        call_id: call_id.clone(),
                        name: name.clone(),
                        ok: false,
                        summary: format!("{name} blocked on explain turn"),
                    });
                    convo.messages.push(Message::tool(name.clone(), feedback));
                    continue 'steps;
                }

                if explain_turn && whole_file_read {
                    if let Some(p) = path_arg {
                        if convo.retrieved_paths.contains(p) {
                            let feedback = invocation
                                .discipline
                                .notes
                                .reread_feedback
                                .as_deref()
                                .unwrap_or("ERROR: you already read `{path}` in full this turn.")
                                .replace("{path}", p);
                            if let Some(a) = self.audit() {
                                a.tool_call(
                                    &call_id,
                                    &name,
                                    &args_json,
                                    false,
                                    "duplicate read blocked (explain turn)",
                                    Some("duplicate_read"),
                                );
                                a.tool_message(&name, &call_id, &feedback);
                            }
                            on_step(Step::ToolResult {
                                call_id: call_id.clone(),
                                name: name.clone(),
                                ok: false,
                                summary: format!("duplicate read: {p}"),
                            });
                            convo.messages.push(Message::tool(name.clone(), feedback));
                            monitor.record_no_progress_event();
                            continue;
                        }
                        let clean_p = p.split(':').next().unwrap_or(p);
                        if let Some(root) = &self.config.workspace_root {
                            if let Some(hook) = &self.resolve_under_root {
                                if let Ok(file_len) = hook(root, clean_p) {
                                    let max_explain_bytes = invocation
                                        .discipline
                                        .limits
                                        .max_whole_file_bytes
                                        .unwrap_or(self.config.limits.max_explain_whole_file_bytes)
                                        as u64;
                                    if file_len > max_explain_bytes {
                                        let feedback = invocation
                                            .discipline
                                            .notes
                                            .size_feedback
                                            .as_deref()
                                            .unwrap_or(
                                                "ERROR: `{path}` is {size_kb} KB — too large for a whole-file read on explain turns.",
                                            )
                                            .replace("{path}", clean_p)
                                            .replace("{size_kb}", &(file_len / 1024).to_string());
                                        if let Some(a) = self.audit() {
                                            a.tool_call(
                                                &call_id,
                                                &name,
                                                &args_json,
                                                false,
                                                "file too large for explain turn",
                                                Some("file_too_large"),
                                            );
                                            a.tool_message(&name, &call_id, &feedback);
                                        }
                                        on_step(Step::ToolResult {
                                            call_id: call_id.clone(),
                                            name: name.clone(),
                                            ok: false,
                                            summary: format!("{clean_p} too large ({file_len})"),
                                        });
                                        convo.messages.push(Message::tool(name.clone(), feedback));
                                        monitor.record_no_progress_event();
                                        continue 'steps;
                                    }
                                }
                            }
                        }
                    }
                }

                if !explain_turn && !self.tools.is_read_only(&name) {
                    if let Some(p) = path_arg {
                        if let Some(feedback) = monitor.check_write_repeat(p) {
                            if let Some(a) = self.audit() {
                                a.tool_call(
                                    &call_id,
                                    &name,
                                    &args_json,
                                    false,
                                    "write_file fragmentation blocked",
                                    Some("write_fragmentation"),
                                );
                                a.tool_message(&name, &call_id, &feedback);
                            }
                            on_step(Step::ToolResult {
                                call_id: call_id.clone(),
                                name: name.clone(),
                                ok: false,
                                summary: format!("too many writes to {p}"),
                            });
                            convo.messages.push(Message::tool(name.clone(), feedback));
                            monitor.record_no_progress_event();
                            continue;
                        }
                    }
                }

                if self.approval.is_some() && self.tools.requires_user_approval(&name) {
                    self.persist_turn_state(
                        convo,
                        TurnState::AwaitingApproval,
                        Some(serde_json::json!({ "tool": name, "call_id": call_id }).to_string()),
                    );
                }
                self.persist_turn_state(
                    convo,
                    TurnState::Executing,
                    Some(serde_json::json!({ "tool": name, "call_id": call_id }).to_string()),
                );
                tetonic_telemetry::fault::inject_fault("before_tool_execution");
                let tool_timer =
                    tetonic_telemetry::StageTimer::start(tetonic_telemetry::PerfStage::Tool);
                let outcome = self.execute_gated(&name, &args, &call_id).await;
                tool_timer.finish(outcome.ok);
                tetonic_telemetry::fault::inject_fault("after_tool_execution");

                // Retrieval discipline: track which files have been read in full so
                // we can nudge the model away from re-scanning files it already has,
                // and keep the tracking honest as the tree changes.
                let mut model_str = outcome.to_model_string();
                if outcome.ok {
                    if whole_file_read {
                        if let Some(p) = path_arg {
                            if !convo.retrieved_paths.insert(p.to_string()) && !explain_turn {
                                if let Some(nudge) = &invocation.discipline.notes.retrieval_nudge {
                                    model_str.push_str(nudge);
                                }
                            }
                        }
                    }
                    if !self.tools.is_read_only(&name) {
                        if let Some(ch) = &outcome.change {
                            if let Some(hook) = &self.post_edit_snapshot {
                                model_str.push_str(&hook(ch));
                            }
                            convo.retrieved_paths.insert(ch.path.clone());
                        } else if let Some(p) = path_arg {
                            convo.retrieved_paths.remove(p);
                        } else {
                            convo.retrieved_paths.clear();
                        }
                    }
                }

                let progress = monitor.record_tool_execution(
                    &name,
                    &args,
                    &args_json,
                    path_arg,
                    whole_file_read,
                    &outcome,
                );
                model_str.push_str(&progress.append_to_model);

                if let Some(a) = self.audit() {
                    a.tool_call(
                        &call_id,
                        &name,
                        &args_json,
                        outcome.ok,
                        &outcome.summary,
                        outcome.error_kind.as_deref(),
                    );
                    a.tool_message(&name, &call_id, &model_str);
                    if let Some(ch) = &outcome.change {
                        a.file_change(
                            &call_id,
                            &ch.path,
                            ch.kind.as_str(),
                            ch.before.as_deref(),
                            ch.after.as_deref(),
                        );
                    }
                }
                on_step(Step::ToolResult {
                    call_id: call_id.clone(),
                    name: name.clone(),
                    ok: outcome.ok,
                    summary: outcome.summary.clone(),
                });
                convo.messages.push(Message::tool(name, model_str));

                if progress.stuck {
                    let note = monitor.stuck_reason();
                    if let Some(a) = self.audit() {
                        a.note(note);
                    }
                    self.abort_staged_mutations().await;
                    on_step(Step::Stopped(note.into()));
                    return CandidateOutcome::Limited {
                        kind: LimitKind::NoProgress,
                        message: note.into(),
                    };
                }
            }
        }

        if explain_turn {
            if let Some(ans) = explain_cap_fallback(&convo.messages, convo.prefix_len) {
                on_step(Step::Answer(ans.clone()));
                on_step(Step::Stopped(
                    "answered at effort cap (synthesized from context)".into(),
                ));
                return CandidateOutcome::Completed {
                    summary: ans,
                    kind: CompletionKind::Answer,
                };
            }
        }

        self.abort_staged_mutations().await;
        on_step(Step::Stopped(format!(
            "effort cap reached ({max_steps} steps)"
        )));
        CandidateOutcome::Limited {
            kind: LimitKind::EffortCap,
            message: format!("effort cap reached ({max_steps} steps)"),
        }
    }
}

#[cfg(windows)]
fn default_shell_identity() -> String {
    "cmd".into()
}

#[cfg(not(windows))]
fn default_shell_identity() -> String {
    "sh".into()
}

fn explain_cap_fallback(messages: &[Message], prefix_len: usize) -> Option<String> {
    for m in messages.iter().rev() {
        if m.role == "assistant" {
            let t = m.content.trim();
            if t.len() >= 40 && !content_looks_like_tool_json(t) {
                return Some(t.to_string());
            }
        }
    }
    for m in messages.iter() {
        if m.role == "assistant" {
            let t = m.content.trim();
            if t.contains("[Summary of earlier work") && t.len() >= 40 {
                return Some(t.to_string());
            }
        }
    }
    if prefix_len >= 2 {
        if let Some(m) = messages.get(1) {
            let summary = m.content.trim();
            if summary.contains("[Summary of earlier work") && summary.len() >= 40 {
                return Some(summary.to_string());
            }
        }
    }
    None
}

fn content_looks_like_tool_json(content: &str) -> bool {
    let t = content.trim();
    t.starts_with('{') && t.contains("tool_calls")
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use lokai_inference::{ChatRequest, ChatResponse, FabricSnapshot, InferenceError, TokenSink};
    use lokai_tools::Tools;
    use tetonic_domain::{ActionKind, CapabilityError, ToolAdvertisement, ToolProposal};

    fn test_inv(user: &str) -> AgentInvocation {
        test_inv_explain(user, false)
    }

    fn wire_test_capability_helpers(agent: Agent) -> Agent {
        agent
            .with_post_edit_snapshot(Arc::new(lokai_tools::format_post_edit_snapshot))
            .with_resolve_under_root(Arc::new(|root, rel| {
                let abs =
                    tetonic_transaction::fs_ops::resolve_under_root(root, rel).map_err(|_| ())?;
                std::fs::metadata(abs).map(|m| m.len()).map_err(|_| ())
            }))
            .with_capture_workspace_version(Arc::new(|root, paths| {
                tetonic_transaction::version::capture_workspace_version(root, paths)
                    .map_err(|e| e.to_string())
            }))
    }

    fn test_inv_explain(user: &str, explain_turn: bool) -> AgentInvocation {
        AgentInvocation {
            instructions: "You are a test agent operating in the user's workspace.".into(),
            user_input: user.to_string(),
            explain_turn,
            empty_tool_nudge: false,
            max_steps: 16,
            completion_tool: "finish".into(),
            discipline: tetonic_domain::LoopDiscipline::default(),
        }
    }

    struct MockTestProvider {
        requests: std::sync::Mutex<Vec<ChatRequest>>,
    }

    #[tokio::test]
    async fn stable_catalog_does_not_authorize_inactive_tools_or_spawn() {
        struct AttemptsForbiddenTools(std::sync::atomic::AtomicUsize);
        #[async_trait]
        impl InferenceProvider for AttemptsForbiddenTools {
            async fn fabric_snapshot(&self) -> FabricSnapshot {
                FabricSnapshot {
                    nodes: vec![],
                    effective_concurrency: 1,
                    generated_at: chrono::Utc::now(),
                }
            }
            async fn chat(
                &self,
                _: ChatRequest,
                _: &mut TokenSink<'_>,
            ) -> Result<ChatResponse, InferenceError> {
                let calls = if self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
                    vec![
                        (
                            "write_file",
                            serde_json::json!({"path":"forbidden.txt","content":"no"}),
                        ),
                        (
                            "spawn_agent",
                            serde_json::json!({"role":"coder","task":"write a file"}),
                        ),
                    ]
                } else {
                    vec![(
                        "finish",
                        serde_json::json!({"summary":"Completed with permitted tools only."}),
                    )]
                };
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(
                        calls
                            .into_iter()
                            .map(|(name, arguments)| lokai_inference::ToolCall {
                                function: lokai_inference::FunctionCall {
                                    name: name.into(),
                                    arguments,
                                },
                            })
                            .collect(),
                    ),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let full = Tools::new(lokai_tools::Workspace::new(dir.path()).unwrap(), false)
            .with_orchestration(true);
        let catalog = full
            .advertisements()
            .into_iter()
            .map(|t| ToolSchema::function(&t.name, &t.description, t.parameters))
            .collect();
        let restricted = full.with_allowed_tools(["finish".to_string()].into_iter().collect());
        let agent = Agent::new(
            Arc::new(AttemptsForbiddenTools(Default::default())),
            restricted,
            AgentConfig::default(),
        )
        .with_stable_tool_catalog(catalog, "Only finish is permitted this turn.".into())
        .unwrap();
        assert_eq!(agent.advertised_tool_names(), vec!["finish"]);
        let mut invocation = test_inv("Complete without writing or spawning");
        invocation.discipline.spawn_tool = Some("spawn_agent".into());
        let mut denied = Vec::new();
        let result = agent
            .turn(&mut Conversation::new(), invocation, |step| {
                if let Step::ToolResult {
                    name, ok: false, ..
                } = step
                {
                    denied.push(name);
                }
            })
            .await;
        assert!(result.is_completed());
        assert_eq!(denied, vec!["write_file", "spawn_agent"]);
        assert!(!dir.path().join("forbidden.txt").exists());
    }

    #[async_trait]
    impl InferenceProvider for MockTestProvider {
        async fn fabric_snapshot(&self) -> FabricSnapshot {
            FabricSnapshot {
                nodes: vec![],
                effective_concurrency: 1,
                generated_at: chrono::Utc::now(),
            }
        }

        async fn chat(
            &self,
            req: ChatRequest,
            _on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            self.requests.lock().unwrap().push(req);
            Ok(ChatResponse {
                message: Message::assistant("").with_tool_calls(vec![lokai_inference::ToolCall {
                    function: lokai_inference::FunctionCall {
                        name: "finish".into(),
                        arguments: serde_json::json!({"summary": "done"}),
                    },
                }]),
                usage: Default::default(),
                provenance: Default::default(),
            })
        }
    }

    #[tokio::test]
    async fn test_tool_schemas_sorted_deterministically() {
        let temp = tempfile::tempdir().unwrap();
        let ws = lokai_tools::Workspace::new(temp.path()).unwrap();
        let tools = Tools::new(ws, false);
        let provider = Arc::new(MockTestProvider {
            requests: std::sync::Mutex::new(vec![]),
        });
        let agent = Agent::new(provider, tools, AgentConfig::default());

        for i in 1..agent.schemas.len() {
            assert!(
                agent.schemas[i - 1].function.name <= agent.schemas[i].function.name,
                "schemas must be sorted alphabetically: {} > {}",
                agent.schemas[i - 1].function.name,
                agent.schemas[i].function.name
            );
        }
    }

    #[tokio::test]
    async fn rebind_preserves_agent_and_history_but_changes_next_request() {
        struct Count;
        impl Tokenizer for Count {
            fn count(&self, _: &str) -> usize {
                7
            }
        }
        let old = Arc::new(MockTestProvider {
            requests: std::sync::Mutex::new(vec![]),
        });
        let new = Arc::new(MockTestProvider {
            requests: std::sync::Mutex::new(vec![]),
        });
        let mut agent = Agent::new(old.clone(), EmptyToolHost, AgentConfig::default());
        agent.config.agent_id = "existing-agent".into();
        agent.config.draft_model = Some("old-draft".into());
        let mut conversation = Conversation::from_audit_messages(vec![
            Message::system("preserve instructions"),
            Message::user("preserve history"),
        ]);
        assert_eq!(conversation.messages[0].cached_tokens(|| 999), 999);
        let initial_model = agent.config.model.clone();
        assert!(agent
            .replace_inference(
                &mut conversation,
                crate::AgentInferenceBinding {
                    provider: new.clone(),
                    model: String::new(),
                    num_ctx: 8192,
                    tokenizer: Box::new(Count),
                }
            )
            .is_err());
        assert_eq!(agent.config.model, initial_model);
        assert_eq!(conversation.messages[0].cached_tokens(|| 7), 999);
        agent
            .replace_inference(
                &mut conversation,
                crate::AgentInferenceBinding {
                    provider: new.clone(),
                    model: "replacement".into(),
                    num_ctx: 8192,
                    tokenizer: Box::new(Count),
                },
            )
            .unwrap();
        assert_eq!(agent.execution_agent_id(), "existing-agent");
        assert_eq!(conversation.messages[0].cached_tokens(|| 7), 7);
        assert_eq!(agent.tools_token_count, 7);
        assert!(agent.config.draft_model.is_none());
        agent
            .turn(
                &mut conversation,
                test_inv_explain("continue", true),
                |_| {},
            )
            .await;
        assert!(old.requests.lock().unwrap().is_empty());
        let requests = new.requests.lock().unwrap();
        assert_eq!(requests[0].model, "replacement");
        assert!(requests[0]
            .messages
            .iter()
            .any(|m| m.content == "preserve history"));
    }

    #[tokio::test]
    async fn test_prefix_kv_cache_invariance() {
        let temp = tempfile::tempdir().unwrap();
        let ws = lokai_tools::Workspace::new(temp.path()).unwrap();
        let tools = Tools::new(ws, false);
        let provider = Arc::new(MockTestProvider {
            requests: std::sync::Mutex::new(vec![]),
        });
        let agent = Agent::new(provider.clone(), tools, AgentConfig::default());

        let mut convo = Conversation::new();
        agent
            .turn(&mut convo, test_inv("Inspect repository"), |_| {})
            .await;

        let reqs = provider.requests.lock().unwrap();
        assert!(!reqs.is_empty());
        let first_sys = &reqs[0].messages[0];
        let first_user = &reqs[0].messages[1];

        // Ensure system prompt is non-empty and starts with operator card
        assert_eq!(first_sys.role, "system");
        assert_eq!(first_user.role, "user");

        let sys_content = &first_sys.content;
        let user_content = &first_user.content;
        assert!(!sys_content.is_empty());
        assert!(!user_content.is_empty());
    }

    struct MultiToolMockProvider {
        turn: std::sync::atomic::AtomicUsize,
        write_first: bool,
    }

    #[async_trait]
    impl InferenceProvider for MultiToolMockProvider {
        async fn fabric_snapshot(&self) -> FabricSnapshot {
            FabricSnapshot {
                nodes: vec![],
                effective_concurrency: 1,
                generated_at: chrono::Utc::now(),
            }
        }

        async fn chat(
            &self,
            _req: ChatRequest,
            _on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            let t = self.turn.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if t == 0 {
                let mut calls = Vec::new();
                if self.write_first {
                    calls.push(lokai_inference::ToolCall {
                        function: lokai_inference::FunctionCall {
                            name: "update".into(),
                            arguments: serde_json::json!({}),
                        },
                    });
                }
                calls.extend(
                    ["a.txt", "b.txt", "c.txt"].map(|path| lokai_inference::ToolCall {
                        function: lokai_inference::FunctionCall {
                            name: "read_file".into(),
                            arguments: serde_json::json!({"path": path}),
                        },
                    }),
                );
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(calls),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            } else {
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        lokai_inference::ToolCall {
                            function: lokai_inference::FunctionCall {
                                name: "finish".into(),
                                arguments: serde_json::json!({"summary": "inspected 3 files"}),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            }
        }
    }

    struct StreamingPrefetchMockProvider {
        turn: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl InferenceProvider for StreamingPrefetchMockProvider {
        async fn chat(
            &self,
            _req: ChatRequest,
            on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            let t = self.turn.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if t == 0 {
                on_token(
                    "{\"name\": \"read_file\", \"arguments\": {\"path\": \"speculative.txt\"}}",
                );
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        lokai_inference::ToolCall {
                            function: lokai_inference::FunctionCall {
                                name: "read_file".into(),
                                arguments: serde_json::json!({"path": "speculative.txt"}),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            } else {
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        lokai_inference::ToolCall {
                            function: lokai_inference::FunctionCall {
                                name: "finish".into(),
                                arguments: serde_json::json!({"summary": "read speculative"}),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            }
        }
    }

    #[tokio::test]
    async fn test_multiple_read_only_tool_execution() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("a.txt"), "hello from a").unwrap();
        std::fs::write(temp.path().join("b.txt"), "hello from b").unwrap();
        std::fs::write(temp.path().join("c.txt"), "hello from c").unwrap();

        let ws = lokai_tools::Workspace::new(temp.path()).unwrap();
        let root = ws.root().to_path_buf();
        let tools = Tools::new(ws, false);
        let provider = Arc::new(MultiToolMockProvider {
            turn: std::sync::atomic::AtomicUsize::new(0),
            write_first: false,
        });
        let agent = Agent::new(
            provider,
            tools,
            AgentConfig {
                workspace_root: Some(root),
                ..AgentConfig::default()
            },
        )
        .with_action_broker(Arc::new(IssueAllBroker));
        let agent = wire_test_capability_helpers(agent);

        let mut convo = Conversation::new();
        agent.turn(&mut convo, test_inv("Read files"), |_| {}).await;

        // Check that messages contains tool outcomes for a, b, c in exact sequence
        let tool_messages: Vec<&Message> =
            convo.messages.iter().filter(|m| m.role == "tool").collect();
        assert_eq!(tool_messages.len(), 3);
        assert!(tool_messages[0].content.contains("hello from a"));
        assert!(tool_messages[1].content.contains("hello from b"));
        assert!(tool_messages[2].content.contains("hello from c"));
    }

    #[tokio::test]
    async fn test_streaming_lookahead_prefetch_execution() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("speculative.txt"),
            "speculative lookahead content",
        )
        .unwrap();

        let ws = lokai_tools::Workspace::new(temp.path()).unwrap();
        let tools = Tools::new(ws, false);
        let provider = Arc::new(StreamingPrefetchMockProvider {
            turn: std::sync::atomic::AtomicUsize::new(0),
        });
        let config = AgentConfig {
            workspace_root: Some(temp.path().to_path_buf()),
            ..AgentConfig::default()
        };
        let agent =
            Agent::new(provider, tools, config).with_action_broker(Arc::new(IssueAllBroker));
        let agent = wire_test_capability_helpers(agent);

        let mut convo = Conversation::new();
        agent
            .turn(&mut convo, test_inv("Read speculative file"), |_| {})
            .await;

        let tool_messages: Vec<&Message> =
            convo.messages.iter().filter(|m| m.role == "tool").collect();
        assert_eq!(tool_messages.len(), 1);
        assert!(tool_messages[0]
            .content
            .contains("speculative lookahead content"));
    }

    struct EscapePrefetchMockProvider {
        turn: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl InferenceProvider for EscapePrefetchMockProvider {
        async fn chat(
            &self,
            _req: ChatRequest,
            on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            let t = self.turn.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if t == 0 {
                on_token("{\"name\": \"read_file\", \"arguments\": {\"path\": \"../secret.txt\"}}");
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        lokai_inference::ToolCall {
                            function: lokai_inference::FunctionCall {
                                name: "read_file".into(),
                                arguments: serde_json::json!({"path": "../secret.txt"}),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            } else {
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        lokai_inference::ToolCall {
                            function: lokai_inference::FunctionCall {
                                name: "finish".into(),
                                arguments: serde_json::json!({"summary": "done"}),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            }
        }
    }

    #[tokio::test]
    async fn test_read_file_host_byte_leak_is_refused() {
        let parent = tempfile::tempdir().unwrap();
        let ws_dir = parent.path().join("ws");
        std::fs::create_dir_all(&ws_dir).unwrap();
        std::fs::write(parent.path().join("secret.txt"), "super secret host bytes").unwrap();
        std::fs::write(ws_dir.join("ok.txt"), "inside").unwrap();

        let ws = lokai_tools::Workspace::new(&ws_dir).unwrap();
        let tools = Tools::new(ws, false);
        let provider = Arc::new(EscapePrefetchMockProvider {
            turn: std::sync::atomic::AtomicUsize::new(0),
        });
        let config = AgentConfig {
            workspace_root: Some(ws_dir.clone()),
            ..AgentConfig::default()
        };
        let agent =
            Agent::new(provider, tools, config).with_action_broker(Arc::new(IssueAllBroker));
        let agent = wire_test_capability_helpers(agent);
        let mut convo = Conversation::new();
        agent
            .turn(&mut convo, test_inv("Read outside file"), |_| {})
            .await;
        let tool_messages: Vec<&Message> =
            convo.messages.iter().filter(|m| m.role == "tool").collect();
        assert_eq!(tool_messages.len(), 1);
        assert!(
            !tool_messages[0].content.contains("super secret host bytes"),
            "host bytes leaked: {}",
            tool_messages[0].content
        );
    }

    struct SlicedReadMockProvider {
        turn: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl InferenceProvider for SlicedReadMockProvider {
        async fn fabric_snapshot(&self) -> FabricSnapshot {
            FabricSnapshot {
                nodes: vec![],
                effective_concurrency: 1,
                generated_at: chrono::Utc::now(),
            }
        }

        async fn chat(
            &self,
            _req: ChatRequest,
            _on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            let t = self.turn.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if t == 0 {
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        lokai_inference::ToolCall {
                            function: lokai_inference::FunctionCall {
                                name: "read_file".into(),
                                arguments: serde_json::json!({
                                    "path": "large_file.rs",
                                    "start_line": 100,
                                    "end_line": 110
                                }),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            } else {
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        lokai_inference::ToolCall {
                            function: lokai_inference::FunctionCall {
                                name: "finish".into(),
                                arguments: serde_json::json!({
                                    "summary": "The large file slice has line 100 to 110 explained in detail."
                                }),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            }
        }
    }

    #[tokio::test]
    async fn test_explain_turn_allows_sliced_read_on_large_file() {
        let temp = tempfile::tempdir().unwrap();
        // Create a 50KB file (>16KB MAX_EXPLAIN_BYTES)
        let large_content = (1..=2000)
            .map(|i| format!("line {i}: some code content here"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(large_content.len() > 30 * 1024);
        std::fs::write(temp.path().join("large_file.rs"), &large_content).unwrap();

        let ws = lokai_tools::Workspace::new(temp.path()).unwrap();
        let root = ws.root().to_path_buf();
        let tools = Tools::new(ws, false);
        let provider = Arc::new(SlicedReadMockProvider {
            turn: std::sync::atomic::AtomicUsize::new(0),
        });
        let config = AgentConfig {
            explain_turn: true,
            workspace_root: Some(root),
            ..AgentConfig::default()
        };
        let agent =
            Agent::new(provider, tools, config).with_action_broker(Arc::new(IssueAllBroker));
        let agent = wire_test_capability_helpers(agent);

        let mut convo = Conversation::new();
        agent
            .turn(
                &mut convo,
                test_inv_explain("Explain lines 100-110", true),
                |_| {},
            )
            .await;

        let tool_messages: Vec<&Message> =
            convo.messages.iter().filter(|m| m.role == "tool").collect();
        assert_eq!(tool_messages.len(), 1);
        assert!(!tool_messages[0].content.contains("too large"));
        assert!(tool_messages[0].content.contains("line 100"));
    }

    struct IssueAllBroker;

    #[async_trait]
    impl tetonic_domain::sinks::ActionBroker for IssueAllBroker {
        async fn evaluate_and_issue(
            &self,
            action: &ProposedAction,
        ) -> Result<tetonic_domain::IssuedCapability, CapabilityError> {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            Ok(tetonic_domain::IssuedCapability {
                capability_id: tetonic_domain::CapabilityId::new("cap_m2_verify"),
                session_id: action.session_id.clone(),
                run_id: action.run_id.clone(),
                task_id: action.task_id.clone(),
                attempt_id: action.attempt_id.clone(),
                agent_id: action.agent_id.clone(),
                action_kind: action.kind.clone(),
                canonical_parameter_digest: action.parameters.digest.clone(),
                workspace_version: action.workspace_version.clone(),
                data_classification: action.data_class,
                issuance_timestamp: now,
                expiration: now + 300,
                max_use_count: 8,
                current_use_count: 0,
                issuing_policy_version: "v2".into(),
                approval_record_id: None,
                revoked: false,
            })
        }
    }

    #[test]
    fn staged_overlay_is_visible_on_tool_host() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("f.txt"), "live\n").unwrap();
        let ws = lokai_tools::Workspace::new(temp.path()).unwrap();
        let tools = Tools::new(ws, false);
        let staged = tools.execute(
            "write_file",
            &serde_json::json!({ "path": "f.txt", "content": "staged\n" }),
        );
        assert!(staged.ok, "stage write: {}", staged.content);
        let overlay = tools
            .verification_overlay_if_staged()
            .expect("overlay query")
            .expect("staged overlay path");
        assert!(
            overlay.display().to_string().contains("verify_overlay"),
            "staged verify must expose overlay path; {overlay:?}"
        );
    }

    #[test]
    fn execute_gated_does_not_call_check_tool() {
        let src = include_str!("agent.rs");
        let mut in_block = false;
        let mut production = String::new();
        for line in src.lines() {
            let trim = line.trim();
            if trim.starts_with("#[cfg(test)]") {
                in_block = true;
            }
            if in_block {
                continue;
            }
            if trim.starts_with("//") {
                continue;
            }
            production.push_str(line);
            production.push('\n');
        }
        let n = production.matches(".check_tool(").count();
        assert_eq!(n, 0, "execute_gated must not call check_tool (found {n})");
    }

    #[test]
    fn agent_tools_field_is_dyn_tool_host_not_concrete_tools() {
        let src = include_str!("agent.rs");
        let mut in_block = false;
        let mut production = String::new();
        for line in src.lines() {
            let trim = line.trim();
            if trim.starts_with("#[cfg(test)]") {
                in_block = true;
            }
            if in_block {
                continue;
            }
            production.push_str(line);
            production.push('\n');
        }
        assert!(
            production.contains("tools: Box<dyn ToolHost>"),
            "Agent must store dyn ToolHost"
        );
        assert!(
            !production.contains("tools: Tools")
                && !production.contains("tools: lokai_tools::Tools"),
            "Agent must not store concrete lokai_tools::Tools"
        );
        assert!(
            production.contains("context_compiler: Option<Arc<dyn ContextCompiler>>"),
            "Agent must store dyn ContextCompiler"
        );
    }

    struct DummyHost;

    #[derive(Clone)]
    struct OrderedHost(Arc<std::sync::Mutex<Vec<String>>>);

    impl ToolHost for OrderedHost {
        fn clone_box(&self) -> Box<dyn ToolHost> {
            Box::new(self.clone())
        }
        fn propose(&self, _name: &str, _args: &Value) -> Option<ToolProposal> {
            None
        }
        fn is_tool_allowed(&self, _name: &str) -> bool {
            true
        }
        fn is_read_only(&self, name: &str) -> bool {
            name != "update"
        }
        fn advertisements(&self) -> Vec<ToolAdvertisement> {
            vec![]
        }
        fn validate_tool_args(&self, _name: &str, _args: &Value) -> Result<(), String> {
            Ok(())
        }
        fn execute_authorized(
            &self,
            name: &str,
            _args: &Value,
            _auth: Option<&AuthorizedAction>,
            _cancel: &tetonic_domain::work_scope::CancellationSignal,
        ) -> ToolOutcome {
            let mut calls = self.0.lock().unwrap();
            if name == "read_file" {
                assert_eq!(
                    calls.first().map(String::as_str),
                    Some("update"),
                    "read ran before mutation"
                );
            }
            calls.push(name.into());
            ToolOutcome::ok("ok", "ok")
        }
    }

    #[tokio::test]
    async fn mixed_tool_batch_preserves_mutation_before_reads() {
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let agent = Agent::new(
            Arc::new(MultiToolMockProvider {
                turn: std::sync::atomic::AtomicUsize::new(0),
                write_first: true,
            }),
            OrderedHost(calls.clone()),
            AgentConfig::default(),
        );
        let outcome = agent
            .turn(
                &mut Conversation::new(),
                test_inv("update and inspect"),
                |_| {},
            )
            .await;
        assert!(matches!(outcome, CandidateOutcome::Completed { .. }));
        assert_eq!(
            *calls.lock().unwrap(),
            ["update", "read_file", "read_file", "read_file"]
        );
    }

    #[tokio::test]
    async fn batched_reads_obey_discipline_before_execution() {
        let calls = Arc::new(std::sync::Mutex::new(vec!["update".to_owned()]));
        let agent = Agent::new(
            Arc::new(MultiToolMockProvider {
                turn: std::sync::atomic::AtomicUsize::new(0),
                write_first: false,
            }),
            OrderedHost(calls.clone()),
            AgentConfig {
                workspace_root: Some(std::env::temp_dir()),
                ..Default::default()
            },
        )
        .with_resolve_under_root(Arc::new(|_, _| Ok(100)));
        let mut invocation = test_inv_explain("inspect", true);
        invocation.discipline.whole_file_tools = vec!["read_file".into()];
        invocation.discipline.limits.max_whole_file_bytes = Some(1);
        agent
            .turn(&mut Conversation::new(), invocation, |_| {})
            .await;
        assert_eq!(
            *calls.lock().unwrap(),
            ["update"],
            "rejected reads must never execute"
        );
    }

    impl Clone for DummyHost {
        fn clone(&self) -> Self {
            DummyHost
        }
    }

    impl ToolHost for DummyHost {
        fn clone_box(&self) -> Box<dyn ToolHost> {
            Box::new(self.clone())
        }
        fn propose(&self, name: &str, _args: &Value) -> Option<ToolProposal> {
            match name {
                "read_file" => Some(ToolProposal {
                    kind: ActionKind::ReadFile,
                    resolved_path: None,
                }),
                _ => None,
            }
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
            _cancel: &tetonic_domain::work_scope::CancellationSignal,
        ) -> ToolOutcome {
            ToolOutcome::ok("ok", "ok")
        }
    }

    #[test]
    fn agent_constructs_with_dummy_tool_host_not_tools() {
        let provider = Arc::new(MockTestProvider {
            requests: std::sync::Mutex::new(vec![]),
        });
        let _agent = Agent::new(provider, DummyHost, AgentConfig::default());
    }

    fn boom_capture() -> CaptureWorkspaceVersion {
        Arc::new(|_, _| Err("boom".into()))
    }

    struct CaptureErrToolProvider {
        turn: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl InferenceProvider for CaptureErrToolProvider {
        async fn fabric_snapshot(&self) -> FabricSnapshot {
            FabricSnapshot {
                nodes: vec![],
                effective_concurrency: 1,
                generated_at: chrono::Utc::now(),
            }
        }

        async fn chat(
            &self,
            _req: ChatRequest,
            _on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            let t = self.turn.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if t == 0 {
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        lokai_inference::ToolCall {
                            function: lokai_inference::FunctionCall {
                                name: "read_file".into(),
                                arguments: serde_json::json!({"path": "a.txt"}),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            } else {
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![
                        lokai_inference::ToolCall {
                            function: lokai_inference::FunctionCall {
                                name: "finish".into(),
                                arguments: serde_json::json!({"summary": "done"}),
                            },
                        },
                    ]),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            }
        }
    }

    #[tokio::test]
    async fn sub03_capture_hook_err_is_per_site() {
        let config = AgentConfig {
            workspace_root: Some(std::path::PathBuf::from("/tmp/sub03-capture-err")),
            ..AgentConfig::default()
        };

        let finish_provider = Arc::new(MockTestProvider {
            requests: std::sync::Mutex::new(vec![]),
        });
        let compiled_agent = Agent::new(finish_provider, DummyHost, config.clone())
            .with_capture_workspace_version(boom_capture());
        let mut convo = Conversation::new();
        let compiled = compiled_agent
            .turn(&mut convo, test_inv("no tools"), |_| {})
            .await;
        assert!(
            !matches!(compiled, CandidateOutcome::Failed { .. }),
            "compiled capture Err without compiler must not Failed: {compiled:?}"
        );

        let tool_provider = Arc::new(CaptureErrToolProvider {
            turn: std::sync::atomic::AtomicUsize::new(0),
        });
        let brokered = Agent::new(tool_provider, DummyHost, config)
            .with_action_broker(Arc::new(IssueAllBroker))
            .with_capture_workspace_version(boom_capture());
        let mut convo = Conversation::new();
        brokered
            .turn(&mut convo, test_inv("Read a file"), |_| {})
            .await;
        let tool_messages: Vec<&Message> =
            convo.messages.iter().filter(|m| m.role == "tool").collect();
        assert_eq!(
            tool_messages.len(),
            1,
            "brokered tool must produce one outcome"
        );
        assert!(
            tool_messages[0].content.contains("workspace version"),
            "broker present+Err must deny with workspace version: {}",
            tool_messages[0].content
        );
    }

    struct MockEmptyToolProvider;

    #[async_trait]
    impl InferenceProvider for MockEmptyToolProvider {
        async fn fabric_snapshot(&self) -> FabricSnapshot {
            FabricSnapshot {
                nodes: vec![],
                effective_concurrency: 1,
                generated_at: chrono::Utc::now(),
            }
        }

        async fn chat(
            &self,
            _req: ChatRequest,
            _on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            Ok(ChatResponse {
                message: Message::assistant("I am providing an answer without calling any tools."),
                usage: Default::default(),
                provenance: Default::default(),
            })
        }
    }

    #[tokio::test]
    async fn test_empty_tools_limited_invokes_abort_staged_mutations() {
        let aborted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let aborted_clone = aborted.clone();
        let abort_hook: AbortStaged = Arc::new(move || {
            aborted_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        });

        let provider = Arc::new(MockEmptyToolProvider);
        let agent =
            Agent::new(provider, DummyHost, AgentConfig::default()).with_abort_staged(abort_hook);

        let mut convo = Conversation::new();
        let outcome = agent.turn(&mut convo, test_inv("hello"), |_| {}).await;

        assert!(matches!(
            outcome,
            CandidateOutcome::Limited {
                kind: LimitKind::EmptyTools,
                ..
            }
        ));
        assert!(
            aborted.load(std::sync::atomic::Ordering::SeqCst),
            "abort_staged hook must be invoked when turn finishes with LimitKind::EmptyTools"
        );
    }
}

#[cfg(test)]
#[path = "blocking_work_tests.rs"]
mod blocking_work_tests;
