use clap::Subcommand;
use std::{io::Read, path::PathBuf};
use tetonic_app::resources::{ContextOwner, LocalControl};

#[derive(Subcommand)]
pub enum ContextCommand {
    /// Close discussion history; preserves messages and does not cancel agents.
    Close {
        #[arg(long)]
        context: String,
        #[arg(long)]
        session: String,
    },
    /// Search authorized messages and tool results in this context.
    Recall {
        #[arg(long)]
        context: String,
        #[arg(long)]
        query: String,
        #[arg(long, default_value_t=8, value_parser=clap::value_parser!(u32).range(1..=30))]
        limit: u32,
    },
    /// Create a context privately owned by the authenticated principal.
    Private {
        #[arg(long)]
        org: String,
        #[arg(long)]
        context: String,
    },
    /// Create a shared context; requires current team participation.
    Team {
        #[arg(long)]
        org: String,
        #[arg(long)]
        team: String,
        #[arg(long)]
        context: String,
    },
    /// Open durable discussion history; no workspace access or inference is granted.
    /// Omit `--session` to create a discussion and receive a server-chosen id.
    Open {
        #[arg(long)]
        context: String,
        /// Reopen this discussion. A missing or foreign id is denied and creates nothing.
        #[arg(long)]
        session: Option<String>,
    },
    /// Store a human message from a UTF-8 file (maximum 64 KiB).
    Send {
        #[arg(long)]
        context: String,
        #[arg(long)]
        session: String,
        /// Reuse this ID for an identical retry; use a new ID for a new message.
        #[arg(long)]
        request: String,
        #[arg(long)]
        message_file: PathBuf,
    },
    /// Read the latest authorized messages as JSON in chronological order.
    History {
        #[arg(long)]
        context: String,
        #[arg(long)]
        session: String,
        #[arg(long, default_value_t=50, value_parser=clap::value_parser!(u32).range(1..=200))]
        limit: u32,
    },
}

pub async fn dispatch(control: &LocalControl, command: ContextCommand) -> anyhow::Result<()> {
    let credential = super::credential_from_stdin().await?;
    let service = control.contexts();
    let output = match command {
        ContextCommand::Close { context, session } => {
            service.close_history(&credential, context, session).await?;
            serde_json::json!({"discussion_closed":true})
        }
        ContextCommand::Recall {
            context,
            query,
            limit,
        } => {
            let hits = service.recall(&credential, context, query, limit).await?;
            let hits: Vec<_> = hits.into_iter().map(|hit|serde_json::json!({"session_id":hit.session_id,"kind":hit.kind,"role":hit.label,"snippet":hit.snippet})).collect();
            serde_json::json!({"hits":hits})
        }
        ContextCommand::Private { org, context } => {
            service
                .create(&credential, context, ContextOwner::Private { org_id: org })
                .await?;
            serde_json::json!({"context_created":true})
        }
        ContextCommand::Team { org, team, context } => {
            service
                .create(
                    &credential,
                    context,
                    ContextOwner::Team {
                        org_id: org,
                        team_id: team,
                    },
                )
                .await?;
            serde_json::json!({"context_created":true})
        }
        ContextCommand::Open { context, session } => match session {
            Some(session) => {
                service
                    .open_history(&credential, context, session.clone())
                    .await?;
                serde_json::json!({"discussion_open":true,"session":session,"agent_activated":false})
            }
            None => {
                let session = service.create_history(&credential, context).await?;
                serde_json::json!({"discussion_open":true,"session":session,"created":true,"agent_activated":false})
            }
        },
        ContextCommand::Send {
            context,
            session,
            request,
            message_file,
        } => {
            let content = tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
                let mut text = String::new();
                std::fs::File::open(message_file)?
                    .take(65_537)
                    .read_to_string(&mut text)?;
                anyhow::ensure!(
                    !text.is_empty() && text.len() <= 65_536,
                    "message must contain 1 to 65536 UTF-8 bytes"
                );
                Ok(text)
            })
            .await??;
            let sequence = service
                .append_message(&credential, context, session, request, content)
                .await?;
            serde_json::json!({"stored":true,"sequence":sequence,"agent_activated":false})
        }
        ContextCommand::History {
            context,
            session,
            limit,
        } => {
            let rows = service
                .transcript(&credential, context, session, limit)
                .await?;
            let messages: Vec<_> = rows.into_iter().map(|(seq,role,content)|serde_json::json!({"sequence":seq,"role":role,"content":content})).collect();
            serde_json::json!({"messages":messages})
        }
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
