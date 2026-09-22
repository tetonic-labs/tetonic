//! `lokai estate capacity` — runtime profile status, doctor, import, optimize (ES5).

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tetonic_app::{Application, JobState, LOCAL_NODE_ID};

pub async fn dispatch(command: CapacitySub) -> Result<()> {
    match command {
        CapacitySub::Status { worker } => run_status(worker.as_deref()).await,
        CapacitySub::Doctor { worker } => run_doctor(worker.as_deref()).await,
        CapacitySub::Profiles { action } => match action {
            ProfilesSub::List => run_profiles_list().await,
            ProfilesSub::Activate { id } => run_profiles_activate(&id).await,
            ProfilesSub::Rollback => run_profiles_rollback().await,
            ProfilesSub::Export { id, out } => run_profiles_export(&id, out.as_ref()).await,
            ProfilesSub::Import {
                file,
                activate,
                refresh_fingerprint,
            } => run_import(&file, activate, refresh_fingerprint).await,
        },
        CapacitySub::Optimize {
            yes,
            depth,
            standalone,
        } => run_optimize(yes, &depth, standalone).await,
    }
}

/// Alias: `lokai estate setup status|doctor` → capacity.
pub async fn dispatch_setup_alias(action: SetupAliasSub) -> Result<()> {
    match action {
        SetupAliasSub::Status => run_status(None).await,
        SetupAliasSub::Doctor => run_doctor(None).await,
    }
}

async fn run_status(worker: Option<&str>) -> Result<()> {
    let app = Application::bootstrap_offline(None).await?;
    let report = app.get_capacity_status_report(worker, None).await?;
    print!("{report}");
    Ok(())
}

async fn run_doctor(worker: Option<&str>) -> Result<()> {
    let app = Application::bootstrap_offline(None).await?;
    let (report, ok) = app.get_capacity_doctor_report(worker, None).await?;
    print!("{report}");
    if !ok {
        std::process::exit(1);
    }
    Ok(())
}

async fn run_optimize(yes: bool, depth: &str, standalone: bool) -> Result<()> {
    if !yes {
        anyhow::bail!(
            "refusing to optimize without --yes (creates Ollama models and runs benchmarks)"
        );
    }
    if !standalone {
        anyhow::bail!(
            "daemon RPC optimize is not wired in CLI yet — rerun with --standalone for in-process optimize"
        );
    }

    let app = Application::bootstrap_offline(None).await?;
    eprintln!("capacity optimize ({depth})…");

    let outcome = app
        .run_capacity_optimize(depth)
        .await
        .context("capacity optimize")?;

    if outcome.state == JobState::Succeeded {
        if let Some(ref id) = outcome.applied_profile_id {
            println!("Optimize complete — activated profile `{id}`");
        } else if let Some(id) = outcome.profile_ids.first() {
            println!(
                "Optimize complete — profile `{id}` saved (gates failed or auto-apply skipped)"
            );
        } else {
            println!("Optimize complete.");
        }
        Ok(())
    } else {
        anyhow::bail!(
            "optimize failed: {}",
            outcome.error.unwrap_or_else(|| "unknown".into())
        )
    }
}

async fn run_profiles_list() -> Result<()> {
    let app = Application::bootstrap_offline(None).await?;
    let summaries = app
        .capacity
        .list_profiles(tetonic_app::commands::ListProfilesCommand {
            node_id: LOCAL_NODE_ID.to_string(),
            role: "coder".into(),
        })
        .context("list profiles")?;
    if summaries.is_empty() {
        println!("No runtime profiles stored.");
        return Ok(());
    }
    println!("{:<28} {:<8} {:<6} estate_model", "id", "active", "gates");
    for s in summaries {
        let mark = if s.active { "*" } else { " " };
        let gates = if s.gates_passed { "ok" } else { "fail" };
        println!(
            "{:<28} {:<8} {:<6} {} (ctx {})",
            s.id, mark, gates, s.estate_model, s.num_ctx
        );
    }
    Ok(())
}

async fn run_profiles_activate(id: &str) -> Result<()> {
    let app = Application::bootstrap_offline(None).await?;
    let result = app
        .capacity
        .activate_profile(tetonic_app::commands::ActivateProfileCommand {
            node_id: LOCAL_NODE_ID.to_string(),
            role: "coder".into(),
            profile_id: id.to_string(),
        })
        .context("activate profile")?;
    println!(
        "Activated `{}` → {} (ctx {})",
        result.profile_id, result.model_fast, result.num_ctx
    );
    Ok(())
}

async fn run_profiles_rollback() -> Result<()> {
    let app = Application::bootstrap_offline(None).await?;
    let result = app
        .capacity
        .rollback_profile(tetonic_app::commands::RollbackProfileCommand {
            node_id: LOCAL_NODE_ID.to_string(),
            role: "coder".into(),
        })
        .context("rollback profile")?;
    println!(
        "Rolled back to `{}` → {} (ctx {})",
        result.profile_id, result.model_fast, result.num_ctx
    );
    Ok(())
}

async fn run_profiles_export(id: &str, out: Option<&PathBuf>) -> Result<()> {
    let app = Application::bootstrap_offline(None).await?;
    let profile = app
        .capacity
        .export_profile(tetonic_app::commands::ExportProfileCommand {
            profile_id: id.to_string(),
        })
        .context("export profile")?;
    let json = serde_json::to_string_pretty(&profile)?;
    match out {
        Some(path) => {
            std::fs::write(path, &json).with_context(|| format!("write {}", path.display()))?;
            println!("Exported `{id}` → {}", path.display());
        }
        None => println!("{json}"),
    }
    Ok(())
}

async fn run_import(file: &PathBuf, activate: bool, refresh_fingerprint: bool) -> Result<()> {
    let raw = std::fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    let app = Application::bootstrap_offline(None).await?;
    let result = app
        .capacity
        .import_profile(tetonic_app::commands::ImportProfileCommand {
            json: raw,
            activate,
            refresh_fingerprint,
        })
        .context("import profile")?;
    if result.activated {
        println!(
            "Imported and activated `{}` → model `{}` @ ctx {}",
            result.profile_id, result.model, result.num_ctx
        );
    } else {
        println!("Imported profile `{}` (not activated).", result.profile_id);
    }
    Ok(())
}

#[derive(Parser, Debug)]
pub struct CapacityCli {
    #[command(subcommand)]
    pub command: CapacitySub,
}

#[derive(Subcommand, Debug)]
pub enum CapacitySub {
    /// Active profile + health summary.
    Status {
        /// Worker enrollment id or label (remote fabric read).
        #[arg(long)]
        worker: Option<String>,
    },
    /// Diagnose live Ollama placement vs saved profile.
    Doctor {
        /// Worker enrollment id or label (remote fabric read).
        #[arg(long)]
        worker: Option<String>,
    },
    /// Adaptive placement-first optimize (creates estate model + profile).
    Optimize {
        /// Confirm writes Modelfiles and runs Ollama benchmarks.
        #[arg(long)]
        yes: bool,
        /// Search depth: `quick` (default) or `full`.
        #[arg(long, default_value = "quick")]
        depth: String,
        /// Run in-process (default). Omit when editor delegates to daemon RPC.
        #[arg(long, default_value_t = true)]
        standalone: bool,
    },
    /// Profile history management.
    Profiles {
        #[command(subcommand)]
        action: ProfilesSub,
    },
}

#[derive(Subcommand, Debug)]
pub enum ProfilesSub {
    /// List stored profiles (newest first).
    List,
    /// Activate a profile by id.
    Activate { id: String },
    /// Roll back to the previous profile for this node.
    Rollback,
    /// Export profile JSON (stdout or --out file).
    Export {
        id: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Import a profile JSON file (append-only).
    Import {
        file: PathBuf,
        /// Set as active coder profile (default true).
        #[arg(long, default_value_t = true)]
        activate: bool,
        /// Replace hardware fingerprint with live detect (recommended on new machine).
        #[arg(long, default_value_t = false)]
        refresh_fingerprint: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum SetupAliasSub {
    Status,
    Doctor,
}
