use clap::Subcommand;
use tetonic_app::resources::LocalControl;

#[derive(Subcommand)]
pub enum ExecutionLimitsCommand {
    /// Read organization admission ceilings; member credential on stdin.
    Get {
        #[arg(long)]
        org: String,
    },
    /// Set ceilings using the revision returned by get; administrator credential on stdin.
    Set {
        #[arg(long)]
        org: String,
        #[arg(long)]
        expected_revision: u64,
        /// Concurrent registered runs across the organization. Zero blocks new work.
        #[arg(long)]
        max_active_runs: u32,
        /// Concurrent registered runs charged to each initiating human principal.
        #[arg(long)]
        max_active_runs_per_principal: u32,
    },
}

#[derive(Subcommand)]
pub enum TeamExecutionLimitsCommand {
    /// Read a team admission ceiling; team reader credential on stdin.
    Get {
        #[arg(long)]
        org: String,
        #[arg(long)]
        team: String,
    },
    /// Set the ceiling using the revision returned by get; team manager credential on stdin.
    Set {
        #[arg(long)]
        org: String,
        #[arg(long)]
        team: String,
        #[arg(long)]
        expected_revision: u64,
        /// Concurrent registered runs in this team. Zero blocks new team work.
        #[arg(long)]
        max_active_runs: u32,
    },
}

pub async fn dispatch_team(
    control: &LocalControl,
    command: TeamExecutionLimitsCommand,
) -> anyhow::Result<()> {
    let credential = super::credential_from_stdin().await?;
    let limits = match command {
        TeamExecutionLimitsCommand::Get { org, team } => {
            control
                .resources()
                .get_team_execution_limits(&credential, org, team)
                .await?
        }
        TeamExecutionLimitsCommand::Set {
            org,
            team,
            expected_revision,
            max_active_runs,
        } => {
            control
                .resources()
                .set_team_execution_limits(
                    &credential,
                    org,
                    team,
                    expected_revision,
                    max_active_runs,
                )
                .await?
        }
    };
    println!("{}", serde_json::to_string(&limits)?);
    Ok(())
}

pub async fn dispatch(
    control: &LocalControl,
    command: ExecutionLimitsCommand,
) -> anyhow::Result<()> {
    let credential = super::credential_from_stdin().await?;
    let limits = match command {
        ExecutionLimitsCommand::Get { org } => {
            control
                .resources()
                .get_execution_limits(&credential, org)
                .await?
        }
        ExecutionLimitsCommand::Set {
            org,
            expected_revision,
            max_active_runs,
            max_active_runs_per_principal,
        } => {
            control
                .resources()
                .set_execution_limits(
                    &credential,
                    org,
                    expected_revision,
                    max_active_runs,
                    max_active_runs_per_principal,
                )
                .await?
        }
    };
    println!("{}", serde_json::to_string(&limits)?);
    Ok(())
}
