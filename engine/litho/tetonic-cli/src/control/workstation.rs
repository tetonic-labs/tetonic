//! Org workstation enrollment and placement (MVP-501/502). Offline parks pins.
use clap::Subcommand;
use tetonic_app::resources::LocalControl;

#[derive(Subcommand)]
pub enum WorkstationCommand {
    /// Enroll a device under an organization. Device secret is distinct from the employee credential.
    Enroll {
        #[arg(long)]
        org: String,
        #[arg(long)]
        workstation: String,
        #[arg(long)]
        label: String,
        /// Explicit platform: linux, macos, or windows.
        #[arg(long)]
        platform: String,
        #[arg(long)]
        device_secret: String,
        #[arg(long, default_value_t = false)]
        shared_assignment: bool,
    },
    /// Owner-approved local resource grant.
    ApproveGrant {
        #[arg(long)]
        org: String,
        #[arg(long)]
        workstation: String,
        #[arg(long)]
        grant: String,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        resource: String,
    },
    /// Pin a work item to a workstation.
    Pin {
        #[arg(long)]
        org: String,
        #[arg(long)]
        team: String,
        #[arg(long)]
        work: String,
        #[arg(long)]
        workstation: String,
    },
    /// Mark offline and park pinned work in place.
    Offline {
        #[arg(long)]
        org: String,
        #[arg(long)]
        workstation: String,
    },
    /// Reconnect without duplicating parked work.
    Reconnect {
        #[arg(long)]
        org: String,
        #[arg(long)]
        workstation: String,
    },
    /// Drain: refuse new assignment claims.
    Drain {
        #[arg(long)]
        org: String,
        #[arg(long)]
        workstation: String,
    },
    /// Revoke device credential, fence claims, clear grants, park pins.
    Revoke {
        #[arg(long)]
        org: String,
        #[arg(long)]
        workstation: String,
    },
}

pub async fn dispatch(control: &LocalControl, command: WorkstationCommand) -> anyhow::Result<()> {
    let credential = super::credential_from_stdin().await?;
    let resources = control.resources();
    match command {
        WorkstationCommand::Enroll {
            org,
            workstation,
            label,
            platform,
            device_secret,
            shared_assignment,
        } => {
            let row = resources
                .enroll_workstation(
                    &credential,
                    device_secret,
                    tetonic_app::resources::EnrollWorkstation {
                        org,
                        workstation_id: workstation,
                        label,
                        platform,
                        shared_assignment,
                    },
                )
                .await?;
            println!("{}", serde_json::to_string(&row)?);
        }
        WorkstationCommand::ApproveGrant {
            org,
            workstation,
            grant,
            kind,
            resource,
        } => {
            let row = resources
                .approve_workstation_grant(&credential, org, workstation, grant, kind, resource)
                .await?;
            println!("{}", serde_json::to_string(&row)?);
        }
        WorkstationCommand::Pin {
            org,
            team,
            work,
            workstation,
        } => {
            let row = resources
                .pin_work_to_workstation(&credential, org, team, work, workstation)
                .await?;
            println!("{}", serde_json::to_string(&row)?);
        }
        WorkstationCommand::Offline { org, workstation } => {
            let (row, parked) = resources
                .mark_workstation_offline(&credential, org, workstation)
                .await?;
            println!(
                "{}",
                serde_json::json!({"workstation": row, "parked_work": parked})
            );
        }
        WorkstationCommand::Reconnect { org, workstation } => {
            let row = resources
                .reconnect_workstation(&credential, org, workstation)
                .await?;
            println!("{}", serde_json::to_string(&row)?);
        }
        WorkstationCommand::Drain { org, workstation } => {
            let row = resources
                .drain_workstation(&credential, org, workstation)
                .await?;
            println!("{}", serde_json::to_string(&row)?);
        }
        WorkstationCommand::Revoke { org, workstation } => {
            let row = resources
                .revoke_workstation(&credential, org, workstation)
                .await?;
            println!("{}", serde_json::to_string(&row)?);
        }
    }
    Ok(())
}
