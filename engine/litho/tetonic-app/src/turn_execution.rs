//! Shared managed-run hooks: finalization effects, outbound event projection
//! with redaction, and workspace capability/filesystem composition hooks.

use std::path::Path;
use std::sync::Arc;

use tetonic_context::workspace::ContextFsHooks;
use tetonic_core::{CaptureWorkspaceVersion, PostEditSnapshot, ResolveUnderRoot, Step};
use tetonic_domain::secrets::{OutboundRedaction, OutboundRedactionSink};
use tetonic_domain::{AttemptId, TaskId};
use tetonic_secrets::ScannerEngine;
use tetonic_tools::{EnforcementLevel, Workspace};

use crate::events::{emit, ApplicationEvent, ApplicationEventSink};
use crate::redaction_audit::{MissingStoreRedactionSink, StoreRedactionSink};
use crate::services::FinalizationEffectDriver;

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

/// Builds session-scoped audit sinks for agent assembly.
pub trait AuditFactory: Send + Sync {
    fn session_audit(&self, session_id: &str, agent_id: &str) -> Box<dyn tetonic_core::AuditSink>;
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

pub(crate) fn composition_capability_hooks(
) -> (PostEditSnapshot, ResolveUnderRoot, CaptureWorkspaceVersion) {
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
            let rel = rel
                .to_string_lossy()
                .replace('\\', "/")
                .to_ascii_lowercase();
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
    let hidden =
        |line: &str| line_mentions_reserved(line, &markers) || git_line_names_sqlite(line, root);
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

/// Production jailed-read hooks for scoped context compilers: refuse the
/// control database and its sidecars, and filter them from git output.
pub fn composition_fs_hooks(reserved: Vec<std::path::PathBuf>) -> ContextFsHooks {
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
