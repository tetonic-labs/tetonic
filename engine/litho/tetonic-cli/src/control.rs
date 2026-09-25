//! Offline/local operator control. Never starts inference or an agent session.
use clap::{Parser, Subcommand};
use std::{
    io::{IsTerminal, Read},
    path::PathBuf,
};
use tetonic_app::resources::LocalControl;

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
            println!(
                "{}",
                serde_json::json!({"org_id":row.org_id,"team_id":row.team_id,"name":row.name,"owner_principal_id":row.owner_principal_id})
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
