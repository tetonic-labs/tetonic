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
    /// Hierarchical pause/cancel/estop for org, team, goal, work, or agent.
    Stop {
        #[arg(long)]
        org: String,
        #[arg(long)]
        scope: String,
        #[arg(long)]
        id: String,
        #[arg(long)]
        mode: String,
        #[arg(long)]
        reason: String,
    },
    /// Clear an active stop so work may resume after inspection.
    ClearStop {
        #[arg(long)]
        org: String,
        #[arg(long)]
        scope: String,
        #[arg(long)]
        id: String,
    },
    /// Propose an effect approval (does not dispatch). Credential on stdin.
    ProposeApproval {
        #[arg(long)]
        org: String,
        #[arg(long)]
        team: String,
        #[arg(long)]
        approval: String,
        #[arg(long)]
        digest: String,
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        expires_at: i64,
        #[arg(long)]
        work: Option<String>,
    },
    /// Approve or reject a pending effect proposal.
    ResolveApproval {
        #[arg(long)]
        org: String,
        #[arg(long)]
        team: String,
        #[arg(long)]
        approval: String,
        #[arg(long)]
        digest: String,
        #[arg(long)]
        allow: bool,
    },
    /// One team view: work, stops, effort, pending approvals. No private bodies.
    InspectTeam {
        #[arg(long)]
        org: String,
        #[arg(long)]
        team: String,
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
        WorkCommand::Stop {
            org,
            scope,
            id,
            mode,
            reason,
        } => {
            let row = resources
                .request_control_stop(&credential, org, scope, id, mode, reason)
                .await?;
            println!("{}", serde_json::to_string(&row)?);
        }
        WorkCommand::ClearStop { org, scope, id } => {
            let row = resources
                .clear_control_stop(&credential, org, scope, id)
                .await?;
            println!("{}", serde_json::to_string(&row)?);
        }
        WorkCommand::ProposeApproval {
            org,
            team,
            approval,
            digest,
            request_id,
            expires_at,
            work,
        } => {
            let row = resources
                .propose_effect_approval(
                    &credential,
                    org,
                    team,
                    approval,
                    digest,
                    request_id,
                    expires_at,
                    work,
                )
                .await?;
            println!("{}", serde_json::to_string(&row)?);
        }
        WorkCommand::ResolveApproval {
            org,
            team,
            approval,
            digest,
            allow,
        } => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let row = resources
                .resolve_effect_approval(&credential, org, team, approval, digest, allow, now)
                .await?;
            println!("{}", serde_json::to_string(&row)?);
        }
        WorkCommand::InspectTeam { org, team } => {
            let row = resources.inspect_team_work(&credential, org, team).await?;
            println!("{}", serde_json::to_string(&row)?);
        }
    }
    Ok(())
}
