use clap::Subcommand;
use std::{io::Read, path::PathBuf};
use tetonic_app::resources::LocalControl;

#[derive(Subcommand)]
pub enum AgentCommand {
    /// Register immutable organization-owned configuration; does not activate an agent.
    Register {
        #[arg(long)]
        org: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        harness: String,
        /// JSON object with harness-specific configuration (maximum 64 KiB).
        #[arg(long)]
        config_file: PathBuf,
    },
    /// Inspect a registered configuration and its durable identity revision.
    Get {
        #[arg(long)]
        org: String,
        #[arg(long)]
        agent: String,
    },
}

pub async fn dispatch(control: &LocalControl, command: AgentCommand) -> anyhow::Result<()> {
    let credential = super::credential_from_stdin().await?;
    let resources = control.resources();
    let registered = match command {
        AgentCommand::Register {
            org,
            agent,
            harness,
            config_file,
        } => {
            let configuration =
                tokio::task::spawn_blocking(move || -> anyhow::Result<serde_json::Value> {
                    let mut bytes = Vec::new();
                    std::fs::File::open(config_file)?
                        .take(65_537)
                        .read_to_end(&mut bytes)?;
                    anyhow::ensure!(bytes.len() <= 65_536, "configuration exceeds 65536 bytes");
                    let value: serde_json::Value = serde_json::from_slice(
                        bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes),
                    )?;
                    anyhow::ensure!(value.is_object(), "configuration must be a JSON object");
                    Ok(value)
                })
                .await??;
            resources
                .register_agent(&credential, org, agent, harness, configuration)
                .await?
        }
        AgentCommand::Get { org, agent } => resources
            .get_agent(&credential, org, agent)
            .await?
            .ok_or_else(|| anyhow::anyhow!("agent registration not found"))?,
    };
    let definition: serde_json::Value = serde_json::from_str(&registered.definition_json)?;
    println!(
        "{}",
        serde_json::json!({
            "identity_id":registered.identity.identity_id,
            "definition_digest":registered.identity.bound_definition_digest,
            "definition":definition,
            "privilege_class":registered.identity.privilege_class,
            "agent_activated":false
        })
    );
    Ok(())
}
