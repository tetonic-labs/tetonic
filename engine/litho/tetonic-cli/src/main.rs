//! `tetonic` — local workspace host and operator commands.

mod capacity;
mod control;
mod estate;
mod job;
mod job_view;
mod local_ui;

use clap::Parser;

const USAGE: &str = "\
Usage: tetonic <COMMAND> [ARGS]

Commands:
  ui        Serve the local team workspace to the web UI
  job       Launch a registered job with operator-supplied host settings
  control   Offline operator control of agents, contexts and work
  estate    Manage enrolled workers and capacity

Run `tetonic <COMMAND> --help` for command options.";

fn main() -> anyhow::Result<()> {
    const STACK_SIZE: usize = 8 * 1024 * 1024;
    std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_stack_size(STACK_SIZE)
                .build()?;
            rt.block_on(Box::pin(run_cli()))
        })?
        .join()
        .unwrap_or_else(|e| std::panic::resume_unwind(e))
}

async fn run_cli() -> anyhow::Result<()> {
    let mut argv: Vec<String> = std::env::args().collect();
    let command = argv.get(1).cloned();
    match command.as_deref() {
        Some("ui") => {
            argv.remove(1);
            local_ui::dispatch(local_ui::UiCli::parse_from(argv)).await
        }
        Some("job") => {
            argv.remove(1);
            job::dispatch(job::JobCli::parse_from(argv)).await
        }
        Some("control") => {
            argv.remove(1);
            control::dispatch(control::ControlCli::parse_from(argv)).await
        }
        Some("estate") => {
            argv.remove(1);
            estate::dispatch(estate::EstateCli::parse_from(argv)).await
        }
        None | Some("help" | "--help" | "-h") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => {
            eprintln!("Unknown command `{other}`.\n\n{USAGE}");
            std::process::exit(2);
        }
    }
}
