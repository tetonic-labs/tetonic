//! `lokai estate` — owned fleet management (N0.1 enrollment).

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use lokai_app::Application;

use crate::capacity::{CapacitySub, SetupAliasSub};

pub fn data_dir() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "lokai")
        .context("could not resolve lokai data directory")?;
    Ok(dirs.data_dir().to_path_buf())
}

pub async fn run_status() -> Result<()> {
    let app = Application::bootstrap_offline(None).await?;
    let result = app
        .estate
        .get_estate_status(lokai_app::commands::GetEstateStatusCommand {
            fabric_pooled: false,
        })
        .context("estate status")?;
    let workers = app.list_worker_enrollments()?;
    if workers.is_empty() && result.workers_enrolled == 0 {
        println!("Estate: no workers enrolled.");
        println!("Start a worker with: lokaid --node --enroll");
        return Ok(());
    }
    println!("Estate workers ({}):", workers.len());
    for w in workers {
        println!(
            "  {} — {} @ {}:{} (enrolled {})",
            w.id, w.label, w.host, w.fabric_port, w.enrolled_at
        );
    }
    Ok(())
}

pub async fn run_worker_add(code: &str, label: Option<&str>) -> Result<()> {
    let app = Application::bootstrap_offline(None).await?;
    let result = app
        .estate
        .enroll_worker(lokai_app::commands::EnrollWorkerCommand {
            code: code.to_string(),
            label: label.map(String::from),
            data_dir: data_dir()?,
        })
        .await
        .context("enroll worker")?;
    println!("Enrolled worker {} ({})", result.worker_id, result.label);
    println!("  host: {} (pinned {})", result.host, result.ip);
    println!("  fabric port: {}", result.fabric_port);
    println!("  egress allow rule added for fabric traffic.");
    Ok(())
}

pub async fn run_worker_trust_set(id_or_label: &str, trust: &str) -> Result<()> {
    let app = Application::bootstrap_offline(None).await?;
    let (id, label, parsed_trust, epoch) = app.set_worker_trust(id_or_label, trust)?;
    println!(
        "Worker {} ({}) trust set to {} (policy_epoch {epoch}).",
        id, label, parsed_trust
    );
    println!("The coordinator will re-read this trust assignment before its next remote dispatch.");
    Ok(())
}

pub async fn run_worker_trust_get(id_or_label: &str) -> Result<()> {
    let app = Application::bootstrap_offline(None).await?;
    let (id, label, trust, audit) = app.get_worker_trust(id_or_label)?;
    println!("Worker {} ({}): trust={trust}", id, label);
    if audit.is_empty() {
        println!("  (no trust changes recorded)");
    } else {
        println!("  audit (newest first):");
        for entry in audit.iter().take(10) {
            println!(
                "    {} epoch={} source={} at {}",
                entry.trust, entry.policy_epoch, entry.source, entry.recorded_at
            );
        }
    }
    Ok(())
}

pub async fn run_worker_remove(ref_id: &str) -> Result<()> {
    let app = Application::bootstrap_offline(None).await?;
    let result = app
        .estate
        .remove_worker(lokai_app::commands::RemoveWorkerCommand {
            ref_id: ref_id.to_string(),
            data_dir: data_dir()?,
        })
        .await
        .context("remove worker")?;
    if result.revoke_pushed {
        println!("Revoke pushed to worker {}.", result.label);
    } else {
        eprintln!(
            "warning: no fabric TLS cert on file or push failed — local un-enroll still applied"
        );
    }
    println!(
        "Removed worker {} ({}) from estate.",
        result.worker_id, result.label
    );
    println!("  egress allow rule cleared on next coordinator/daemon start.");
    Ok(())
}

fn resolve_enrollment_code_input(
    code: Option<String>,
    code_file: Option<PathBuf>,
) -> Result<String> {
    match (code, code_file) {
        (Some(c), None) => Ok(c.trim().to_string()),
        (None, Some(path)) => Ok(std::fs::read_to_string(&path)
            .with_context(|| format!("read enrollment code from {}", path.display()))?
            .trim()
            .to_string()),
        (Some(_), Some(_)) => {
            anyhow::bail!("provide either a code argument or --code-file, not both")
        }
        (None, None) => anyhow::bail!(
            "provide an enrollment code or --code-file (from `lokaid --node --enroll`)"
        ),
    }
}

pub async fn dispatch(estate: EstateCli) -> Result<()> {
    match estate.command {
        EstateSub::Status => run_status().await,
        EstateSub::Capacity { command } => crate::capacity::dispatch(command).await,
        EstateSub::Setup { action } => crate::capacity::dispatch_setup_alias(action).await,
        EstateSub::Worker { action } => match action {
            WorkerSub::Add {
                code,
                code_file,
                label,
            } => {
                let code_str = resolve_enrollment_code_input(code, code_file)?;
                run_worker_add(&code_str, label.as_deref()).await
            }
            WorkerSub::Remove { id_or_label } => run_worker_remove(&id_or_label).await,
            WorkerSub::Trust { action } => match action {
                WorkerTrustSub::Set { id_or_label, trust } => {
                    run_worker_trust_set(&id_or_label, &trust).await
                }
                WorkerTrustSub::Get { id_or_label } => run_worker_trust_get(&id_or_label).await,
            },
        },
        EstateSub::Egress { action } => match action {
            EgressSub::Allow { label, ip, port } => run_egress_allow(&label, &ip, port).await,
            EgressSub::Remove { label } => run_egress_remove(&label).await,
        },
    }
}

#[derive(Parser, Debug)]
#[command(name = "lokai estate", about = "Owned fleet enrollment and status")]
pub struct EstateCli {
    #[command(subcommand)]
    pub command: EstateSub,
}

#[derive(Subcommand, Debug)]
pub enum EstateSub {
    /// List enrolled workers and health summary.
    Status,
    /// Inference capacity profiles (ES5).
    Capacity {
        #[command(subcommand)]
        command: CapacitySub,
    },
    /// Alias for capacity status/doctor (`estate setup status`).
    Setup {
        #[command(subcommand)]
        action: SetupAliasSub,
    },
    /// Worker management.
    Worker {
        #[command(subcommand)]
        action: WorkerSub,
    },
    /// Persisted egress allow rules (loaded on next lokaid initialize).
    Egress {
        #[command(subcommand)]
        action: EgressSub,
    },
}

#[derive(Subcommand, Debug)]
pub enum WorkerSub {
    /// Complete enrollment using a one-time code from `lokaid --node --enroll`.
    Add {
        /// Full code string (`lokai-enroll-v1:...`). Quote it in PowerShell.
        code: Option<String>,
        /// Read code from file (written by `lokaid --node --enroll`).
        #[arg(long)]
        code_file: Option<PathBuf>,
        #[arg(long)]
        label: Option<String>,
    },
    /// Remove an enrolled worker (local DB + best-effort worker revoke push).
    Remove {
        /// Worker id (`worker_…`) or label (`gpu-box`).
        id_or_label: String,
    },
    /// Coordinator-assigned worker trust tier (M5-3).
    Trust {
        #[command(subcommand)]
        action: WorkerTrustSub,
    },
}

#[derive(Subcommand, Debug)]
pub enum WorkerTrustSub {
    /// Assign trust tier for a worker (persisted; live daemon via RPC or restart).
    Set {
        id_or_label: String,
        /// Trust tier: owner_controlled_estate, external_untrusted, etc.
        trust: String,
    },
    /// Show persisted trust and audit history.
    Get { id_or_label: String },
}

#[derive(Subcommand, Debug)]
pub enum EgressSub {
    /// Add or update a persisted egress allow rule.
    Allow {
        /// Stable label (e.g. `my-api`).
        label: String,
        /// Destination IP address.
        ip: String,
        /// Optional destination port (omit for any port on that IP).
        #[arg(long)]
        port: Option<u16>,
    },
    /// Remove a persisted egress allow rule by label.
    Remove { label: String },
}

pub async fn run_egress_allow(label: &str, ip: &str, port: Option<u16>) -> Result<()> {
    let app = Application::bootstrap_offline(None).await?;
    app.upsert_egress_allow_rule(label, ip, port)?;
    println!(
        "Egress allow rule `{label}` → {ip}{} persisted.",
        port.map(|p| format!(":{p}")).unwrap_or_default()
    );
    println!("Restart lokaid (or re-initialize) to load the rule into the live guard.");
    Ok(())
}

pub async fn run_egress_remove(label: &str) -> Result<()> {
    let app = Application::bootstrap_offline(None).await?;
    if app.remove_egress_allow_rule(label)? {
        println!("Removed egress allow rule `{label}`.");
        println!("Restart lokaid (or re-initialize) to update the live guard.");
    } else {
        anyhow::bail!("egress allow rule not found: {label}");
    }
    Ok(())
}
