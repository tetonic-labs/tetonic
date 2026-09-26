//! Turn orchestration and agent assembly (Slice 3 execution path).

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tetonic_core::{
    Agent, ApprovalRequest, CaptureWorkspaceVersion, Conversation, PostEditSnapshot,
    ResolveUnderRoot, SpawnHook, Step, Tokenizer,
};
use tetonic_domain::secrets::{OutboundRedaction, OutboundRedactionSink};
use tetonic_domain::{
    AgentInvocation, AttemptId, CandidateOutcome, CodeIndexOpen, LspSessionOpen, TaskId,
};
use tetonic_inference::FabricCallMeta;
use tetonic_inference::InferenceProvider;
use tetonic_memory::RecoverMutex;
use tetonic_orchestrator::{
    format_orchestration_log, format_router_log, llm_route_task, run_orchestrated_turn,
    run_spawned_specialist, AgentBuildRequest, ChildJob, OrchestratedTurnInput, OrchestrationMode,
    RootExecute, RouteContext, RouteMode, SpawnBudgetGate, SpawnHost, SpawnLimits,
    SpawnSessionTrack, SpecialistPack,
};

use crate::coding_pack::CodingPack;
use crate::definition::CodingAgentDefinition;

type AgentBuildFn =
    dyn FnMut(AgentBuildRequest, &str) -> Result<(Agent, AgentInvocation), String> + Send;
use tetonic_context::workspace::{build_production_context_compiler_injected, ContextFsHooks};
use tetonic_runtime::{
    base_agent_config, AgentAssemblyParts, AgentConfigInput, AssemblyMode, EngineRuntime,
    NullAudit, ProductionApproval,
};

#[derive(Clone)]
struct AppRootExecute {
    runs: Arc<dyn RunService>,
    envelopes: Arc<Mutex<std::collections::HashMap<String, crate::events::EventEnvelope>>>,
    runtime: Arc<EngineRuntime>,
    approval_hook: tetonic_core::ApprovalHook,
    /// Session and agent to mark started only after the specialist is built.
    announce_start: Option<(String, String)>,
    /// Root-turn sink. Admission does not emit `started`; the first execution does.
    events: Option<Arc<dyn ApplicationEventSink>>,
    started_once: Arc<AtomicBool>,
}

struct AttemptApprovalGuard {
    runtime: Arc<EngineRuntime>,
    attempt_id: AttemptId,
}

impl Drop for AttemptApprovalGuard {
    fn drop(&mut self) {
        self.runtime
            .action_broker()
            .unregister_attempt_approval(&self.attempt_id);
    }
}

#[async_trait::async_trait]
impl RootExecute for AppRootExecute {
    async fn execute(
        &self,
        attempt_id: AttemptId,
        agent: &mut tetonic_core::Agent,
        conversation: &mut Conversation,
        invocation: AgentInvocation,
        on_step: &mut (dyn FnMut(Step) + Send),
    ) -> CandidateOutcome {
        let envelope = crate::events::EventEnvelope {
            run_id: self.runs.run_id_for_attempt(&attempt_id).map(|id| id.0),
            task_id: self.runs.task_id_for_attempt(&attempt_id).map(|id| id.0),
            attempt_id: Some(attempt_id.0.clone()),
            identity_id: self
                .runs
                .active_job_spec(&attempt_id)
                .map(|spec| spec.identity_id.0),
        };
        self.envelopes
            .lock_recover()
            .insert(agent.execution_agent_id().into(), envelope.clone());
        self.runtime
            .action_broker()
            .register_attempt_approval(attempt_id.clone(), self.approval_hook.clone());
        let _guard = AttemptApprovalGuard {
            runtime: self.runtime.clone(),
            attempt_id: attempt_id.clone(),
        };
        if let Some(sink) = &self.events {
            if self
                .started_once
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                if let Some(run_id) = envelope.run_id.clone() {
                    emit(
                        sink,
                        ApplicationEvent::run_status(
                            run_id,
                            "started".into(),
                            None,
                            None,
                            &envelope,
                        ),
                    );
                }
            }
        }
        if let Some((session_id, agent_id)) = &self.announce_start {
            let _ = self
                .runs
                .report_run_status(&crate::commands::ReportRunStatusCommand {
                    session_id: session_id.clone(),
                    status: "started".into(),
                    agent_id: Some(agent_id.clone()),
                    error: None,
                })
                .await;
        }
        self.runs
            .execute_attempt(attempt_id, agent, conversation, invocation, on_step)
            .await
    }
}
use tetonic_domain::CompletionKind;
use tetonic_secrets::ScannerEngine;
use tetonic_tools::{EnforcementLevel, Tools, Workspace};

use crate::approval::ApprovalService;
use crate::commands::{
    CompleteTurnCommand, RegisterApprovalCommand, RunTurnCommand, SpawnAgentCommand,
};
use crate::errors::AppError;
use crate::events::{emit, make_specialist_node_meta, ApplicationEvent, ApplicationEventSink};
use crate::redaction_audit::{MissingStoreRedactionSink, StoreRedactionSink};
use crate::services::{FinalizationEffectDriver, FinalizationPolicy, RunService, RunTurnPlan};
use crate::spawn_budget::LedgerSpawnBudgetGate;

pub(crate) struct ToolsFinalizationDriver(pub(crate) std::sync::Arc<tetonic_tools::Tools>);

impl FinalizationEffectDriver for ToolsFinalizationDriver {
    fn bind_effect_identity(&self, task_id: &TaskId, attempt_id: &AttemptId) -> Result<(), String> {
        self.0
            .bind_effect_identity(task_id.clone(), attempt_id.clone())
            .map_err(|e| e.to_string())
    }

    fn run_verify(
        &self,
        verify_cmd: &str,
        _cancel: &tetonic_domain::work_scope::CancellationSignal,
    ) -> Result<(), (String, Option<String>)> {
        let (ok, output) = self.0.run_command_cancellable(verify_cmd, Some(_cancel));
        if ok {
            Ok(())
        } else {
            let hint = tetonic_tools::summarize_verify_failure(&output);
            Err((output, hint))
        }
    }

    fn commit_workspace(&self) -> Result<Option<tetonic_domain::CommitResult>, String> {
        self.0.commit_staged_if_any().map_err(|e| e.to_string())
    }
}

struct SharedTokenizer(Arc<dyn Tokenizer>);

impl Tokenizer for SharedTokenizer {
    fn count(&self, text: &str) -> usize {
        self.0.count(text)
    }
}

fn product_code_index(index_db: &Option<std::path::PathBuf>) -> Option<Arc<dyn CodeIndexOpen>> {
    index_db
        .as_ref()
        .map(|_| Arc::new(tetonic_index::FilesystemCodeIndex) as Arc<dyn CodeIndexOpen>)
}

fn product_lsp_open(
    runtime: &EngineRuntime,
    workspace_root: &str,
) -> Option<Arc<dyn LspSessionOpen>> {
    let consumer: Arc<dyn tetonic_domain::CapabilityConsumer> = runtime.capability_store().clone();
    let open = crate::lsp_session::SandboxLspSessionOpen::new(Some(consumer));
    if open.available(Path::new(workspace_root)) {
        Some(Arc::new(open) as Arc<dyn LspSessionOpen>)
    } else {
        None
    }
}

/// Transport-provided execution host for `RunService::run_turn`.
#[derive(Clone)]
pub(crate) struct TurnExecutionHost {
    pub live_session: Option<std::sync::Weak<crate::session_live::LiveSession>>,
    pub runtime: Arc<EngineRuntime>,
    pub provider: Arc<dyn InferenceProvider>,
    pub store: Option<tetonic_memory::SharedStore>,
    pub index_db: Option<std::path::PathBuf>,
    pub workspace_root: String,
    pub tool_workspace: std::path::PathBuf,
    pub model_fast: String,
    pub model_hard: String,
    pub num_ctx: u32,
    pub session_max_steps: usize,
    pub explicit_hard_tier: bool,
    pub orchestration: OrchestrationMode,
    pub critic_enabled: bool,
    #[allow(dead_code)]
    pub llm_router: bool,
    pub plan: tetonic_orchestrator::SessionStartPlan,
    pub session_id: String,
    pub allow_shell: bool,
    pub force_explain: bool,
    pub spawn_limits: SpawnLimits,
    pub tokenizer: Arc<dyn Tokenizer>,
    pub cancel: Arc<AtomicBool>,
    pub approvals: Arc<dyn ApprovalService>,
    pub audit_factory: Option<Arc<dyn AuditFactory>>,
    pub spawn_track: Option<Arc<SpawnSessionTrack>>,
    pub turn_spawn_count: Arc<AtomicU32>,
    /// When set, verify/test process jobs admit through ComputeBroker (M6-1).
    pub compute_broker: Option<Arc<tetonic_broker::DefaultComputeBroker>>,
    /// R4-3: ScannerEngine for tool/event payload redaction.
    pub secret_scanner: Option<Arc<ScannerEngine>>,
    pub redaction_sink: Option<Arc<dyn OutboundRedactionSink>>,
    pub auto_grant_approvals: bool,
}

/// Builds session-scoped audit sinks for agent assembly.
pub trait AuditFactory: Send + Sync {
    fn session_audit(&self, session_id: &str, agent_id: &str) -> Box<dyn tetonic_core::AuditSink>;
}

pub(crate) fn redact_failure_text(
    scanner: Option<&ScannerEngine>,
    sink: Option<&dyn OutboundRedactionSink>,
    session_id: &str,
    text: String,
) -> String {
    redact_step_text(scanner, sink, session_id, "failure", text.clone()).unwrap_or(text)
}

fn redact_outcome(host: &TurnExecutionHost, outcome: CandidateOutcome) -> CandidateOutcome {
    let redact = |text: String| {
        redact_failure_text(
            host.secret_scanner.as_deref(),
            host.redaction_sink.as_deref(),
            &host.session_id,
            text,
        )
    };
    match outcome {
        CandidateOutcome::Failed { message } => CandidateOutcome::Failed {
            message: redact(message),
        },
        CandidateOutcome::Limited { kind, message } => CandidateOutcome::Limited {
            kind,
            message: redact(message),
        },
        CandidateOutcome::Canceled { reason } => CandidateOutcome::Canceled {
            reason: redact(reason),
        },
        other => other,
    }
}

fn redact_step_text(
    scanner: Option<&ScannerEngine>,
    sink: Option<&dyn OutboundRedactionSink>,
    session_id: &str,
    role: &str,
    text: String,
) -> Option<String> {
    let scanner = scanner?;
    match tetonic_secrets::redact_text_sync(scanner, &text) {
        Ok((out, hit)) => {
            if !hit {
                return Some(text);
            }
            if let Some(sink) = sink {
                let omitted = out.is_empty();
                let _ = sink.record(&OutboundRedaction {
                    session_id: Some(session_id.to_string()),
                    model: String::new(),
                    role: role.to_string(),
                    message_index: 0,
                    omitted,
                    records: Vec::new(),
                });
            }
            Some(if out.is_empty() {
                "[omitted — secret material withheld]".into()
            } else {
                out
            })
        }
        Err(_) => Some(tetonic_secrets::SCAN_FAILED_PLACEHOLDER.to_string()),
    }
}

fn redact_step_json(
    scanner: Option<&ScannerEngine>,
    value: serde_json::Value,
) -> Option<serde_json::Value> {
    let scanner = scanner?;
    match tetonic_secrets::redact_json_value(scanner, &value) {
        Ok((out, _)) => Some(out),
        Err(_) => Some(serde_json::Value::String(
            tetonic_secrets::SCAN_FAILED_PLACEHOLDER.to_string(),
        )),
    }
}

pub fn step_to_events(
    events: &Arc<dyn ApplicationEventSink>,
    session_id: &str,
    agent_id: &str,
    step: Step,
    scanner: Option<&ScannerEngine>,
    sink: Option<&dyn OutboundRedactionSink>,
    envelope: Option<&crate::events::EventEnvelope>,
) {
    let run_id = envelope.and_then(|e| e.run_id.clone());
    let task_id = envelope.and_then(|e| e.task_id.clone());
    let attempt_id = envelope.and_then(|e| e.attempt_id.clone());
    let identity_id = envelope.and_then(|e| e.identity_id.clone());
    match step {
        Step::Token(token) => {
            let Some(token) = redact_step_text(scanner, sink, session_id, "token", token) else {
                return;
            };
            emit(
                events,
                ApplicationEvent::ModelToken {
                    session_id: session_id.to_string(),
                    agent_id: agent_id.to_string(),
                    token,
                    run_id,
                    task_id,
                    attempt_id,
                    identity_id,
                },
            )
        }
        Step::Thought(token) => {
            let Some(token) = redact_step_text(scanner, sink, session_id, "thought", token) else {
                return;
            };
            emit(
                events,
                ApplicationEvent::ThoughtToken {
                    session_id: session_id.to_string(),
                    agent_id: agent_id.to_string(),
                    token,
                    run_id,
                    task_id,
                    attempt_id,
                    identity_id,
                },
            )
        }
        Step::Context(r) => emit(
            events,
            ApplicationEvent::ContextSnapshot {
                session_id: session_id.to_string(),
                agent_id: agent_id.to_string(),
                system_tokens: r.system_tokens,
                tools_tokens: r.tools_tokens,
                conversation_tokens: r.conversation_tokens,
                total_tokens: r.total_tokens,
                budget: r.budget,
                dropped_messages: r.dropped_messages,
                estimated: r.estimated,
                data_class: r.data_class,
            },
        ),
        Step::Note(text) => {
            let Some(message) =
                redact_step_text(scanner, sink, session_id, "note", text.to_string())
            else {
                return;
            };
            emit(
                events,
                ApplicationEvent::LogDiagnostic {
                    session_id: Some(session_id.to_string()),
                    agent_id: Some(agent_id.to_string()),
                    message,
                },
            )
        }
        Step::Generation(u) => emit(
            events,
            ApplicationEvent::LogDiagnostic {
                session_id: Some(session_id.to_string()),
                agent_id: Some(agent_id.to_string()),
                message: format!(
                    "gen prefill {:?} decode {:?}",
                    u.prompt_tokens, u.eval_tokens
                ),
            },
        ),
        Step::ToolCall {
            call_id,
            name,
            args,
        } => {
            let Some(args) = redact_step_json(scanner, args) else {
                return;
            };
            let parent_agent_id = if agent_id.contains('.') {
                agent_id.rsplit_once('.').map(|(p, _)| p.to_string())
            } else {
                None
            };
            emit(
                events,
                ApplicationEvent::ToolCall {
                    session_id: session_id.to_string(),
                    agent_id: agent_id.to_string(),
                    call_id,
                    tool: name,
                    args,
                    parent_agent_id,
                    run_id,
                    task_id,
                    attempt_id,
                    identity_id,
                },
            )
        }
        Step::ToolResult {
            call_id,
            name,
            ok,
            summary,
        } => {
            let Some(summary) = redact_step_text(scanner, sink, session_id, "tool", summary) else {
                return;
            };
            let parent_agent_id = if agent_id.contains('.') {
                agent_id.rsplit_once('.').map(|(p, _)| p.to_string())
            } else {
                None
            };
            emit(
                events,
                ApplicationEvent::ToolResult {
                    session_id: session_id.to_string(),
                    agent_id: agent_id.to_string(),
                    call_id,
                    tool: name.clone(),
                    ok,
                    summary: summary.clone(),
                    error_kind: if !ok && summary.to_ascii_lowercase().contains("denied") {
                        Some("denied".into())
                    } else {
                        None
                    },
                    parent_agent_id,
                    run_id,
                    task_id,
                    attempt_id,
                    identity_id,
                },
            );
            // R27: mutating tools produce a semantic workspace-mutation effect
            // on the shared execute_turn path (CLI + daemon).
            if ok && matches!(name.as_str(), "write_file" | "edit_file") {
                emit(events, ApplicationEvent::WorkspaceDiff { diff: name });
            }
        }
        Step::Answer(text) => {
            let Some(text) =
                redact_step_text(scanner, sink, session_id, "answer", text.to_string())
            else {
                return;
            };
            emit(
                events,
                ApplicationEvent::TurnAnswer {
                    session_id: session_id.to_string(),
                    agent_id: agent_id.to_string(),
                    text,
                    from_finish: false,
                    run_id,
                    task_id,
                    attempt_id,
                    identity_id,
                },
            )
        }
        Step::Stopped(reason) => {
            let Some(message) =
                redact_step_text(scanner, sink, session_id, "stopped", reason.to_string())
            else {
                return;
            };
            emit(
                events,
                ApplicationEvent::LogDiagnostic {
                    session_id: Some(session_id.to_string()),
                    agent_id: Some(agent_id.to_string()),
                    message: format!("stopped: {message}"),
                },
            )
        }
    }
}

/// Build ScannerEngine + redaction audit sink for outbound event edges (R4-3).
pub fn outbound_event_scanner(
    store: &Option<tetonic_memory::SharedStore>,
) -> (Arc<ScannerEngine>, Arc<dyn OutboundRedactionSink>) {
    let scanner = crate::secret_scanner_factory::scanner_from_shared_store(store);
    let sink: Arc<dyn OutboundRedactionSink> = match store {
        Some(s) => Arc::new(StoreRedactionSink::new(s.clone())),
        None => Arc::new(MissingStoreRedactionSink),
    };
    (scanner, sink)
}

pub(crate) fn interactive_approval_hook(
    approvals: Arc<dyn ApprovalService>,
    session_id: String,
    allow_shell: bool,
    workspace: std::path::PathBuf,
    auto_grant_approvals: bool,
) -> tetonic_core::ApprovalHook {
    Arc::new(move |req: ApprovalRequest| {
        let approvals = approvals.clone();
        let session_id = session_id.clone();
        let workspace = workspace.clone();
        Box::pin(async move {
            let mut req = req;
            crate::approval::attach_shell_confinement(&mut req, &workspace);

            if !crate::approval::must_prompt_interactively(&req) {
                if allow_shell && crate::approval::is_shell_approval(&req) {
                    return true;
                }
                if let Ok(Some(auto)) = approvals.preapprove(&req) {
                    return auto;
                }
            }

            let detail = crate::approval::approval_detail(&req);
            let approval_id = format!("cli_{}", req.call_id);
            let rx = match approvals.register_request(RegisterApprovalCommand {
                session_id: session_id.clone(),
                approval_id: approval_id.clone(),
                call_id: req.call_id.clone(),
                kind: req.kind.clone(),
                detail: detail.clone(),
                tool: req.tool.clone(),
                args: req.args.clone(),
                missing_controls: req.missing_controls.clone(),
                user_approval_required: req.user_approval_required,
                auto_grant_approvals,
                attempt_id: req.attempt_id.clone(),
            }) {
                Ok(rx) => rx,
                Err(_) => return false,
            };
            rx.await.unwrap_or(false)
        })
    })
}

pub(crate) fn parse_spawn_role(role: &str) -> Result<tetonic_orchestrator::RoleId, AppError> {
    CodingPack
        .parse(role)
        .ok_or_else(|| AppError::InvalidRequest(format!("unknown role: {role}")))
}

fn compile_build_agent_config(
    host: &TurnExecutionHost,
    turn: Option<&RunTurnPlan>,
    build: &AgentBuildRequest,
) -> tetonic_core::AgentConfig {
    let model = if build.use_hard_model {
        host.model_hard.clone()
    } else {
        host.model_fast.clone()
    };
    let tier = if build.use_hard_model {
        "hard".to_string()
    } else {
        "fast".to_string()
    };
    let agent_id = build.agent_id.clone();
    let mut config = base_agent_config(AgentConfigInput {
        model,
        agent_id: agent_id.clone(),
        session_id: Some(host.session_id.clone()),
        run_id: turn.map(|turn| turn.run_id.to_string()),
        task_id: turn.map(|turn| turn.task_id.to_string()),
        attempt_id: if build.role.is_some() || build.dynamic_spec.is_some() || build.spawned {
            None
        } else {
            turn.map(|turn| turn.attempt_id.to_string())
        },
        workspace_root: Some(host.tool_workspace.clone()),
        verify_cmd: host.plan.verify_cmd.clone(),
        briefing: if build.role.is_some() || build.dynamic_spec.is_some() {
            None
        } else {
            host.plan.briefing.clone()
        },
        project_context: host.plan.project_context.clone(),
        model_tier: Some(tier),
        explain_turn: build.explain_turn || host.force_explain,
        num_ctx: host.num_ctx as usize,
        max_steps: build.max_steps.unwrap_or(host.session_max_steps),
        context_reserve: 1024,
        data_class: host.plan.data_class,
        disclosure_tier: host.plan.disclosure_tier,
    });
    config.inherited_workspace_version = build.inherited_workspace_version.clone();
    if let Some(spec) = &build.dynamic_spec {
        config.system_overlay = Some(spec.system_overlay.clone());
        config.specialist_role = Some(spec.role_name.clone());
        if let Some(ms) = build.max_steps {
            config.max_steps = ms;
        }
    } else if let Some(r) = &build.role {
        config = CodingAgentDefinition::production().apply_role(&config, r, &agent_id);
    }
    config
}

pub(crate) fn composition_capability_hooks() -> (PostEditSnapshot, ResolveUnderRoot, CaptureWorkspaceVersion) {
    (
        Arc::new(tetonic_tools::format_post_edit_snapshot),
        Arc::new(|root, rel| {
            let abs = tetonic_transaction::fs_ops::resolve_under_root(root, rel).map_err(|_| ())?;
            std::fs::metadata(abs).map(|m| m.len()).map_err(|_| ())
        }),
        Arc::new(|root, paths| {
            tetonic_transaction::version::capture_workspace_version(root, paths)
                .map_err(|e| e.to_string())
        }),
    )
}

fn git_args_leave_workspace(args: &[&str]) -> bool {
    args.iter().any(|arg| {
        let arg = arg.replace('\\', "/");
        arg == ".." || arg.starts_with("../") || arg.contains("/../")
    })
}

fn reserved_markers(reserved: &[std::path::PathBuf], root: &Path) -> Vec<String> {
    let mut markers = Vec::new();
    for path in reserved {
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            let name = name.to_ascii_lowercase();
            if !name.is_empty() {
                markers.push(name);
            }
        }
        if let Ok(rel) = path.strip_prefix(root) {
            let rel = rel.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
            if !rel.is_empty() {
                markers.push(rel);
            }
        }
    }
    markers
}

fn line_mentions_reserved(line: &str, markers: &[String]) -> bool {
    let lower = line.replace('\\', "/").to_ascii_lowercase();
    markers.iter().any(|marker| lower.contains(marker))
}

fn git_line_names_sqlite(line: &str, root: &Path) -> bool {
    let mut candidates = Vec::new();
    if let Some(rest) = line.trim().strip_prefix("diff --git ") {
        candidates.extend(rest.split_whitespace().map(str::to_string));
    } else {
        let raw = line.trim_end();
        if raw.len() > 3 {
            candidates.push(raw[3..].trim().trim_matches('"').to_string());
        }
    }
    candidates.into_iter().any(|token| {
        let rel = token
            .strip_prefix("a/")
            .or_else(|| token.strip_prefix("b/"))
            .unwrap_or(token.as_str());
        if rel.is_empty() || rel.contains("..") || rel.contains('\0') {
            return false;
        }
        tetonic_context::workspace::path_is_sqlite_store_family(&root.join(rel))
    })
}

/// Drop git diff sections and status lines for the protected store or any live
/// SQLite database. A text diff of that file would otherwise enter the model prompt.
pub(crate) fn without_reserved_git_output(
    output: &str,
    reserved: &[std::path::PathBuf],
    root: &Path,
) -> String {
    let markers = reserved_markers(reserved, root);
    let hidden = |line: &str| {
        line_mentions_reserved(line, &markers) || git_line_names_sqlite(line, root)
    };
    if !output.contains("diff --git") {
        return output
            .lines()
            .filter(|line| !hidden(line))
            .collect::<Vec<_>>()
            .join("\n");
    }
    let mut kept = String::new();
    let mut section = String::new();
    let mut drop_section = false;
    let mut in_section = false;
    for line in output.lines() {
        if line.starts_with("diff --git") {
            if in_section && !drop_section {
                kept.push_str(&section);
            }
            in_section = true;
            drop_section = hidden(line);
            section = String::new();
            if !drop_section {
                section.push_str(line);
                section.push('\n');
            }
        } else if !in_section || !drop_section {
            if in_section {
                section.push_str(line);
                section.push('\n');
            } else if !hidden(line) {
                kept.push_str(line);
                kept.push('\n');
            }
        }
    }
    if in_section && !drop_section {
        kept.push_str(&section);
    }
    kept
}

pub(crate) fn composition_fs_hooks(reserved: Vec<std::path::PathBuf>) -> ContextFsHooks {
    let git_reserved = reserved.clone();
    ContextFsHooks {
        skip_symlink: Arc::new(tetonic_transaction::fs_ops::is_symlink_or_reparse),
        jailed_read: Arc::new(move |root, rel| {
            let ws = Workspace::new(root).map_err(|e| e.to_string())?;
            let path = ws.resolve(rel).map_err(|e| e.to_string())?;
            if tetonic_tools::path_is_reserved(&reserved, &path) {
                return Err("file is outside this execution grant".into());
            }
            tetonic_tools::read_to_string_nofollow(&path).map_err(|e| e.to_string())
        }),
        run_git: Arc::new(move |root, args| {
            if git_args_leave_workspace(args) {
                return Err("git command is outside this execution grant".into());
            }
            let pe = tetonic_tools::coding_executor(root, EnforcementLevel::Sandboxed);
            let r = pe.run_git(args.iter().map(|s| (*s).to_string()))?;
            Ok(without_reserved_git_output(&r.output, &git_reserved, root))
        }),
    }
}

fn ensure_turn_tools(host: &TurnExecutionHost) -> Result<Arc<Tools>, AppError> {
    let memory_db = host
        .store
        .as_ref()
        .and_then(|s| s.read_sync(|db| db.path().to_path_buf()).ok());
    let ws = Workspace::new(&host.tool_workspace)
        .map_err(|e| AppError::InvalidRequest(format!("workspace: {e}")))?;
    let mut tools = Tools::new(ws, host.allow_shell)
        .with_enforcement_level(EnforcementLevel::Sandboxed)
        .with_capability_consumer(host.runtime.capability_store().clone());
    if let Some(db) = host.index_db.clone() {
        tools = tools.with_index(db);
    }
    if let Some(open) = product_code_index(&host.index_db) {
        tools = tools.with_code_index_open(open);
    }
    if let Some(mem) = memory_db {
        tools = tools.with_memory(&mem, Some(host.session_id.clone()));
    }
    if let Some(open) = product_lsp_open(&host.runtime, &host.workspace_root) {
        tools = tools.with_lsp_open(open);
    }
    Ok(Arc::new(tools))
}

struct ExecutionProjectionAudit {
    inner: Box<dyn tetonic_core::AuditSink>,
    store: Option<tetonic_memory::SharedStore>,
    session_id: String,
    agent_id: String,
    plan_user: String,
    skipped_plan_user: Arc<AtomicBool>,
}

impl ExecutionProjectionAudit {
    fn persist(
        &self,
        role: &str,
        content: &str,
        tool_calls_json: Option<&str>,
        tool_name: Option<&str>,
        tool_call_id: Option<&str>,
    ) {
        let Some(store) = &self.store else {
            return;
        };
        let session = self.session_id.clone();
        let agent_id = self.agent_id.clone();
        let role = role.to_string();
        let content = content.to_string();
        let tool_calls_json = tool_calls_json.map(str::to_string);
        let tool_name = tool_name.map(str::to_string);
        let tool_call_id = tool_call_id.map(str::to_string);
        if store
            .write_sync(move |db| {
                db.require_legacy_session(&session)?;
                db.append_message_with(
                    &session,
                    &role,
                    &agent_id,
                    &content,
                    tool_calls_json.as_deref(),
                    tool_name.as_deref(),
                    tool_call_id.as_deref(),
                )
            })
            .is_err()
        {
            tracing::warn!("product persist failed");
        }
    }
}

impl tetonic_core::AuditSink for ExecutionProjectionAudit {
    fn message(&self, role: &str, content: &str, tool_calls_json: Option<&str>) {
        if role == "user"
            && content == self.plan_user
            && !self.skipped_plan_user.swap(true, Ordering::SeqCst)
        {
            return;
        }
        self.persist(role, content, tool_calls_json, None, None);
    }

    fn tool_message(&self, name: &str, tool_call_id: &str, content: &str) {
        self.persist("tool", content, None, Some(name), Some(tool_call_id));
    }

    fn tool_call(
        &self,
        id: &str,
        tool: &str,
        args_json: &str,
        ok: bool,
        summary: &str,
        error_kind: Option<&str>,
    ) {
        self.inner
            .tool_call(id, tool, args_json, ok, summary, error_kind);
    }

    fn file_change(
        &self,
        tool_call_id: &str,
        path: &str,
        kind: &str,
        before: Option<&str>,
        after: Option<&str>,
    ) {
        self.inner
            .file_change(tool_call_id, path, kind, before, after);
    }

    fn note(&self, text: &str) {
        self.inner.note(text);
    }

    fn audit_persists(&self) -> bool {
        self.inner.audit_persists()
    }
}

struct BuildAgentParts<'a> {
    tokenizer: Arc<dyn Tokenizer>,
    spawn_hook: Option<SpawnHook>,
    user_input: &'a str,
    turn_tools: &'a Arc<Tools>,
    plan_user: &'a str,
    skipped_plan_user: &'a Arc<AtomicBool>,
}

fn build_agent(
    host: &TurnExecutionHost,
    turn: Option<&RunTurnPlan>,
    build: AgentBuildRequest,
    parts: BuildAgentParts<'_>,
) -> Result<(Agent, tetonic_domain::AgentInvocation), AppError> {
    let session_id = host.session_id.clone();
    let memory_db = host
        .store
        .as_ref()
        .and_then(|s| s.read_sync(|db| db.path().to_path_buf()).ok());
    let mut tools = parts
        .turn_tools
        .as_ref()
        .clone()
        .with_orchestration(build.orchestration_tools);
    if let Some(spec) = &build.dynamic_spec {
        if let Some(names) = &spec.allowed_tools {
            let set: std::collections::HashSet<String> = names.iter().cloned().collect();
            tools = tools.with_allowed_tools(set);
        }
    } else if let Some(r) = &build.role {
        tools = if build.spawned {
            let mut t = CodingPack::apply_spawn_tool_filter(r, tools);
            if host.allow_shell && matches!(r.as_str(), "coder" | "debugger") {
                t = t.allow_tool("run_shell");
            }
            t
        } else {
            CodingPack::apply_tool_filter(r, tools)
        };
    }

    let abort_tools = tools.clone();
    let agent_id = build.agent_id.clone();
    let mut config = compile_build_agent_config(host, turn, &build);
    config.workspace_root = Some(tools.workspace().root().to_path_buf());
    // Routed roles and critic/revision turns share the root conversation. Keep
    // its model-facing tool grammar fixed; the filtered host still controls
    // execution and the manager still sees only the role's active capabilities.
    // Spawned/dynamic agents retain their own least-privilege catalog.
    let stable_catalog = if !build.spawned && build.dynamic_spec.is_none() {
        let catalog_tools = parts.turn_tools.as_ref().clone().with_orchestration(
            host.orchestration == tetonic_orchestrator::OrchestrationMode::Auto,
        );
        let catalog = tetonic_domain::ToolHost::advertisements(&catalog_tools)
            .into_iter()
            .map(|t| tetonic_inference::ToolSchema::function(&t.name, &t.description, t.parameters))
            .collect::<Vec<_>>();
        let mut active = tetonic_domain::ToolHost::advertisements(&tools)
            .into_iter()
            .map(|t| t.name)
            .collect::<Vec<_>>();
        active.sort();
        let overlay = config.system_overlay.take().unwrap_or_else(|| {
            "Handle the current request as the primary coding assistant.".into()
        });
        let instructions = format!(
            "Current turn execution scope (supersedes earlier turn scopes):\n{overlay}\n\
             The tool catalog is shared across roles and is not a permission grant.\n\
             For this turn, use ONLY these tools: {}. All other tools are unavailable.\n\
             This scope remains in force through tool results until the next turn scope.",
            active.join(", ")
        );
        Some((catalog, instructions))
    } else {
        None
    };
    let invocation = CodingAgentDefinition::production().compile_invocation_from_tools(
        &tools,
        &config,
        parts.user_input,
    );

    let inner = host
        .audit_factory
        .as_ref()
        .map(|f| f.session_audit(&session_id, &agent_id))
        .unwrap_or_else(|| Box::new(NullAudit) as Box<dyn tetonic_core::AuditSink>);
    let audit = if turn.is_some() {
        Box::new(ExecutionProjectionAudit {
            inner,
            store: host.store.clone(),
            session_id: session_id.clone(),
            agent_id: agent_id.clone(),
            plan_user: parts.plan_user.to_string(),
            skipped_plan_user: parts.skipped_plan_user.clone(),
        }) as Box<dyn tetonic_core::AuditSink>
    } else {
        inner
    };

    let assembly_mode = if host.store.is_some() {
        AssemblyMode::Session
    } else {
        AssemblyMode::CliEphemeral
    };
    let hook = interactive_approval_hook(
        host.approvals.clone(),
        session_id.clone(),
        host.allow_shell,
        host.tool_workspace.clone(),
        host.auto_grant_approvals,
    );
    let approval = ProductionApproval::host(hook);

    let process_broker = Some(match &host.compute_broker {
        Some(broker) => {
            let inner: Arc<dyn tetonic_domain::sinks::ProcessBroker> =
                Arc::new(tools.executor().clone());
            Arc::new(tetonic_broker::BrokerGatedProcessBroker::new(
                broker.clone(),
                inner,
            )) as Arc<dyn tetonic_domain::sinks::ProcessBroker>
        }
        None => Arc::new(tools.executor().clone()) as Arc<dyn tetonic_domain::sinks::ProcessBroker>,
    });
    let workspace_root = config.workspace_root.clone();
    let agent = Agent::with_tokenizer(
        host.provider.clone(),
        tools,
        config,
        Box::new(SharedTokenizer(parts.tokenizer)),
    );
    let agent = match stable_catalog {
        Some((catalog, instructions)) => agent
            .with_stable_tool_catalog(catalog, instructions)
            .map_err(|e| AppError::InvalidRequest(e.to_string()))?,
        None => agent,
    };
    let (post_edit_snapshot, resolve_under_root, capture_workspace_version) =
        composition_capability_hooks();

    let context_compiler: Option<Arc<dyn tetonic_domain::ContextCompiler>> =
        workspace_root.as_ref().map(|root| {
            let compiler: Arc<dyn tetonic_domain::ContextCompiler> =
                build_production_context_compiler_injected(
                    root,
                    host.runtime.artifact_store().clone(),
                    host.index_db.clone(),
                    memory_db.clone(),
                    product_code_index(&host.index_db),
                    host.index_db.as_ref().map(|_| {
                        Arc::new(tetonic_index::IndexTextSkeleton)
                            as Arc<dyn tetonic_domain::TextSkeleton>
                    }),
                    composition_fs_hooks(
                        memory_db
                            .as_deref()
                            .map(tetonic_tools::store_sidecar_paths)
                            .unwrap_or_default(),
                    ),
                );
            compiler
        });
    if let (Some(owner), Some(compiler)) = (&host.live_session, &context_compiler) {
        let owner = owner
            .upgrade()
            .ok_or_else(|| AppError::InvalidRequest("session no longer exists".into()))?;
        owner.register_context_compiler(
            tetonic_domain::SessionId::new(&host.session_id),
            compiler,
        )?;
    }

    let agent = host
        .runtime
        .assemble_agent(
            assembly_mode,
            AgentAssemblyParts {
                agent,
                audit,
                approval,
                spawn: parts.spawn_hook,
                process_broker,
                context_compiler,
                post_edit_snapshot,
                resolve_under_root,
                capture_workspace_version,
            },
        )
        .map_err(|e| AppError::InvalidRequest(e.to_string()))?;

    Ok((
        agent.with_abort_staged(std::sync::Arc::new(move || {
            let _ = abort_tools.abort_staged_if_any();
        })),
        invocation,
    ))
}

pub(crate) async fn execute_turn(
    runs: &(dyn RunService + Sync),
    events: &Arc<dyn ApplicationEventSink>,
    cmd: &RunTurnCommand,
    host: &TurnExecutionHost,
    conversation: &mut Conversation,
    spawn_serial: &mut u32,
) -> Result<(), AppError> {
    tetonic_telemetry::propagation::scope_context(
        tetonic_telemetry::TraceContext::default(),
        execute_turn_scoped(runs, events, cmd, host, conversation, spawn_serial),
    )
    .await
}

async fn execute_turn_scoped(
    runs: &(dyn RunService + Sync),
    events: &Arc<dyn ApplicationEventSink>,
    cmd: &RunTurnCommand,
    host: &TurnExecutionHost,
    conversation: &mut Conversation,
    spawn_serial: &mut u32,
) -> Result<(), AppError> {
    let task_timer = tetonic_telemetry::StageTimer::start(tetonic_telemetry::PerfStage::TaskTurn);
    let turn_plan = runs.plan_turn(cmd).await?;
    // R02: bind live session/run/task ids on the shared CLI+daemon turn path
    // (before model/tools). Both transports call this function.
    tetonic_telemetry::inject_turn_context(
        &cmd.session_id,
        &turn_plan.run_id.0,
        Some(turn_plan.task_id.0.as_str()),
    );
    if host.cancel.load(Ordering::Relaxed)
        || runs.attempt_must_not_infer(&turn_plan.attempt_id)
    {
        host.runtime
            .action_broker()
            .unregister_attempt_approval(&turn_plan.attempt_id);
        host.approvals.fail_attempt_waits(&turn_plan.attempt_id.0);
        runs.complete_turn(
            &CompleteTurnCommand {
                session_id: cmd.session_id.clone(),
                attempt_id: turn_plan.attempt_id.clone(),
                workspace_root: host.workspace_root.clone(),
                canceled: true,
                error: None,
            },
            None,
            None,
        )
        .await?;
        task_timer.finish(false);
        return Ok(());
    }

    let res = run_turn_body(
        runs,
        events,
        cmd,
        host,
        conversation,
        spawn_serial,
        turn_plan.clone(),
    )
    .await;
    host.runtime
        .action_broker()
        .unregister_attempt_approval(&turn_plan.attempt_id);
    host.approvals.fail_attempt_waits(&turn_plan.attempt_id.0);
    task_timer.finish(res.is_ok());
    res
}

/// Drop an in-flight classifier call when stop arrives. The check before
/// `provider.chat` does not abort a call that has already started.
pub(crate) async fn stop_or<T, F, Fut>(
    cancel: &AtomicBool,
    attempt_closed: impl Fn() -> bool,
    mut authority_revoked: F,
    work: impl std::future::Future<Output = T>,
) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    tokio::select! {
        biased;
        _ = async {
            let mut next_authority = std::time::Instant::now();
            loop {
                if cancel.load(Ordering::Relaxed) || attempt_closed() {
                    break;
                }
                if std::time::Instant::now() >= next_authority {
                    if authority_revoked().await {
                        break;
                    }
                    next_authority =
                        std::time::Instant::now() + std::time::Duration::from_millis(200);
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        } => None,
        result = work => Some(result),
    }
}

/// The intent classifier sends the user text before the agent loop. It must
/// carry the session class so a secret session is not placed on a remote worker.
pub(crate) fn session_classifier_fabric(
    plan: &tetonic_orchestrator::SessionStartPlan,
    session_id: &str,
    run_id: &str,
    task_id: &str,
    attempt_id: &str,
) -> FabricCallMeta {
    FabricCallMeta {
        session_id: Some(session_id.to_string()),
        run_id: Some(run_id.to_string()),
        task_id: Some(task_id.to_string()),
        attempt_id: Some(attempt_id.to_string()),
        data_class: plan.data_class,
        disclosure_tier: plan.disclosure_tier,
        ..Default::default()
    }
}

async fn run_turn_body(
    runs: &(dyn RunService + Sync),
    events: &Arc<dyn ApplicationEventSink>,
    cmd: &RunTurnCommand,
    host: &TurnExecutionHost,
    conversation: &mut Conversation,
    spawn_serial: &mut u32,
    turn_plan: RunTurnPlan,
) -> Result<(), AppError> {
    let skipped_plan_user = Arc::new(AtomicBool::new(false));
    let plan_user = cmd.user_input.clone();
    let verify_gated = turn_plan.verify_gated;
    let llm_router = turn_plan.llm_router;
    let turn_identity = turn_plan.clone();
    let orchestration = host.orchestration;
    let critic_enabled = host.critic_enabled;

    let events_cb = events.clone();
    let session_id = host.session_id.clone();
    let scanner = host.secret_scanner.clone();
    let sink = host.redaction_sink.clone();
    let approval_hook = interactive_approval_hook(
        host.approvals.clone(),
        host.session_id.clone(),
        host.allow_shell,
        host.tool_workspace.clone(),
        host.auto_grant_approvals,
    );
    let execution_envelopes = Arc::new(Mutex::new(std::collections::HashMap::<
        String,
        crate::events::EventEnvelope,
    >::new()));
    let event_envelopes = execution_envelopes.clone();
    let on_step = Arc::new(Mutex::new(Box::new(move |aid: &str, step: Step| {
        step_to_events(
            &events_cb,
            &session_id,
            aid,
            step,
            scanner.as_deref(),
            sink.as_deref(),
            event_envelopes.lock_recover().get(aid),
        )
    }) as Box<dyn FnMut(&str, Step) + Send>));

    let turn_spawn_count = host.turn_spawn_count.clone();
    turn_spawn_count.store(0, Ordering::SeqCst);
    let spawn_serial_cell = Arc::new(AtomicU32::new(*spawn_serial));
    let session_steps = Arc::new(AtomicUsize::new(host.session_max_steps));
    let spawn_track = host
        .spawn_track
        .clone()
        .unwrap_or_else(SpawnSessionTrack::new);

    let fs_index = tetonic_index::FilesystemCodeIndex;
    let code_index_ref = host
        .index_db
        .as_ref()
        .map(|_| &fs_index as &dyn CodeIndexOpen);

    let route_attempt = turn_plan.attempt_id.clone();
    let route_cancel = host.cancel.clone();
    let route_blocked = runs.attempt_must_not_infer(&route_attempt)
        || runs.attempt_authority_revoked(&route_attempt).await;
    let llm_route = if llm_router && orchestration == OrchestrationMode::Auto && !route_blocked {
        emit(
            events,
            ApplicationEvent::StageTransition {
                session_id: host.session_id.clone(),
                agent_id: tetonic_orchestrator::ROOT_AGENT.to_string(),
                stage: crate::events::EngineStage::Routing {
                    mode: "llm_classifier".to_string(),
                },
                detail: Some("Classifying intent & selecting specialist...".to_string()),
            },
        );
        stop_or(
            &route_cancel,
            || runs.attempt_must_not_infer(&route_attempt),
            || runs.attempt_authority_revoked(&route_attempt),
            llm_route_task(
                host.provider.as_ref(),
                &host.model_fast,
                &cmd.user_input,
                true,
                RouteContext {
                    workspace_root: Path::new(&host.workspace_root),
                    index_db: host.index_db.as_deref(),
                    code_index: code_index_ref,
                    pack: &CodingPack,
                },
                session_classifier_fabric(
                    &host.plan,
                    &host.session_id,
                    &turn_plan.run_id.to_string(),
                    &turn_plan.task_id.to_string(),
                    &turn_plan.attempt_id.to_string(),
                ),
                || {
                    !route_cancel.load(Ordering::Relaxed)
                        && !runs.attempt_must_not_infer(&route_attempt)
                },
            ),
        )
        .await
    } else {
        None
    };

    let admit_spawn: Option<Arc<dyn SpawnBudgetGate>> =
        host.compute_broker.as_ref().map(|broker| {
            LedgerSpawnBudgetGate::new(broker.budgets().clone(), turn_plan.run_id.clone())
                .into_arc()
        });

    let turn_tools = ensure_turn_tools(host)?;

    let spawn_host: Option<Arc<SpawnHost>> = if orchestration == OrchestrationMode::Auto {
        let sub_host = host.clone();
        let sub_tools = turn_tools.clone();
        let tok = host.tokenizer.clone();
        let turn_spawn_count = turn_spawn_count.clone();
        let spawn_limits = host.spawn_limits;
        let spawn_serial_cell = spawn_serial_cell.clone();
        let session_steps = session_steps.clone();
        let on_step_spawn = on_step.clone();
        let sub_turn = turn_identity.clone();
        let spawn_plan_user = plan_user.clone();
        let spawn_skipped = skipped_plan_user.clone();
        let build: Arc<Mutex<AgentBuildFn>> = Arc::new(Mutex::new(
            move |build: AgentBuildRequest, user_input: &str| {
                if let Some(ref track) = sub_host.spawn_track {
                    track.register_agent(&build.agent_id);
                }
                build_agent(
                    &sub_host,
                    Some(&sub_turn),
                    build,
                    BuildAgentParts {
                        tokenizer: tok.clone(),
                        spawn_hook: None,
                        user_input,
                        turn_tools: &sub_tools,
                        plan_user: &spawn_plan_user,
                        skipped_plan_user: &spawn_skipped,
                    },
                )
                .map_err(|e| e.to_string())
            },
        ));
        Some(Arc::new(SpawnHost {
            limits: spawn_limits,
            turn_spawn_count,
            spawn_serial: spawn_serial_cell,
            session_max_steps: session_steps,
            build,
            on_step: Arc::new(Mutex::new(Box::new(move |aid: &str, step: Step| {
                on_step_spawn.lock_recover()(aid, step);
            })
                as Box<dyn for<'a> FnMut(&'a str, Step) + Send>)),
            spawn_track: spawn_track.clone(),
            session_id: host.session_id.clone(),
            admit_spawn: admit_spawn.clone(),
            pack: CodingPack::arc(),
            child_job: runs.child_job(&cmd.session_id),
            root_execute: Arc::new(AppRootExecute {
                runs: runs.execution_service(),
                envelopes: execution_envelopes.clone(),
                runtime: host.runtime.clone(),
                approval_hook: approval_hook.clone(),
                announce_start: None,
                events: None,
                started_once: Arc::new(AtomicBool::new(false)),
            }),
        }))
    } else {
        None
    };

    let workspace_root = host.workspace_root.clone();
    let index_db = host.index_db.clone();
    let user_text = cmd.user_input.clone();
    let session_prefers_hard = host.explicit_hard_tier;
    let spawn_limits = host.spawn_limits;
    let session_max_steps = host.session_max_steps;
    let cancel = host.cancel.clone();

    let host_for_turn = host.clone();
    let host_tools = turn_tools.clone();
    let body_plan_user = plan_user.clone();
    let body_skipped = skipped_plan_user.clone();
    let outcome = if cancel.load(Ordering::Relaxed)
        || runs.attempt_must_not_infer(&turn_identity.attempt_id)
    {
        Err("canceled".into())
    } else {
        run_orchestrated_turn(
            conversation,
            &OrchestratedTurnInput {
                user_text: &user_text,
                orchestration,
                critic_enabled,
                verify_gated,
                workspace_root: Path::new(&workspace_root),
                index_db: index_db.as_deref(),
                code_index: code_index_ref,
                pack: &CodingPack,
                llm_route,
                session_prefers_hard,
                spawn_limits,
                session_max_steps,
                root_attempt_id: turn_identity.attempt_id.clone(),
            },
            spawn_serial_cell.clone(),
            turn_spawn_count.clone(),
            |build, user_input| {
                if let Some(ref track) = host_for_turn.spawn_track {
                    track.register_agent(&build.agent_id);
                }
                let started_role = build.role.clone();
                let started_agent = build.agent_id.clone();
                let hook = if build.orchestration_tools {
                    spawn_host.as_ref().map(|h| h.hook())
                } else {
                    None
                };
                let built = build_agent(
                    &host_for_turn,
                    Some(&turn_identity),
                    build,
                    BuildAgentParts {
                        tokenizer: host_for_turn.tokenizer.clone(),
                        spawn_hook: hook,
                        user_input,
                        turn_tools: &host_tools,
                        plan_user: &body_plan_user,
                        skipped_plan_user: &body_skipped,
                    },
                )
                .map_err(|e| e.to_string())?;
                if let Some(role) = started_role {
                    let meta = make_specialist_node_meta(
                        &started_agent,
                        tetonic_orchestrator::ROOT_AGENT,
                        &turn_identity.run_id.to_string(),
                        &host_for_turn.session_id,
                        role.as_str(),
                    );
                    emit(events, ApplicationEvent::NodeStarted { meta });
                }
                Ok(built)
            },
            |aid, step| on_step.lock_recover()(aid, step),
            Some(|route, tier| {
                emit(
                    events,
                    ApplicationEvent::LogDiagnostic {
                        session_id: Some(host_for_turn.session_id.clone()),
                        agent_id: Some(tetonic_orchestrator::ROOT_AGENT.to_string()),
                        message: format!("router: {} [tier={tier}]", format_router_log(&route)),
                    },
                );
            }),
            AppRootExecute {
                runs: runs.execution_service(),
                envelopes: execution_envelopes.clone(),
                runtime: host.runtime.clone(),
                approval_hook: approval_hook.clone(),
                announce_start: None,
                events: Some(events.clone()),
                started_once: Arc::new(AtomicBool::new(false)),
            },
            runs.child_job(&cmd.session_id),
        )
        .await
    };

    *spawn_serial = spawn_serial_cell.load(Ordering::SeqCst);
    persist_spawn_rollbacks(&host.store, &host.session_id, &spawn_track);
    let canceled = cancel.load(Ordering::Relaxed)
        || outcome
            .as_ref()
            .is_ok_and(|o| matches!(o.outcome, CandidateOutcome::Canceled { .. }));
    let turn_error = match &outcome {
        Err(e) => Some(redact_failure_text(
            host.secret_scanner.as_deref(),
            host.redaction_sink.as_deref(),
            &host.session_id,
            e.to_string(),
        )),
        Ok(o) => match &o.outcome {
            CandidateOutcome::Failed { message } => Some(redact_failure_text(
                host.secret_scanner.as_deref(),
                host.redaction_sink.as_deref(),
                &host.session_id,
                message.clone(),
            )),
            _ => None,
        },
    };
    let is_explain = host.force_explain
        || CodingPack.root_explain_turn(&cmd.user_input)
        || outcome.as_ref().is_ok_and(|o| match &o.route.mode {
            RouteMode::Specialist(r) => CodingPack.explain_turn(r, false),
            RouteMode::Single => false,
        });
    let policy = if is_explain {
        None
    } else {
        Some(FinalizationPolicy {
            effect_driver: Some(Arc::new(ToolsFinalizationDriver(turn_tools.clone()))),
            verify_cmd: cmd
                .verify_cmd
                .clone()
                .or_else(|| host.plan.verify_cmd.clone()),
        })
    };
    let terminal = outcome
        .as_ref()
        .ok()
        .map(|o| redact_outcome(host, o.outcome.clone()));
    let finalization_timer =
        tetonic_telemetry::StageTimer::start(tetonic_telemetry::PerfStage::Finalization);
    let final_outcome = runs
        .complete_turn(
            &CompleteTurnCommand {
                session_id: cmd.session_id.clone(),
                attempt_id: turn_plan.attempt_id.clone(),
                workspace_root: host.workspace_root.clone(),
                canceled,
                error: turn_error,
            },
            terminal.as_ref(),
            policy,
        )
        .await?;

    finalization_timer.finish(matches!(final_outcome, CandidateOutcome::Completed { .. }));

    if let Ok(ref outcome) = outcome {
        emit(
            events,
            ApplicationEvent::LogDiagnostic {
                session_id: Some(host.session_id.clone()),
                agent_id: Some(tetonic_orchestrator::ROOT_AGENT.to_string()),
                message: format!("orchestration: {}", format_orchestration_log(outcome)),
            },
        );
    }

    match final_outcome {
        CandidateOutcome::Completed { .. } => {
            outcome.map_err(|e| AppError::InvalidRequest(e.to_string()))?;
            Ok(())
        }
        CandidateOutcome::Canceled { reason } => Err(AppError::InvalidRequest(reason)),
        CandidateOutcome::Failed { message } | CandidateOutcome::Limited { message, .. } => {
            Err(AppError::InvalidRequest(message))
        }
    }
}

pub fn persist_spawn_rollbacks(
    store: &Option<tetonic_memory::SharedStore>,
    session_id: &str,
    track: &SpawnSessionTrack,
) {
    if let Some(s) = store {
        for id in track.rolled_back_agents() {
            let session_id = session_id.to_string();
            let id = id.clone();
            let _ = s.write_sync(move |db| db.record_spawn_rollback(&session_id, &id));
        }
    }
}

pub(crate) async fn execute_spawn(
    runs: &(dyn RunService + Sync),
    events: &Arc<dyn ApplicationEventSink>,
    cmd: &SpawnAgentCommand,
    host: &TurnExecutionHost,
    conversation: &mut Conversation,
    spawn_serial: &mut u32,
) -> Result<(), AppError> {
    let execution_envelopes = Arc::new(Mutex::new(std::collections::HashMap::new()));
    let role = parse_spawn_role(&cmd.role)?;
    let child_job = runs.child_job(&cmd.session_id);
    let spawn_attempt_id = runs
        .register_spawn_task(&cmd.session_id, &cmd.agent_id, &cmd.role, &cmd.task)
        .await?;
    let is_root_job =
        runs.active_parent_attempt(&cmd.session_id).as_ref() == Some(&spawn_attempt_id);
    let spawn_run_id = runs
        .run_id_for_attempt(&spawn_attempt_id)
        .ok_or_else(|| AppError::InvalidRequest("spawn parent run not found".into()))?;
    conversation.begin_turn();
    let turn_tools = ensure_turn_tools(host)?;
    let spawn_serial_cell = Arc::new(AtomicU32::new(*spawn_serial));
    let turn_spawn_count = host.turn_spawn_count.clone();
    let spawn_track = host
        .spawn_track
        .clone()
        .unwrap_or_else(SpawnSessionTrack::new);
    let events_cb = events.clone();
    let session_id = host.session_id.clone();
    let scanner = host.secret_scanner.clone();
    let sink = host.redaction_sink.clone();

    let admit_spawn: Option<Arc<dyn SpawnBudgetGate>> =
        host.compute_broker.as_ref().map(|broker| {
            LedgerSpawnBudgetGate::new(broker.budgets().clone(), spawn_run_id.clone()).into_arc()
        });

    let child_task_id = runs
        .task_id_for_attempt(&spawn_attempt_id)
        .unwrap_or_else(|| tetonic_domain::TaskId::new(format!("task_{}", spawn_attempt_id.0)));
    let child_identity_id = runs
        .active_job_spec(&spawn_attempt_id)
        .map(|s| s.identity_id.0.clone())
        .or_else(|| Some(cmd.agent_id.clone()));
    let child_envelope = crate::events::EventEnvelope {
        run_id: Some(spawn_run_id.0.clone()),
        task_id: Some(child_task_id.0.clone()),
        attempt_id: Some(spawn_attempt_id.0.clone()),
        identity_id: child_identity_id,
    };

    let outcome = run_spawned_specialist(
        conversation,
        tetonic_core::SpawnRequest {
            role: cmd.role.clone(),
            task: cmd.task.clone(),
            parent_agent_id: cmd.parent_agent_id.clone(),
        },
        spawn_serial_cell.clone(),
        turn_spawn_count,
        host.spawn_limits,
        |build, user_input| {
            spawn_track.register_agent(&build.agent_id);
            build_agent(
                host,
                None,
                build,
                BuildAgentParts {
                    tokenizer: host.tokenizer.clone(),
                    spawn_hook: None,
                    user_input,
                    turn_tools: &turn_tools,
                    plan_user: "",
                    skipped_plan_user: &Arc::new(AtomicBool::new(false)),
                },
            )
            .map_err(|e| e.to_string())
        },
        |aid, step| {
            step_to_events(
                &events_cb,
                &session_id,
                aid,
                step,
                scanner.as_deref(),
                sink.as_deref(),
                Some(&child_envelope),
            )
        },
        None,
        role.clone(),
        Some(spawn_track.clone()),
        host.session_id.clone(),
        admit_spawn,
        child_job.clone(),
        AppRootExecute {
            runs: runs.execution_service(),
            envelopes: execution_envelopes.clone(),
            runtime: host.runtime.clone(),
            approval_hook: interactive_approval_hook(
                host.approvals.clone(),
                host.session_id.clone(),
                host.allow_shell,
                host.tool_workspace.clone(),
                host.auto_grant_approvals,
            ),
            announce_start: Some((cmd.session_id.clone(), cmd.agent_id.clone())),
            events: None,
            started_once: Arc::new(AtomicBool::new(false)),
        },
        Some(spawn_attempt_id.clone()),
    )
    .await;

    *spawn_serial = spawn_serial_cell.load(Ordering::SeqCst);
    persist_spawn_rollbacks(&host.store, &cmd.session_id, &spawn_track);

    let canceled = host.cancel.load(Ordering::Relaxed);
    let mapped = if outcome.ok && !canceled {
        CandidateOutcome::Completed {
            summary: outcome.summary.clone(),
            kind: CompletionKind::Finish,
        }
    } else if canceled {
        CandidateOutcome::Canceled {
            reason: outcome.summary.clone(),
        }
    } else {
        CandidateOutcome::Failed {
            message: outcome.summary.clone(),
        }
    };
    let error = (!outcome.ok && !canceled).then(|| outcome.summary.clone());
    if is_root_job {
        let is_explain = host.force_explain || CodingPack.explain_turn(&role, false);
        let policy = if is_explain {
            None
        } else {
            Some(FinalizationPolicy {
                effect_driver: Some(Arc::new(ToolsFinalizationDriver(turn_tools.clone()))),
                verify_cmd: host.plan.verify_cmd.clone(),
            })
        };
        let final_outcome = runs
            .complete_turn(
                &CompleteTurnCommand {
                    session_id: cmd.session_id.clone(),
                    attempt_id: spawn_attempt_id,
                    workspace_root: host.workspace_root.clone(),
                    canceled,
                    error,
                },
                Some(&mapped),
                policy,
            )
            .await?;
        match final_outcome {
            CandidateOutcome::Completed { .. } => Ok(()),
            CandidateOutcome::Canceled { reason } => Err(AppError::InvalidRequest(reason)),
            CandidateOutcome::Failed { message } | CandidateOutcome::Limited { message, .. } => {
                Err(AppError::InvalidRequest(message))
            }
        }
    } else {
        child_job
            .complete_child(spawn_attempt_id, mapped)
            .await
            .map_err(AppError::InvalidRequest)?;
        if outcome.ok {
            Ok(())
        } else {
            Err(AppError::InvalidRequest(outcome.summary))
        }
    }
}

#[cfg(test)]
#[path = "code01_build_agent_tests.rs"]
mod code01_build_agent_tests;

#[cfg(test)]
mod projection_audit_tests {
    use super::ExecutionProjectionAudit;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use tetonic_runtime::NullAudit;

    #[test]
    fn projection_does_not_write_a_private_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projection.db");
        let store = tetonic_memory::SharedStore::open(&path, 1).unwrap();
        store
            .write_sync(|db| {
                db.bootstrap_control("admin", "org", "Org").unwrap();
                db.create_information_context(
                    "admin",
                    "private",
                    &tetonic_memory::ContextOwner::Private {
                        org_id: "org".into(),
                    },
                )
                .unwrap();
                db.insert_open_discussion("admin", "private", "private-notes")
                    .unwrap();
            })
            .unwrap();
        let audit = ExecutionProjectionAudit {
            inner: Box::new(NullAudit),
            store: Some(store),
            session_id: "private-notes".into(),
            agent_id: "agent".into(),
            plan_user: String::new(),
            skipped_plan_user: Arc::new(AtomicBool::new(false)),
        };
        audit.persist(
            "assistant",
            "PRIVATECANARY projection",
            None,
            None,
            None,
        );
        let raw = rusqlite::Connection::open(&path).unwrap();
        let hits: i64 = raw
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE content='PRIVATECANARY projection'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(hits, 0);
    }
}
