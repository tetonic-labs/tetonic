//! Interactive multi-turn chat loop — submits turns through product `submit_chat_turn`.

use std::sync::Arc;

use tetonic_app::commands::RunTurnCommand;

use crate::app_kernel::TerminalRenderer;
use crate::session::CliTurnContext;
use crate::tui::{TuiAction, TuiLaunch};

pub async fn run_tui_chat(
    ctx: CliTurnContext,
    tx_events: crate::event_queue::Sender,
    rx_events: crate::event_queue::Receiver,
    resumed: bool,
    resume_state: String,
    history: Vec<(String, String)>,
    debug: bool,
) -> anyhow::Result<()> {
    let (tx_input, mut rx_input) = tokio::sync::mpsc::channel::<TuiAction>(32);
    let (tx_errors, rx_errors) = tokio::sync::mpsc::channel::<String>(8);
    let ctx_bg = ctx.clone();

    let worker = tokio::task::spawn_local(async move {
        while let Some(action) = rx_input.recv().await {
            let line = match action {
                TuiAction::Quit => break,
                TuiAction::Submit(line) => line,
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('/') {
                handle_slash_command(line, &ctx_bg, &tx_events).await;
                continue;
            }
            let submit_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                ctx_bg.app.submit_chat_turn(RunTurnCommand {
                    session_id: ctx_bg.session_id.clone(),
                    user_input: line.to_string(),
                    verify_cmd: None,
                    llm_router: Some(ctx_bg.llm_router),
                })
            }));
            match submit_res {
                Ok(Err(e)) => {
                    if tx_errors
                        .send(crate::terminal_task::bounded_error(e.employee_message()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(panic_err) => {
                    let msg = if let Some(s) = panic_err.downcast_ref::<&str>() {
                        format!("turn submission panicked: {s}")
                    } else if let Some(s) = panic_err.downcast_ref::<String>() {
                        format!("turn submission panicked: {s}")
                    } else {
                        "turn submission panicked".to_string()
                    };
                    if tx_errors
                        .send(crate::terminal_task::bounded_error(msg))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(Ok(())) => {}
            }
        }
    });

    crate::terminal_task::supervise(
        worker,
        crate::tui::run_tui(
            rx_events,
            rx_errors,
            tx_input,
            TuiLaunch {
                model: ctx.config.model_fast.clone(),
                workspace_root: ctx.config.workspace_root.clone(),
                session_id: ctx.session_id.clone(),
                resumed,
                resume_state,
                history,
                coordinator: ctx.approval_coordinator.clone(),
                kernel: ctx.app.clone(),
                debug,
            },
        ),
    )
    .await
}

pub async fn run_one_shot(
    ctx: &CliTurnContext,
    task: &str,
    renderer: Arc<TerminalRenderer>,
) -> anyhow::Result<()> {
    let finish = ctx.app.arm_turn_join(&ctx.session_id);
    ctx.app
        .submit_chat_turn(RunTurnCommand {
            session_id: ctx.session_id.clone(),
            user_input: task.to_string(),
            verify_cmd: None,
            llm_router: Some(ctx.llm_router),
        })
        .map_err(|e| anyhow::anyhow!(e.employee_message()))?;
    let finish = finish.await.unwrap_or(tetonic_app::TurnFinish {
        ok: false,
        canceled: false,
        error: Some("join dropped".into()),
    });
    renderer.end_line();
    if let Some(err) = finish.error {
        anyhow::bail!("{err}");
    }
    if !finish.ok {
        anyhow::bail!("turn did not complete");
    }
    Ok(())
}

fn inspector_error(error: &tetonic_app::errors::AppError) -> String {
    error.employee_message()
}

fn inspector_text(tx: &crate::event_queue::Sender, text: impl Into<String>) {
    let _ = tx.send(tetonic_app::events::ApplicationEvent::InspectorUpdate { text: text.into() });
}

async fn inspector_doctor(ctx: &CliTurnContext, tx: &crate::event_queue::Sender) {
    let selected = match ctx.app.session_inference(&ctx.session_id) {
        Ok(selected) => selected,
        Err(e) => {
            inspector_text(tx, inspector_error(&e));
            return;
        }
    };
    match ctx
        .app
        .get_capacity_doctor_report(None, Some(&selected.model_fast))
        .await
    {
        Ok((text, _)) => inspector_text(tx, text),
        Err(e) => inspector_text(tx, format!("capacity doctor failed: {}\n", inspector_error(&e))),
    }
}

async fn inspector_capacity_status(ctx: &CliTurnContext, tx: &crate::event_queue::Sender) {
    let selected = match ctx.app.session_inference(&ctx.session_id) {
        Ok(selected) => selected,
        Err(e) => {
            inspector_text(tx, inspector_error(&e));
            return;
        }
    };
    match ctx
        .app
        .get_capacity_status_report(None, Some(&selected.model_fast))
        .await
    {
        Ok(text) => inspector_text(tx, text),
        Err(e) => inspector_text(tx, format!("capacity status failed: {}\n", inspector_error(&e))),
    }
}

async fn inspector_ps(ctx: &CliTurnContext, tx: &crate::event_queue::Sender) {
    inspector_text(tx, "> Fetching Ollama loaded models...\n\n");
    match ctx.app.ollama_ps().await {
        Ok(v) => inspector_text(tx, v),
        Err(e) => inspector_text(tx, format!("Failed to query Ollama: {}\n", inspector_error(&e))),
    }
}

async fn inspector_evict(ctx: &CliTurnContext, tx: &crate::event_queue::Sender) {
    inspector_text(tx, "> Evicting Ollama models from VRAM...\n\n");
    match ctx.app.ollama_evict().await {
        Ok(report) => inspector_text(tx, report),
        Err(e) => inspector_text(tx, format!("Failed to evict: {}\n", inspector_error(&e))),
    }
}

fn inspector_egress(ctx: &CliTurnContext, tx: &crate::event_queue::Sender, rest: &[&str]) {
    match rest {
        ["allow", label, ip, rest @ ..] => {
            let port = rest
                .windows(2)
                .find(|w| w[0] == "--port")
                .and_then(|w| w[1].parse().ok());
            match ctx.app.upsert_egress_allow_rule(label, ip, port) {
                Ok(()) => {
                    inspector_text(tx, format!("Egress allow `{label}` persisted.\n"));
                    if let Err(e) = ctx.app.reload_enrollment_egress() {
                        inspector_text(
                            tx,
                            format!("[warning] egress reload failed: {}\n", inspector_error(&e)),
                        );
                    } else {
                        inspector_text(tx, "[info] Egress rules reloaded for the active run.\n");
                    }
                }
                Err(e) => inspector_text(tx, format!("{}\n", inspector_error(&e))),
            }
        }
        ["remove", label] => match ctx.app.remove_egress_allow_rule(label) {
            Ok(true) => {
                inspector_text(tx, format!("Removed egress allow `{label}`.\n"));
                let _ = ctx.app.reload_enrollment_egress();
            }
            Ok(false) => inspector_text(tx, format!("egress allow rule not found: {label}\n")),
            Err(e) => inspector_text(tx, format!("{}\n", inspector_error(&e))),
        },
        _ => inspector_text(
            tx,
            "Usage: /egress allow <label> <ip> [--port N]  or  /egress remove <label>\n",
        ),
    }
}

/// TUI inspector: app services + egress HTTP. Nested `tetonic` subprocesses are
/// not used (R3-2). `/optimize` and arbitrary `/tetonic` stay terminal-only.
async fn handle_slash_command(
    line: &str,
    ctx: &CliTurnContext,
    tx_events: &crate::event_queue::Sender,
) {
    let _ = tx_events.send(tetonic_app::events::ApplicationEvent::InspectorClear);

    let parts: Vec<&str> = line.trim_start_matches('/').split_whitespace().collect();
    if parts.is_empty() {
        return;
    }

    let mut cmd = parts[0];
    let mut is_help = false;

    if cmd == "help" {
        if parts.len() == 1 {
            inspector_text(tx_events, crate::tui::slash::grouped_help());
            return;
        } else if parts.get(1) == Some(&"more") {
            inspector_text(tx_events, crate::tui::slash::leftover_help());
            return;
        } else {
            cmd = parts[1];
            is_help = true;
        }
    }

    let args_start = if is_help { 2 } else { 1 };
    let rest = &parts[args_start.min(parts.len())..];

    match cmd {
        "model" | "inference" => {
            let result = (|| -> Result<String, tetonic_app::errors::AppError> {
                let selected = ctx.app.session_inference(&ctx.session_id)?;
                if is_help {
                    return Ok("/model MODEL [HARD_MODEL]\n/inference PROFILE MODEL [HARD_MODEL]\n/inference shows selection and profiles. Changes apply between turns, for this live session.\n".into());
                }
                if rest.is_empty() {
                    if cmd == "model" {
                        let catalog = ctx.app.model_catalog(&ctx.session_id)?;
                        let mut text = String::from("Choose a model with /model in the TUI or F5.\nAvailable choices:\n");
                        for model in catalog.models {
                            text.push_str(&format!("{}{} / {} - {}\n", if model.current { "* " } else { "  " }, model.name, model.provider_label, model.availability));
                        }
                        return Ok(text);
                    }
                    return Ok(format!("profile     {}\nfast        {}\nhard        {}\nrevision    {}\nprofiles    {}\n", selected.profile, selected.model_fast, selected.model_hard, selected.revision, ctx.app.inference_profiles().join(", ")));
                }
                let (profile, fast, hard) = match (cmd, rest) {
                    ("model", [fast]) => (selected.profile.as_str(), *fast, *fast),
                    ("model", [fast, hard]) => (selected.profile.as_str(), *fast, *hard),
                    ("inference", [profile, fast]) => (*profile, *fast, *fast),
                    ("inference", [profile, fast, hard]) => (*profile, *fast, *hard),
                    _ => return Err(tetonic_app::errors::AppError::InvalidRequest("use /model MODEL [HARD_MODEL] or /inference PROFILE MODEL [HARD_MODEL]".into())),
                };
                let changed = ctx.app.change_session_inference(tetonic_app::inference_selection::ChangeInferenceCommand {
                    session_id: ctx.session_id.clone(), profile: profile.into(), model_fast: fast.into(),
                    model_hard: hard.into(), expected_revision: selected.revision,
                })?;
                Ok(format!("Inference updated for the next turn.\nprofile  {}\nfast     {}\nhard     {}\n", changed.profile, changed.model_fast, changed.model_hard))
            })();
            inspector_text(tx_events, result.unwrap_or_else(|e| inspector_error(&e)));
        }
        "status" => {
            if is_help {
                inspector_text(
                    tx_events,
                    "/status — session id, resume state, model, and workspace.\n\
                     The live phase is shown on the TUI status bar.\n",
                );
                return;
            }
            inspector_text(
                tx_events,
                format!(
                    "session     {}\nresume      (see TUI banner)\nmodel       {}\nworkspace   {}\n",
                    ctx.session_id, ctx.app.session_inference(&ctx.session_id).map(|s| s.model_fast).unwrap_or_else(|_| "unavailable".into()), ctx.config.workspace_root
                ),
            );
        }
        "doctor" => {
            if is_help {
                inspector_text(
                    tx_events,
                    "/doctor — this chat's model vs the saved optimize profile.\n\
                     `--model` is what this session runs. Optimize only changes the default for sessions that omit `--model`.\n",
                );
                return;
            }
            inspector_doctor(ctx, tx_events).await;
        }
        "capacity" => {
            if is_help {
                inspector_text(
                    tx_events,
                    "/capacity — saved profile health plus this session's `--model`.\n\
                     Other subcommands: run `tetonic estate capacity …` in a terminal.\n",
                );
                return;
            }
            match rest.first().copied() {
                None | Some("status") => inspector_capacity_status(ctx, tx_events).await,
                Some("doctor") => inspector_doctor(ctx, tx_events).await,
                Some(other) => inspector_text(
                    tx_events,
                    format!(
                        "`/{cmd} {other}` is a long-running or mutating estate command.\n\
                         Run `tetonic estate capacity {other} …` in a terminal.\n"
                    ),
                ),
            }
        }
        "recovery" => {
            let result = match rest {
                [] => ctx.app.recovery_report().await,
                ["abandon", run_id, revision] => match revision.parse::<u64>() {
                    Ok(revision) => ctx.app.abandon_recovery_run(run_id, revision).await,
                    Err(_) => Err(tetonic_app::errors::AppError::InvalidRequest("revision must be a number from /recovery".into())),
                },
                _ => {
                    inspector_text(tx_events, "/recovery - inspect interrupted runs\n/recovery abandon <run-id> <revision> - cancel further execution; preserve workspace changes\n");
                    return;
                }
            };
            inspector_text(
                tx_events,
                result.unwrap_or_else(|e| format!("Recovery failed: {}\n", inspector_error(&e))),
            );
        }
        "optimize" => inspector_text(
            tx_events,
            "Optimize rebuilds the *default* capacity profile (creates an estate model and benches).\n\
             You do not need it to chat with a smaller model — restart with `--model <tag>`.\n\n\
             To rebuild the default: `tetonic estate capacity optimize --yes` in a terminal.\n",
        ),
        "egress" => {
            if is_help {
                inspector_text(
                    tx_events,
                    "/egress allow <label> <ip> [--port N]\n/egress remove <label>\n",
                );
                return;
            }
            inspector_egress(ctx, tx_events, rest);
        }
        "tetonic" => inspector_text(
            tx_events,
            // Infra leftover: inspector must not spawn the tetonic binary (R3-2).
            "Nested `tetonic` is not spawned from the inspector.\n\
             Run the command in a terminal, or use /doctor /capacity /egress /ps /evict.\n",
        ),
        "kill-ollama" => inspector_text(
            tx_events,
            // Process kill is unsandboxed; R6-2 owns ProcessBroker. Do not spawn taskkill/pkill here.
            "Inspector does not kill processes.\nStop Ollama from a terminal if it is wedged.\n",
        ),
        "ps" => {
            if is_help {
                inspector_text(
                    tx_events,
                    "/ps — GET /api/ps through EgressGuard (same loopback pin as inference).\n",
                );
                return;
            }
            inspector_ps(ctx, tx_events).await;
        }
        "evict" => {
            if is_help {
                inspector_text(
                    tx_events,
                    "/evict — POST /api/generate keep_alive=0 through EgressGuard.\n",
                );
                return;
            }
            inspector_evict(ctx, tx_events).await;
        }
        _ => inspector_text(
            tx_events,
            format!("Unknown slash command: {cmd}\nType /help for available commands.\n"),
        ),
    }
}
