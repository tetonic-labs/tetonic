use clap::Subcommand;
use tetonic_app::resources::{HuddleProposal, LocalControl};

#[derive(Subcommand)]
pub enum WorkCommand {
    /// Create a durable team goal. Credential on stdin.
    CreateGoal {
        #[arg(long)]
        org: String,
        #[arg(long)]
        team: String,
        #[arg(long)]
        goal: String,
        #[arg(long)]
        title: String,
    },
    /// Create a quick work item (no huddle required). Credential on stdin.
    Create {
        #[arg(long)]
        org: String,
        #[arg(long)]
        team: String,
        #[arg(long)]
        work: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        goal: Option<String>,
    },
    /// List work items for a team. Credential on stdin.
    List {
        #[arg(long)]
        org: String,
        #[arg(long)]
        team: String,
    },
    /// Park open or running work. Credential on stdin.
    Park {
        #[arg(long)]
        org: String,
        #[arg(long)]
        team: String,
        #[arg(long)]
        work: String,
    },
    /// Resume parked work after membership recheck. Credential on stdin.
    Resume {
        #[arg(long)]
        org: String,
        #[arg(long)]
        team: String,
        #[arg(long)]
        work: String,
    },
    /// Accept a versioned huddle proposal; creates work idempotently. Credential on stdin.
    AcceptHuddle {
        #[arg(long)]
        org: String,
        #[arg(long)]
        team: String,
        #[arg(long)]
        huddle: String,
        #[arg(long)]
        request_id: String,
        #[arg(long, default_value_t = 1)]
        version: i64,
        /// Repeatable work titles for the proposal.
        #[arg(long = "title", required = true)]
        titles: Vec<String>,
    },
}

pub async fn dispatch(control: &LocalControl, command: WorkCommand) -> anyhow::Result<()> {
    let credential = super::credential_from_stdin().await?;
    let resources = control.resources();
    match command {
        WorkCommand::CreateGoal {
            org,
            team,
            goal,
            title,
        } => {
            let row = resources
                .create_team_goal(&credential, org, team, goal, title)
                .await?;
            println!("{}", serde_json::to_string(&row)?);
        }
        WorkCommand::Create {
            org,
            team,
            work,
            title,
            request_id,
            goal,
        } => {
            let row = resources
                .create_team_work_item(&credential, org, team, work, title, request_id, goal)
                .await?;
            println!("{}", serde_json::to_string(&row)?);
        }
        WorkCommand::List { org, team } => {
            let rows = resources
                .list_team_work_items(&credential, org, team)
                .await?;
            println!("{}", serde_json::to_string(&rows)?);
        }
        WorkCommand::Park { org, team, work } => {
            let row = resources
                .park_team_work_item(&credential, org, team, work)
                .await?;
            println!("{}", serde_json::to_string(&row)?);
        }
        WorkCommand::Resume { org, team, work } => {
            let row = resources
                .resume_team_work_item(&credential, org, team, work)
                .await?;
            println!("{}", serde_json::to_string(&row)?);
        }
        WorkCommand::AcceptHuddle {
            org,
            team,
            huddle,
            request_id,
            version,
            titles,
        } => {
            let rows = resources
                .accept_huddle_proposal(
                    &credential,
                    HuddleProposal {
                        org_id: org,
                        team_id: team,
                        huddle_id: huddle,
                        proposal_version: version,
                        status: "accepted".into(),
                        request_id,
                        created_by: String::new(),
                        work_titles: titles,
                    },
                )
                .await?;
            println!("{}", serde_json::to_string(&rows)?);
        }
    }
    Ok(())
}
