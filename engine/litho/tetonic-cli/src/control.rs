//! Offline/local operator control. Never starts inference or an agent session.
use clap::{Parser, Subcommand};
mod agents;
mod contexts;
mod execution_limits;
mod work;
use std::{
    io::{IsTerminal, Read},
    path::PathBuf,
};
use tetonic_app::resources::{LocalControl, OrganizationRole};

#[derive(Parser)]
#[command(
    name = "tetonic control",
    bin_name = "tetonic control",
    about = "Local operator access to durable organizations and teams (database access is administrative)"
)]
pub struct ControlCli {
    #[arg(long)]
    database: PathBuf,
    #[arg(long)]
    audience: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Durable organization admission ceilings; does not cancel already admitted work.
    ExecutionLimits {
        #[command(subcommand)]
        command: execution_limits::ExecutionLimitsCommand,
    },
    /// Durable team admission ceiling; does not cancel already admitted work.
    TeamExecutionLimits {
        #[command(subcommand)]
        command: execution_limits::TeamExecutionLimitsCommand,
    },
    /// Replay authorized governed lifecycle events; credential on stdin.
    ReplayRun {
        #[arg(long)]
        org: String,
        #[arg(long)]
        context: String,
        #[arg(long)]
        run: String,
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long, default_value_t = 100)]
        limit: u32,
        /// Keep reading new events until the run is terminal or access is denied.
        #[arg(long, default_value_t = false)]
        follow: bool,
    },
    /// Inspect a governed run through current context membership; credential on stdin.
    InspectRun {
        #[arg(long)]
        org: String,
        #[arg(long)]
        context: String,
        #[arg(long)]
        run: String,
    },
    /// Durable team goals, work items and huddles (does not activate agents).
    Work {
        #[command(subcommand)]
        command: work::WorkCommand,
    },
    /// Organization-owned agent configuration; no execution privileges are granted.
    Agent {
        #[command(subcommand)]
        command: agents::AgentCommand,
    },
    /// Authenticated private/team discussion history (does not activate agents).
    Context {
        #[command(subcommand)]
        command: contexts::ContextCommand,
    },
    /// Grant explicit team metadata access; administrator or owner credential on stdin.
    AddTeamMember {
        #[arg(long)]
        org: String,
        #[arg(long)]
        team: String,
        #[arg(long)]
        principal: String,
    },
    /// Remove explicit membership; does not revoke owner or organization-admin rights.
    RemoveTeamMember {
        #[arg(long)]
        org: String,
        #[arg(long)]
        team: String,
        #[arg(long)]
        principal: String,
    },
    /// Trusted local operator: register an identity without granting membership.
    RegisterPrincipal {
        #[arg(long)]
        principal: String,
    },
    /// Set organization membership using an administrator credential on stdin.
    SetMember {
        #[arg(long)]
        org: String,
        #[arg(long)]
        principal: String,
        #[arg(long, value_enum)]
        role: MemberRole,
    },
    /// Remove organization membership using an administrator credential on stdin.
    RemoveMember {
        #[arg(long)]
        org: String,
        #[arg(long)]
        principal: String,
    },
    /// Initialize the first administrator once. Existing principals forbid this.
    Bootstrap {
        #[arg(long)]
        principal: String,
        #[arg(long)]
        org: String,
        #[arg(long)]
        name: String,
    },
    /// Trusted local operator: print a new bearer secret to stdout. Protect output.
    IssueCredential {
        #[arg(long)]
        principal: String,
        #[arg(long, default_value_t = 3600)]
        lifetime_seconds: u32,
    },
    /// Trusted local operator: revoke by public credential ID.
    RevokeCredential {
        #[arg(long)]
        credential_id: String,
    },
    /// Create a team; read the bearer credential from stdin, never command arguments.
    CreateTeam {
        #[arg(long)]
        org: String,
        #[arg(long)]
        team: String,
        #[arg(long)]
        name: String,
    },
    /// Inspect a team using a credential from stdin.
    GetTeam {
        #[arg(long)]
        org: String,
        #[arg(long)]
        team: String,
    },
}

#[derive(Clone, clap::ValueEnum)]
enum MemberRole {
    Administrator,
    TeamCreator,
    Member,
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
        anyhow::ensure!(
            !value.is_empty(),
            "a bearer credential is required on stdin"
        );
        Ok(value)
    })
    .await?
}

pub async fn dispatch(args: ControlCli) -> anyhow::Result<()> {
    let control = LocalControl::open(args.database, args.audience).await?;
    match args.command {
        Command::ExecutionLimits { command } => {
            execution_limits::dispatch(&control, command).await?
        }
        Command::TeamExecutionLimits { command } => {
            execution_limits::dispatch_team(&control, command).await?
        }
        Command::ReplayRun {
            org,
            context,
            run,
            after,
            limit,
            follow,
        } => {
            let credential = credential_from_stdin().await?;
            if !follow {
                let replay = control
                    .contexts()
                    .replay_run(&credential, org, context, run, after, limit)
                    .await?;
                let output = match replay {
                    Ok(events) => serde_json::json!({"events":events}),
                    Err(gap) => serde_json::json!({"gap":gap}),
                };
                println!("{output}");
            } else {
                let mut cursor = after;
                loop {
                    match control
                        .contexts()
                        .poll_run(&credential, org.clone(), context.clone(), run.clone(), cursor, limit)
                        .await?
                    {
                        tetonic_app::resources::RunPoll::CaughtUp => break,
                        tetonic_app::resources::RunPoll::Gap(gap) => {
                            println!("{}", serde_json::json!({"gap": gap}));
                            break;
                        }
                        tetonic_app::resources::RunPoll::Events(events) => {
                            if let Some(last) = events.last() {
                                cursor = last.sequence;
                            }
                            if !events.is_empty() {
                                println!("{}", serde_json::json!({"events": events}));
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                        }
                    }
                }
            }
        }
        Command::InspectRun { org, context, run } => {
            let credential = credential_from_stdin().await?;
            let snapshot = control
                .contexts()
                .inspect_run(&credential, org, context, run)
                .await?;
            println!("{}", serde_json::to_string(&snapshot)?);
        }
        Command::Agent { command } => agents::dispatch(&control, command).await?,
        Command::Work { command } => work::dispatch(&control, command).await?,
        Command::Context { command } => contexts::dispatch(&control, command).await?,
        Command::AddTeamMember {
            org,
            team,
            principal,
        } => {
            let credential = credential_from_stdin().await?;
            control
                .resources()
                .set_team_member(&credential, org.clone(), team.clone(), principal.clone(), true)
                .await?;
            let working_context_id =
                tetonic_app::resources::team_participation_context_id(&org, &team, &principal)?;
            println!(
                "{}",
                serde_json::json!({
                    "explicit_membership_updated": true,
                    "working_context_id": working_context_id
                })
            );
        }
        Command::RemoveTeamMember {
            org,
            team,
            principal,
        } => {
            let credential = credential_from_stdin().await?;
            control
                .resources()
                .set_team_member(&credential, org, team, principal, false)
                .await?;
            println!(
                "{}",
                serde_json::json!({"explicit_membership_updated":true})
            );
        }
        Command::RegisterPrincipal { principal } => {
            control.register_principal(principal).await?;
            println!("{}", serde_json::json!({"registration_processed":true}));
        }
        Command::SetMember {
            org,
            principal,
            role,
        } => {
            let credential = credential_from_stdin().await?;
            let role = match role {
                MemberRole::Administrator => OrganizationRole::Administrator,
                MemberRole::TeamCreator => OrganizationRole::TeamCreator,
                MemberRole::Member => OrganizationRole::Member,
            };
            control
                .resources()
                .set_organization_member(&credential, org, principal, Some(role))
                .await?;
            println!("{}", serde_json::json!({"membership_updated":true}));
        }
        Command::RemoveMember { org, principal } => {
            let credential = credential_from_stdin().await?;
            control
                .resources()
                .set_organization_member(&credential, org, principal, None)
                .await?;
            println!("{}", serde_json::json!({"membership_updated":true}));
        }
        Command::Bootstrap {
            principal,
            org,
            name,
        } => {
            control.bootstrap(principal, org, name).await?;
            println!("{}", serde_json::json!({"initialized":true}));
        }
        Command::IssueCredential {
            principal,
            lifetime_seconds,
        } => {
            let key = control
                .credentials()
                .issue(principal, lifetime_seconds)
                .await?;
            println!(
                "{}",
                serde_json::json!({"credential_id":key.credential_id,"expires_at":key.expires_at,"credential":key.expose_secret()})
            );
        }
        Command::RevokeCredential { credential_id } => {
            control.credentials().revoke(credential_id).await?;
            println!("{}", serde_json::json!({"revocation_processed":true}));
        }
        Command::CreateTeam { org, team, name } => {
            let credential = credential_from_stdin().await?;
            let row = control
                .resources()
                .create_team(&credential, org, team, name)
                .await?;
            let working_context_id = tetonic_app::resources::team_participation_context_id(
                &row.org_id,
                &row.team_id,
                &row.owner_principal_id,
            )?;
            println!(
                "{}",
                serde_json::json!({
                    "org_id": row.org_id,
                    "team_id": row.team_id,
                    "name": row.name,
                    "owner_principal_id": row.owner_principal_id,
                    "working_context_id": working_context_id
                })
            );
        }
        Command::GetTeam { org, team } => {
            let credential = credential_from_stdin().await?;
            let row = control.resources().get_team(&credential, org, team).await?;
            let result=row.map(|r|serde_json::json!({"org_id":r.org_id,"team_id":r.team_id,"name":r.name,"owner_principal_id":r.owner_principal_id}));
            println!("{}", serde_json::to_string(&result)?);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_operator_commands_and_rejects_bearer_argument() {
        assert!(ControlCli::try_parse_from([
            "control",
            "--database",
            "db",
            "--audience",
            "local",
            "bootstrap",
            "--principal",
            "local/admin",
            "--org",
            "a",
            "--name",
            "A"
        ])
        .is_ok());
        assert!(ControlCli::try_parse_from([
            "control",
            "--database",
            "db",
            "--audience",
            "local",
            "get-team",
            "--org",
            "a",
            "--team",
            "b",
            "--credential",
            "secret"
        ])
        .is_err());
    }
}
