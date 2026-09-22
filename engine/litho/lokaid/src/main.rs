//! `lokaid` — the local-only agent daemon the editor spawns.
//!
//! It owns the request/dispatch loop and wires the engine (default-deny egress
//! guard, local Ollama provider, workspace-scoped tools, the single-agent loop,
//! and the audit store) behind the stdio JSON-RPC contract in `lokai-rpc`.
//!
//! Concurrency: a multi-threaded tokio runtime (4 workers) + `LocalSet` for `chat/send` and
//! `agent/spawn` (live `Conversation` in `lokai-app` is `!Send`).
//! Capacity optimize and the stdout writer use `tokio::spawn` because their
//! futures are `Send`, achieving full Concurrency Domain Isolation. Combined-mode
//! fabric serving stays on `spawn_local` because `WorkerStore` is `!Send`.
//!
//! Forward-compat (Phase C harness/sub-agent model): every notification is
//! stamped with an `agent_id` (always `"a0"`, the root, today) and the engine is
//! grouped into an `EngineServices` bundle so a future `spawn_agent` can clone
//! the hot, already-resident handles cheaply. See agent-rpc-v1.md.

mod daemon;
mod node;
mod supervise;

use anyhow::Result;
use std::sync::Arc;
use tokio::io::BufReader;

use lokai_rpc::framing;
use lokai_rpc::protocol::{ErrorCode, Incoming, Response, RpcError};
use lokai_rpc::{writer_task, Notifier, OutboundQueue, DEFAULT_OUTBOUND_CAPACITY};

use daemon::{Daemon, Dispatch};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let node_mode = args.iter().any(|a| a == "--node");
    let combined_mode = args.iter().any(|a| a == "--combined");

    if std::env::args().skip(1).any(|a| a == "--print-schema") {
        let bundle = lokai_rpc::schema_bundle();
        println!("{}", serde_json::to_string_pretty(&bundle)?);
        return Ok(());
    }

    // Restart-and-rehydrate parent (H3-3). Abort still kills the child; this
    // loop does not continue in-flight Infer/shell.
    if supervise::wants_supervise(&args) {
        return supervise::run();
    }

    // Two execution modes (`node-enrollment-v1`): the default *coordinator* — this
    // binary behind the editor, speaking JSON-RPC over stdio, owning the
    // workspace/tools/audit — and a future *worker* (`--node`) that exposes only
    // the local inference runtime to enrolled coordinators. Worker mode is a
    // Phase F capability; the flag is reserved now so adding it later is an
    // additive branch here, not a startup rewrite.
    if node_mode && !combined_mode {
        if args.iter().any(|a| a == "--enroll") {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .enable_all()
                .build()?;
            return rt.block_on(node::run_enroll());
        }
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()?;
        return rt.block_on(node::run_serve());
    }

    let mode = if std::env::var("LOKAI_DIAGNOSTIC_RAW_PAYLOADS").is_ok() {
        lokai_app::lokai_telemetry::DiagnosticMode::UnsafeRawPayloads
    } else {
        lokai_app::lokai_telemetry::DiagnosticMode::Safe
    };

    let subscriber = lokai_app::lokai_telemetry::init_subscriber(mode);
    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set telemetry subscriber");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()?;
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, serve())
}

async fn serve() -> Result<()> {
    let root_span = tracing::info_span!("lokaid_root");
    let _root_enter = root_span.enter();
    lokai_app::lokai_telemetry::inject_context(lokai_app::lokai_telemetry::TraceContext::default());

    let combined = std::env::args().skip(1).any(|a| a == "--combined");
    let audit_store = lokai_app::open_default_audit_store();
    lokai_app::install_shared_scanner_from_store(&audit_store);
    let scanner = lokai_app::scanner_from_shared_store(&audit_store);
    let (mut queue, wake_rx) = OutboundQueue::new(DEFAULT_OUTBOUND_CAPACITY);
    let scanner_hook = scanner.clone();
    queue = queue.with_redactor(Arc::new(move |text| {
        match scanner_hook.scan_and_redact_sync(text, None) {
            Ok(Some((_, redacted))) => Ok(Some(redacted)),
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }));
    let notifier = Notifier::new(queue.clone());

    // One task owns stdout; everything emits frames through the bounded queue.
    let mut writer = tokio::spawn(writer_task(queue, wake_rx, tokio::io::stdout()));

    let fabric_shutdown = if combined {
        Some(lokai_app::Application::spawn_combined_fabric()?)
    } else {
        None
    };

    let mut daemon = Daemon::new(notifier.clone(), fabric_shutdown, Some(scanner.clone()));

    let mut reader = BufReader::new(tokio::io::stdin());

    loop {
        let incoming = tokio::select! {
            frame = framing::read_frame(&mut reader) => frame,
            _ = &mut writer => break,
        };
        let frame = match incoming {
            Ok(Some(bytes)) => bytes,
            Ok(None) => break, // editor closed the pipe
            Err(e) => {
                tracing::warn!("framing error: {e}");
                break;
            }
        };

        let msg: Incoming = match serde_json::from_slice(&frame) {
            Ok(m) => m,
            Err(e) => {
                // We can't recover the request id from unparseable input.
                notifier.respond(&Response::err(
                    serde_json::Value::Null,
                    RpcError::new(ErrorCode::ParseError, e.to_string()),
                ));
                continue;
            }
        };

        let id = msg.id.clone().unwrap_or(serde_json::Value::Null);

        if daemon.handle(msg, id).await == Dispatch::Stop {
            break;
        }
    }

    daemon.shutdown().await;
    writer.abort();
    Ok(())
}
