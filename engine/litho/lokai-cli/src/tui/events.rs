//! Apply application events onto TUI state.

use lokai_app::events::ApplicationEvent;

use super::approval::PendingApproval;
use super::failure;
use super::status::{self, TurnPhase};
use super::transcript::{self, LineKind, TranscriptLine};
use super::App;

pub fn apply(
    app: &mut App,
    event: ApplicationEvent,
    coordinator: &crate::app_kernel::TerminalApprovalCoordinator,
) {
    app.last_event_at = Some(std::time::Instant::now());
    match event {
        ApplicationEvent::StageTransition { stage, detail, .. } => {
            if app.turn_settled {
                return;
            }
            let label = detail.unwrap_or_else(|| match &stage {
                lokai_app::events::EngineStage::Routing { .. } => "Classifying intent...".into(),
                lokai_app::events::EngineStage::CompilingContext { .. } => {
                    "Compiling context...".into()
                }
                lokai_app::events::EngineStage::SearchingIndex { .. } => {
                    "Searching index...".into()
                }
                lokai_app::events::EngineStage::PreparingModel { model } => {
                    format!("Preparing model {model}...")
                }
                lokai_app::events::EngineStage::Inferring { .. } => "Generating...".into(),
                lokai_app::events::EngineStage::RunningTool { tool } => {
                    format!("Running {tool}...")
                }
            });
            app.phase = TurnPhase::Stage(stage, label);
        }
        ApplicationEvent::TurnAnswer { text, .. } => {
            if app.phase == TurnPhase::Canceled {
                return;
            }
            // Agent answers can precede critic/revision and finalization.
            // Only TurnCompleted settles the product turn.
            if !app.expand_thoughts {
                transcript::set_thought_collapsed(&mut app.transcript, true);
            }
            app.push_transcript(TranscriptLine::new(LineKind::Lokai, text));
        }
        ApplicationEvent::ModelToken { token, .. } => {
            if app.turn_settled {
                return;
            }
            app.thinking = true;
            app.phase = TurnPhase::Generating;
            transcript::append_token(&mut app.transcript, &token);
        }
        ApplicationEvent::ThoughtToken { token, .. } => {
            if app.turn_settled {
                return;
            }
            app.thinking = true;
            app.phase = TurnPhase::Generating;
            transcript::append_thought_token(&mut app.transcript, &token, !app.expand_thoughts);
        }
        ApplicationEvent::NodeStarted { meta } => {
            let role = match meta.kind {
                lokai_app::events::NodeKind::Specialist { role } => {
                    transcript::AgentRoleKind::Specialist(role)
                }
                lokai_app::events::NodeKind::Evaluator { .. } => transcript::AgentRoleKind::Critic,
                lokai_app::events::NodeKind::RefinementLoop { iteration, .. } => {
                    transcript::AgentRoleKind::Revision(iteration)
                }
                lokai_app::events::NodeKind::Root => transcript::AgentRoleKind::Primary,
                lokai_app::events::NodeKind::ToolExecution { tool } => {
                    transcript::AgentRoleKind::Specialist(tool)
                }
            };
            if role != transcript::AgentRoleKind::Primary {
                app.push_transcript(TranscriptLine::new(
                    LineKind::SubagentHeader {
                        role,
                        agent_id: meta.node_id.clone(),
                        label: meta.label.clone(),
                    },
                    meta.label,
                ));
            }
        }
        ApplicationEvent::NodeProgress {
            state: lokai_app::events::NodeState::Active { status_message },
            ..
        } => {
            if app.turn_settled {
                return;
            }
            app.phase = TurnPhase::Stage(
                lokai_app::events::EngineStage::Inferring {
                    agent_id: "".into(),
                },
                status_message,
            );
        }
        ApplicationEvent::NodeCompleted { state, .. } => match state {
            lokai_app::events::NodeState::Succeeded { summary } => {
                app.push_transcript(TranscriptLine::new(
                    LineKind::SubagentFooter {
                        ok: true,
                        summary: summary.clone(),
                    },
                    summary,
                ));
            }
            lokai_app::events::NodeState::Failed { error, .. } => {
                app.push_transcript(TranscriptLine::new(
                    LineKind::SubagentFooter {
                        ok: false,
                        summary: error.clone(),
                    },
                    error,
                ));
            }
            _ => {}
        },
        ApplicationEvent::ToolCall {
            agent_id,
            parent_agent_id,
            tool,
            args,
            ..
        } => {
            let summary = transcript::tool_summary(&tool, &args);
            app.phase = if summary.contains("verify") {
                TurnPhase::Verifying
            } else {
                TurnPhase::RunningTool(summary.clone())
            };
            let is_subagent = parent_agent_id.is_some()
                || agent_id.contains('.')
                || (agent_id.starts_with("a0_") && agent_id != "a0");
            let kind = if is_subagent {
                LineKind::SubagentStep { indent: 0 }
            } else {
                LineKind::Tool
            };
            app.push_transcript(TranscriptLine::new(kind, summary.clone()));
            if tool == "read_file" || tool == "edit_file" {
                app.set_artifact(format!("{tool}\n\n{args}"));
            }
        }
        ApplicationEvent::ToolResult {
            tool, ok, summary, ..
        } => {
            transcript::apply_tool_result(&mut app.transcript, &tool, ok, &summary);
            if !ok {
                let detail = if summary.trim().is_empty() {
                    format!("{tool} failed")
                } else {
                    format!("{tool} failed\n\n{summary}\n")
                };
                app.set_artifact(detail);
            }
        }
        ApplicationEvent::ApprovalRequest {
            session_id,
            approval_id,
            kind,
            detail,
            missing_controls,
            user_approval_required,
            ..
        } => {
            app.phase = TurnPhase::WaitingApproval;
            let pending = PendingApproval {
                session_id,
                approval_id,
                kind,
                detail,
                missing_controls,
                user_approval_required,
            };
            if app.pending_approval.is_none() {
                app.approval_scroll_offset = 0;
                app.approval_selection = 0;
                app.pending_approval = Some(pending);
            } else if app.queued_approvals.len() < 32 {
                app.queued_approvals.push_back(pending);
            } else {
                app.portal_error = Some(
                    "Too many pending approvals; session stopped without granting them".into(),
                );
                super::deny_pending(app, coordinator);
            }
        }
        ApplicationEvent::LogDiagnostic { message, .. } => {
            if transcript::diagnostic_is_transcript(&message) {
                if !app.capacity_warned {
                    let copy = failure::explain_capacity_warning(&message);
                    app.capacity_warned = true;
                    app.push_transcript(TranscriptLine::new(LineKind::Warn, copy.headline));
                    app.push_transcript(TranscriptLine::new(LineKind::Warn, copy.summary));
                    if let Some(hint) = copy.hint {
                        app.push_transcript(TranscriptLine::new(LineKind::Warn, hint));
                    }
                }
            } else {
                app.push_activity(message);
            }
        }
        ApplicationEvent::DispatchPlacement {
            target,
            decision,
            reason_code,
            ..
        } => {
            let why = reason_code
                .as_deref()
                .map(|c| format!(" ({c})"))
                .unwrap_or_default();
            app.push_activity(format!("{decision} → {target}{why}"));
        }
        ApplicationEvent::RunStatus { status, error, .. } => {
            if status == "started" {
                if !app.thinking {
                    app.begin_turn();
                }
                app.push_activity("status: started".into());
            } else if status == "error" || error.is_some() {
                let raw = error
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| "the turn failed without an error explanation".into());
                app.report_turn_failure(&raw);
            }
        }
        ApplicationEvent::TurnCompleted { error, status, .. } => {
            super::deny_pending(app, coordinator);
            if !app.expand_thoughts {
                transcript::set_thought_collapsed(&mut app.transcript, true);
            }
            if app.phase == TurnPhase::Canceled {
                return;
            }
            if let Some(err) = error.filter(|s| !s.trim().is_empty()) {
                app.report_turn_failure(&err);
            } else if status == "error" {
                app.report_turn_failure("the turn failed without an error explanation");
            } else {
                app.finish_turn_ok();
            }
        }
        ApplicationEvent::SessionEnded { .. } => super::deny_pending(app, coordinator),
        ApplicationEvent::Cancellation { .. } => {
            app.push_transcript(TranscriptLine::new(LineKind::Sys, "Turn canceled."));
            app.thinking = false;
            app.turn_settled = true;
            app.turn_failed = false;
            app.turn_start = None;
            app.last_event_at = None;
            app.phase = TurnPhase::Canceled;
            super::deny_pending(app, coordinator);
        }
        ApplicationEvent::InspectorClear => {
            app.inspector_text.clear();
            app.inspector_scroll_offset = 0;
            app.show_activity = false;
        }
        ApplicationEvent::InspectorUpdate { text } => {
            super::retention::append(
                &mut app.inspector_text,
                &text,
                super::retention::INSPECTOR_BYTES,
            );
            app.show_activity = false;
        }
        ApplicationEvent::WorkspaceDiff { diff } => {
            app.set_artifact(diff);
        }
        ApplicationEvent::ContextInformation { info } => {
            app.push_activity(info);
        }
        ApplicationEvent::CapacityProgress { message, .. } => {
            app.push_activity(message);
        }
        _ => {}
    }
}

pub fn status_inspect(app: &App) -> String {
    let h = status::health(
        app.recovery_chip,
        app.degraded,
        app.thinking,
        app.pending_approval.is_some(),
    );
    status::session_status(
        &app.session_id,
        &app.resume_state,
        &app.model,
        &app.workspace_root,
        &app.phase,
        h,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lokai_app::events::ApplicationEvent;

    #[test]
    fn tokens_and_tools_go_to_transcript_logs_go_to_activity() {
        let mut app = App::test_stub();
        let coordinator = crate::app_kernel::TerminalApprovalCoordinator::new();
        apply(
            &mut app,
            ApplicationEvent::ModelToken {
                session_id: "s".into(),
                agent_id: "a".into(),
                token: "Hi".into(),
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );
        apply(
            &mut app,
            ApplicationEvent::LogDiagnostic {
                session_id: None,
                agent_id: None,
                message: "router: single: orchestration disabled".into(),
            },
            coordinator.as_ref(),
        );
        apply(
            &mut app,
            ApplicationEvent::ToolCall {
                session_id: "s".into(),
                agent_id: "a".into(),
                call_id: "s_a_read_file".into(),
                tool: "read_file".into(),
                args: serde_json::json!({"path": "src/lib.rs"}),
                parent_agent_id: None,
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );
        assert!(app
            .transcript
            .iter()
            .any(|l| l.kind == LineKind::Lokai && l.text == "Hi"));
        assert!(app
            .transcript
            .iter()
            .any(|l| l.kind == LineKind::Tool && l.text.contains("read_file")));
        assert!(app.activity.iter().any(|a| a.contains("router:")));
        assert!(!app.transcript.iter().any(|l| l.text.contains("router:")));
    }

    #[test]
    fn turn_answer_delivers_to_chat_transcript() {
        let mut app = App::test_stub();
        let coordinator = crate::app_kernel::TerminalApprovalCoordinator::new();
        app.begin_turn();
        assert!(app.thinking);
        apply(
            &mut app,
            ApplicationEvent::TurnAnswer {
                session_id: "s".into(),
                agent_id: "a0".into(),
                text: "All 5 unit tests pass.".into(),
                from_finish: true,
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );
        assert!(app.thinking);
        assert!(!app.turn_settled);
        assert!(app
            .transcript
            .iter()
            .any(|l| l.kind == LineKind::Lokai && l.text == "All 5 unit tests pass."));
    }

    #[test]
    fn stage_transition_updates_turn_phase() {
        let mut app = App::test_stub();
        let coordinator = crate::app_kernel::TerminalApprovalCoordinator::new();
        app.begin_turn();
        apply(
            &mut app,
            ApplicationEvent::StageTransition {
                session_id: "s".into(),
                agent_id: "a0".into(),
                stage: lokai_app::events::EngineStage::Routing { mode: "llm".into() },
                detail: Some("Classifying intent...".into()),
            },
            coordinator.as_ref(),
        );
        assert!(matches!(app.phase, TurnPhase::Stage(_, _)));
        assert_eq!(
            status::status_text(&app.phase, false, None, None, None),
            " Classifying intent..."
        );
    }

    #[test]
    fn capacity_warn_does_not_pin_degraded() {
        let mut app = App::test_stub();
        let coordinator = crate::app_kernel::TerminalApprovalCoordinator::new();
        apply(
            &mut app,
            ApplicationEvent::LogDiagnostic {
                session_id: None,
                agent_id: None,
                message: "capacity: saved profile for `qwen` is degraded. Chat continues; inference will abort if this model spills VRAM.".into(),
            },
            coordinator.as_ref(),
        );
        apply(
            &mut app,
            ApplicationEvent::LogDiagnostic {
                session_id: None,
                agent_id: None,
                message: "capacity: saved profile for `qwen` is degraded. Chat continues; inference will abort if this model spills VRAM.".into(),
            },
            coordinator.as_ref(),
        );
        assert!(!app.degraded);
        assert_eq!(
            app.transcript
                .iter()
                .filter(|l| l.kind == LineKind::Warn)
                .count(),
            3
        );
        app.finish_turn_ok();
        assert!(!app.degraded);
    }

    #[test]
    fn failed_tool_puts_reason_on_the_call_line() {
        let mut app = App::test_stub();
        let coordinator = crate::app_kernel::TerminalApprovalCoordinator::new();
        apply(
            &mut app,
            ApplicationEvent::ToolCall {
                session_id: "s".into(),
                agent_id: "a".into(),
                call_id: "s_a_read_file".into(),
                tool: "read_file".into(),
                args: serde_json::json!({"path": "src/lib.rs"}),
                parent_agent_id: None,
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );
        apply(
            &mut app,
            ApplicationEvent::ToolResult {
                session_id: "s".into(),
                agent_id: "a".into(),
                call_id: "s_a_read_file".into(),
                tool: "read_file".into(),
                ok: false,
                summary: "duplicate read: src/lib.rs".into(),
                error_kind: None,
                parent_agent_id: None,
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );
        let line = app
            .transcript
            .iter()
            .find(|l| l.kind == LineKind::Tool)
            .unwrap();
        assert!(line.text.contains("failed — duplicate read"));
        assert!(app.inspector_text.contains("duplicate read"));
    }

    #[test]
    fn thought_stream_accumulates_and_collapses_on_turn_completed() {
        let mut app = App::test_stub();
        let coordinator = crate::app_kernel::TerminalApprovalCoordinator::new();
        apply(
            &mut app,
            ApplicationEvent::ThoughtToken {
                session_id: "s".into(),
                agent_id: "a".into(),
                token: "Let me think ".into(),
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );
        apply(
            &mut app,
            ApplicationEvent::ThoughtToken {
                session_id: "s".into(),
                agent_id: "a".into(),
                token: "about this.".into(),
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );
        assert_eq!(app.transcript.len(), 1);
        assert_eq!(
            app.transcript[0].kind,
            LineKind::Thought { collapsed: true }
        );
        assert_eq!(app.transcript[0].text, "Let me think about this.");

        apply(
            &mut app,
            ApplicationEvent::TurnCompleted {
                session_id: "s".into(),
                status: "ok".into(),
                error: None,
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );
        assert_eq!(
            app.transcript[0].kind,
            LineKind::Thought { collapsed: true }
        );
    }

    #[test]
    fn subagent_node_lifecycle_and_steps_render_tree_hierarchy() {
        let mut app = App::test_stub();
        let coordinator = crate::app_kernel::TerminalApprovalCoordinator::new();

        // 1. Specialist Node started
        apply(
            &mut app,
            ApplicationEvent::NodeStarted {
                meta: lokai_app::events::ExecutionNodeMeta {
                    node_id: "a0.1".into(),
                    parent_node_id: Some("a0".into()),
                    run_id: "r1".into(),
                    session_id: "s1".into(),
                    kind: lokai_app::events::NodeKind::Specialist {
                        role: "Coder".into(),
                    },
                    label: "Specialist: Coder".into(),
                },
            },
            coordinator.as_ref(),
        );

        // 2. Subagent Tool Call and Result
        apply(
            &mut app,
            ApplicationEvent::ToolCall {
                session_id: "s1".into(),
                agent_id: "a0.1".into(),
                call_id: "c1".into(),
                tool: "read_file".into(),
                args: serde_json::json!({"path": "src/main.rs"}),
                parent_agent_id: Some("a0".into()),
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );
        apply(
            &mut app,
            ApplicationEvent::ToolResult {
                session_id: "s1".into(),
                agent_id: "a0.1".into(),
                call_id: "c1".into(),
                tool: "read_file".into(),
                ok: true,
                summary: "40 lines".into(),
                error_kind: None,
                parent_agent_id: Some("a0".into()),
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );

        // 3. Specialist Node Completed
        apply(
            &mut app,
            ApplicationEvent::NodeCompleted {
                node_id: "a0.1".into(),
                state: lokai_app::events::NodeState::Succeeded {
                    summary: "Code edits applied".into(),
                },
                duration_ms: 1200,
            },
            coordinator.as_ref(),
        );

        // 4. Critic Node started & finished
        apply(
            &mut app,
            ApplicationEvent::NodeStarted {
                meta: lokai_app::events::ExecutionNodeMeta {
                    node_id: "a0.2".into(),
                    parent_node_id: Some("a0".into()),
                    run_id: "r1".into(),
                    session_id: "s1".into(),
                    kind: lokai_app::events::NodeKind::Evaluator {
                        criterion: "LSP".into(),
                    },
                    label: "Critic Review".into(),
                },
            },
            coordinator.as_ref(),
        );
        apply(
            &mut app,
            ApplicationEvent::NodeCompleted {
                node_id: "a0.2".into(),
                state: lokai_app::events::NodeState::Succeeded {
                    summary: "Verdict: APPROVED".into(),
                },
                duration_ms: 400,
            },
            coordinator.as_ref(),
        );

        // 5. Final conversation answer
        apply(
            &mut app,
            ApplicationEvent::TurnAnswer {
                session_id: "s1".into(),
                agent_id: "a0".into(),
                text: "All changes are in place.".into(),
                from_finish: true,
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );

        assert_eq!(app.transcript.len(), 6);
        assert!(matches!(
            app.transcript[0].kind,
            LineKind::SubagentHeader { .. }
        ));
        assert!(matches!(
            app.transcript[1].kind,
            LineKind::SubagentStep { .. }
        ));
        assert!(matches!(
            app.transcript[2].kind,
            LineKind::SubagentFooter { ok: true, .. }
        ));
        assert!(matches!(
            app.transcript[3].kind,
            LineKind::SubagentHeader { .. }
        ));
        assert!(matches!(
            app.transcript[4].kind,
            LineKind::SubagentFooter { ok: true, .. }
        ));
        assert_eq!(app.transcript[5].kind, LineKind::Lokai);
    }

    #[test]
    fn late_token_after_turn_completed_does_not_revive_thinking_or_append_transcript() {
        let mut app = App::test_stub();
        let coordinator = crate::app_kernel::TerminalApprovalCoordinator::new();
        app.begin_turn();
        assert!(app.thinking);
        assert!(!app.turn_settled);

        // Turn completes normally
        apply(
            &mut app,
            ApplicationEvent::TurnCompleted {
                session_id: "s".into(),
                status: "ok".into(),
                error: None,
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );
        assert!(!app.thinking);
        assert!(app.turn_settled);
        assert_eq!(app.phase, TurnPhase::Idle);

        // Stale model token arrives late
        apply(
            &mut app,
            ApplicationEvent::ModelToken {
                session_id: "s".into(),
                agent_id: "a".into(),
                token: "late model token".into(),
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );
        // Stale thought token arrives late
        apply(
            &mut app,
            ApplicationEvent::ThoughtToken {
                session_id: "s".into(),
                agent_id: "a".into(),
                token: "late thought token".into(),
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );

        // Thinking must NOT be revived, phase must remain Idle, no lines appended
        assert!(!app.thinking);
        assert_eq!(app.phase, TurnPhase::Idle);
        assert!(!app.transcript.iter().any(|l| l.text.contains("late")));
    }

    #[test]
    fn agent_answer_keeps_turn_open_for_following_agent_output() {
        let mut app = App::test_stub();
        let coordinator = crate::app_kernel::TerminalApprovalCoordinator::new();
        app.begin_turn();

        apply(
            &mut app,
            ApplicationEvent::TurnAnswer {
                session_id: "s".into(),
                agent_id: "a".into(),
                text: "Here is the result.".into(),
                from_finish: true,
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );
        assert!(app.thinking);
        assert!(!app.turn_settled);

        // A critic or revision can continue after an agent answer.
        apply(
            &mut app,
            ApplicationEvent::ModelToken {
                session_id: "s".into(),
                agent_id: "a".into(),
                token: "extra trailing token".into(),
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );

        assert!(app.thinking);
        assert_eq!(app.phase, TurnPhase::Generating);
        assert!(app
            .transcript
            .iter()
            .any(|l| l.text.contains("extra trailing token")));
    }

    #[test]
    fn completion_and_answer_after_cancellation_are_ignored() {
        let mut app = App::test_stub();
        let coordinator = crate::app_kernel::TerminalApprovalCoordinator::new();
        app.begin_turn();
        assert!(app.thinking);

        // User cancels turn
        apply(
            &mut app,
            ApplicationEvent::Cancellation {
                session_id: "s".into(),
                run_id: None,
                attempt_id: None,
            },
            coordinator.as_ref(),
        );
        assert!(!app.thinking);
        assert!(app.turn_settled);
        assert_eq!(app.phase, TurnPhase::Canceled);

        // Late TurnCompleted arrives from canceled worker
        apply(
            &mut app,
            ApplicationEvent::TurnCompleted {
                session_id: "s".into(),
                status: "ok".into(),
                error: None,
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );
        assert_eq!(app.phase, TurnPhase::Canceled);
        assert!(!app.thinking);

        // Late TurnAnswer arrives from canceled worker
        apply(
            &mut app,
            ApplicationEvent::TurnAnswer {
                session_id: "s".into(),
                agent_id: "a".into(),
                text: "Late answer text".into(),
                from_finish: true,
                run_id: None,
                task_id: None,
                attempt_id: None,
                identity_id: None,
            },
            coordinator.as_ref(),
        );
        assert_eq!(app.phase, TurnPhase::Canceled);
        assert!(!app
            .transcript
            .iter()
            .any(|l| l.text.contains("Late answer text")));
    }

    #[test]
    fn turn_failure_settles_turn_and_clears_thinking() {
        let mut app = App::test_stub();
        app.begin_turn();
        assert!(app.thinking);
        assert!(!app.turn_settled);

        app.report_turn_failure("network disconnected");
        assert!(!app.thinking);
        assert!(app.turn_settled);
        assert!(app.turn_failed);
        assert!(matches!(app.phase, TurnPhase::Failed(_)));

        // Starting a new turn clears settled flag and failure state
        app.begin_turn();
        assert!(app.thinking);
        assert!(!app.turn_settled);
        assert!(!app.turn_failed);
        assert_eq!(app.phase, TurnPhase::Generating);
    }
}
