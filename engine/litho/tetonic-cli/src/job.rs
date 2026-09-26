//! Local launch of a registered job. Host settings come from an operator file.
//! The employee request cannot set the workspace, model, tool ceiling, or deadline.
use std::{
    io::{IsTerminal, Read},
    path::PathBuf,
};

use clap::{Parser, Subcommand};
use tetonic_app::resources::{RegisteredAgentJob, TeamWorkLaunch};
use tetonic_app::{host_settings_from_json, launch_registered_job, launch_team_work};

#[derive(Parser)]
#[command(
    name = "tetonic job",
    bin_name = "tetonic job",
    about = "Launch one registered job and wait for its terminal outcome"
)]
pub struct JobCli {
    #[arg(long)]
    database: PathBuf,
    #[arg(long)]
    audience: String,
    /// Operator-owned host settings. Not an employee permission grant.
    #[arg(long)]
    host_settings: PathBuf,
    #[command(subcommand)]
    command: JobCommand,
}

#[derive(Subcommand)]
enum JobCommand {
    /// Activate a granted agent. Pipe the bearer credential to stdin.
    Run {
        #[arg(long)]
        org: String,
        /// Bound information context. Not used together with `--team`.
        #[arg(long, required_unless_present = "team", conflicts_with = "team")]
        context: Option<String>,
        /// Use the caller's private working context for this team.
        #[arg(long, required_unless_present = "context", conflicts_with = "context")]
        team: Option<String>,
        /// When set, activate this durable work item through the managed path.
        #[arg(long)]
        work: Option<String>,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        definition_digest: String,
        #[arg(long)]
        grant: String,
        #[arg(long, required_unless_present = "work")]
        request_id: Option<String>,
        #[arg(long)]
        recovery_id: String,
        #[arg(long, required_unless_present = "work")]
        input_file: Option<PathBuf>,
        /// Draw the terminal outcome and journal event names after activation.
        #[arg(long)]
        view: bool,
    },
}

pub async fn dispatch(args: JobCli) -> anyhow::Result<()> {
    let JobCommand::Run {
        org,
        context,
        team,
        work,
        agent,
        definition_digest,
        grant,
        request_id,
        recovery_id,
        input_file,
        view,
    } = args.command;
    let credential = credential_from_stdin().await?;
    let team_id = team.clone();
    let context = match team {
        Some(team) => {
            let local = tetonic_app::resources::LocalControl::open(
                args.database.clone(),
                args.audience.clone(),
            )
            .await?;
            let context = local
                .contexts()
                .team_participation_context(&credential, org.clone(), team)
                .await?;
            drop(local);
            context
        }
        None => context.expect("context is required when --team is absent"),
    };
    let host = host_settings_from_json(
        &std::fs::read(&args.host_settings)?,
        args.database,
        args.audience,
    )?;
    let receipt = if let Some(work_id) = work {
        let team = team_id.ok_or_else(|| anyhow::anyhow!("--work requires --team"))?;
        let (_work, receipt) = launch_team_work(
            &credential,
            host,
            TeamWorkLaunch {
                organization_id: org,
                team_id: team,
                work_id,
                information_context_id: context,
                agent_key: agent,
                definition_digest,
                execution_grant_id: grant,
                input: match input_file {
                    Some(path) => Some(std::fs::read_to_string(path)?),
                    None => None,
                },
                recovery_id,
            },
        )
        .await
        .map_err(|error| anyhow::anyhow!(error.employee_message()))?;
        receipt
    } else {
        let input = std::fs::read_to_string(
            input_file.expect("input_file is required when --work is absent"),
        )?;
        launch_registered_job(
            &credential,
            host,
            RegisteredAgentJob {
                request_id: request_id.expect("request_id is required when --work is absent"),
                organization_id: org,
                information_context_id: context,
                agent_key: agent,
                definition_digest,
                execution_grant_id: grant,
                input,
                recovery_id,
            },
        )
        .await
        .map_err(|error| anyhow::anyhow!(error.employee_message()))?
    };
    println!("{}", serde_json::to_string(&receipt)?);
    if view {
        eprint!("{}", crate::job_view::activation_text(&receipt));
    }
    Ok(())
}

async fn credential_from_stdin() -> anyhow::Result<String> {
    tokio::task::spawn_blocking(|| -> anyhow::Result<String> {
        anyhow::ensure!(
            !std::io::stdin().is_terminal(),
            "pipe the bearer credential to stdin; interactive input would expose it"
        );
        let mut input = String::new();
        std::io::stdin().take(1025).read_to_string(&mut input)?;
        anyhow::ensure!(input.len() <= 1024, "credential input exceeds limit");
        let value = input.trim_end_matches(['\r', '\n']).to_string();
        anyhow::ensure!(!value.is_empty(), "a bearer credential is required on stdin");
        Ok(value)
    })
    .await?
}
