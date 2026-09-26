//! `tetonic` — the Phase A headless coding agent.

mod app_kernel;
mod args;
mod banner;
mod capacity;
mod chat;
mod control;
mod estate;
mod event_queue;
mod help;
mod job;
mod job_view;
mod offline;
mod printer;
mod session;
mod signal;
mod terminal_task;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use anyhow::{bail, Context};
use clap::Parser;
use tetonic_app::commands::EndSessionCommand;
use tetonic_app::tetonic_telemetry;
use tetonic_app::{Application, CliBootstrapParams};

use app_kernel::{TerminalApprovalCoordinator, TerminalEventSink, TerminalRenderer};
use args::Args;
use offline::{code_index, project_memory, show_history, time_travel};
use session::{CliSessionConfig, CliTurnContext};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("help") && std::env::args().len() == 2 {
        return help::print_cli_help();
    }

    if std::env::args().nth(1).as_deref() == Some("estate") {
        let mut argv: Vec<String> = std::env::args().collect();
        argv.remove(1);
        return estate::dispatch(estate::EstateCli::parse_from(argv)).await;
    }
    if std::env::args().nth(1).as_deref() == Some("control") {
        let mut argv: Vec<String> = std::env::args().collect();
        argv.remove(1);
        return control::dispatch(control::ControlCli::parse_from(argv)).await;
    }
    if std::env::args().nth(1).as_deref() == Some("job") {
        let mut argv: Vec<String> = std::env::args().collect();
        argv.remove(1);
        return job::dispatch(job::JobCli::parse_from(argv)).await;
    }
    let args = Args::parse();
    let task = args.prompt.join(" ");
    let interactive = args.chat || task.trim().is_empty();

    if args.debug && std::env::var("LOKAI_LOG").is_err() {
        std::env::set_var("LOKAI_LOG", "debug");
    }

    let mode = if args.debug || std::env::var("LOKAI_DIAGNOSTIC_RAW_PAYLOADS").is_ok() {
        tetonic_telemetry::DiagnosticMode::UnsafeRawPayloads
    } else {
        tetonic_telemetry::DiagnosticMode::Safe
    };

    let _log_guard = if interactive {
        if let Some(dirs) = directories::ProjectDirs::from("", "", "lokai") {
            let log_dir = dirs.data_dir().join("logs");
            std::fs::create_dir_all(&log_dir).ok();
            let (subscriber, guard) =
                tetonic_telemetry::init_subscriber_cli(mode, log_dir, interactive);
            tracing::subscriber::set_global_default(subscriber)
                .expect("Failed to set telemetry subscriber");
            Some(guard)
        } else {
            let subscriber = tetonic_telemetry::init_subscriber_stderr(mode);
            tracing::subscriber::set_global_default(subscriber)
                .expect("Failed to set telemetry subscriber");
            None
        }
    } else {
        let subscriber = tetonic_telemetry::init_subscriber_stderr(mode);
        tracing::subscriber::set_global_default(subscriber)
            .expect("Failed to set telemetry subscriber");
        None
    };

    let root_span = tracing::info_span!("lokai_cli_root");
    let _root_enter = root_span.enter();
    let root_ctx = tetonic_telemetry::TraceContext::default();
    tetonic_telemetry::inject_context(root_ctx);

    if args.allow_shell && !args.i_understand_unapproved_shell {
        bail!(
            "--allow-shell bypasses interactive approval; pass --i-understand-unapproved-shell to confirm"
        );
    }

    if let Some(d) = &args.draft_model {
        std::env::set_var("LOKAI_DRAFT_MODEL", d);
    }

    if args.checkpoint.is_some()
        || args.checkpoints
        || args.restore.is_some()
        || args.undo
        || args.redo
    {
        return time_travel(&args).await;
    }

    if args.index
        || args.embed
        || args.index_status
        || args.gc_index
        || args.def.is_some()
        || args.refs.is_some()
        || args.outline.is_some()
        || args.search.is_some()
        || args.watch_index
    {
        return code_index(&args).await;
    }

    if args.project_status || args.project_note.is_some() {
        return project_memory(&args).await;
    }

    if args.sessions || args.session.is_some() {
        return show_history(args.session.as_deref()).await;
    }

    let (tui_tx, tui_rx) = event_queue::channel();
    let renderer = if interactive {
        Arc::new(TerminalRenderer::with_tui(tui_tx.clone()))
    } else {
        Arc::new(TerminalRenderer::new())
    };
    let approval_coordinator = TerminalApprovalCoordinator::new();
    let event_sink = TerminalEventSink::new(renderer.clone(), approval_coordinator.clone());

    let out = Application::bootstrap_cli(CliBootstrapParams {
        workspace: args.workspace.clone(),
        ollama: args.ollama.clone(),
        model: args.model.clone(),
        model_hard: args.model_hard.clone(),
        model_tier: args.model_tier.clone(),
        allow_shell: args.allow_shell,
        explain: args.explain,
        no_verify: args.no_verify,
        verify: args.verify.clone(),
        orchestrate: args.orchestrate.clone(),
        no_critic: args.no_critic,
        llm_router: args.llm_router,
        max_steps: args.max_steps,
        num_ctx: u32::try_from(args.num_ctx).context("context size exceeds runtime limit")?,
        event_sink,
        anthropic_key: args.anthropic_key.clone(),
        openai_key: args.openai_key.clone(),
        endpoint: args.endpoint.clone(),
    })
    .await
    .context("application bootstrap failed")?;

    let app = out.app;
    approval_coordinator.bind_approvals(app.approvals.clone());

    banner::print_startup_banner();
    println!("  workspace: {}", out.workspace_root);
    println!("  model:     {} (via {})", out.model_fast, out.ollama_base);
    if out.session_result.resumed {
        println!("  session:   resumed ({})", out.session_result.resume_state);
    }
    println!(
        "  shell:     {}",
        if args.allow_shell {
            "pre-approved"
        } else {
            "prompt on use (y/N/r=remember)"
        }
    );
    if args.explain {
        println!("  mode:      explain (read-only; no edits or verify gate)");
    }
    if let Some(ref v) = out.verify_resolved {
        println!("  verify:    {v} (gates finish)");
    }

    if let Some(ref path) = args.tokenizer {
        if let Err(e) = app.bind_exact_tokenizer(path) {
            eprintln!("warning: tokenizer load failed ({e}); using heuristic");
            app.bind_heuristic_tokenizer();
        } else {
            println!("  tokenizer: {path} (exact)");
        }
    } else {
        app.bind_heuristic_tokenizer();
    }

    let _cancel_handle = signal::CancelHandle::spawn(app.clone(), out.session_id.clone());

    let session_config = CliSessionConfig {
        workspace_root: out.workspace_root.clone(),
        model_fast: out.model_fast.clone(),
        model_hard: out.model_hard.clone(),
        ollama_base: out.ollama_base.clone(),
    };

    let turn_ctx = CliTurnContext {
        app: app.clone(),
        config: session_config,
        session_id: out.session_id.clone(),
        llm_router: out.session_result.llm_router,
        approval_coordinator,
    };

    let local = tokio::task::LocalSet::new();
    let run_result: anyhow::Result<()> = local
        .run_until(async {
            let result = if interactive {
                println!("  mode: interactive TUI chat (Ctrl+Q to quit, Ctrl+C cancels a turn)\n");
                let history = out
                    .session_result
                    .messages
                    .iter()
                    .map(|m| (m.role.clone(), m.content.clone()))
                    .collect();
                chat::run_tui_chat(
                    turn_ctx,
                    tui_tx.clone(),
                    tui_rx,
                    out.session_result.resumed,
                    out.session_result.resume_state.clone(),
                    history,
                    args.debug,
                )
                .await
            } else {
                println!("  task: {task}\n");
                chat::run_one_shot(&turn_ctx, &task, renderer).await
            };
            app.close_session(EndSessionCommand {
                session_id: out.session_id.clone(),
                workspace_root: out.workspace_root.clone(),
                status: Some(if result.is_ok() { "ok" } else { "error" }.into()),
                error: result.as_ref().err().map(ToString::to_string),
            })
            .await
            .with_context(|| "session close failed")?;
            result
        })
        .await;

    let log = app.egress_activity_log();
    println!("\n--- network activity ({} request(s)) ---", log.len());
    for ev in &log {
        println!(
            "  [{:?}] {} -> {}:{} ({})",
            ev.decision, ev.initiator, ev.host, ev.port, ev.reason
        );
    }
    let _ = app.record_egress_and_consolidate(&out.session_id);

    run_result
}

mod tui;
