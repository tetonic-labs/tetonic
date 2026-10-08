//! Host-bound shell approval for existing team work. No model-supplied authority.
use std::{path::PathBuf, sync::Arc, time::Duration};
use tetonic_core::{ApprovalHook, ApprovalRequest};
use tetonic_domain::{execution::CanonicalActionParameters, ExecutionScope};
use tetonic_memory::{SharedStore, ShellApprovalProposal};

pub(super) fn for_work(
    store: SharedStore,
    scope: ExecutionScope,
    work: Option<(String, String)>,
    root: Option<PathBuf>,
    approved_shared_environment: bool,
) -> ApprovalHook {
    Arc::new(move |request| {
        let (store, scope, work, root) = (store.clone(), scope.clone(), work.clone(), root.clone());
        Box::pin(async move {
            let (Some((team, work)), Some(root)) = (work, root) else {
                return false;
            };
            // Do not publish private-context command content into a team's inbox.
            if !approved_shared_environment
                && tetonic_memory::team_participation_context_id(
                    &scope.organization_id,
                    &team,
                    &scope.principal_id,
                )
                .ok()
                .as_deref()
                    != Some(scope.information_context_id.as_str())
            {
                return false;
            }
            approve(store, scope, team, work, root, request)
                .await
                .unwrap_or(false)
        })
    })
}

async fn approve(
    store: SharedStore,
    scope: ExecutionScope,
    team: String,
    work: String,
    root: PathBuf,
    mut request: ApprovalRequest,
) -> Option<bool> {
    if request.tool != "run_shell" {
        return None;
    }
    let params: CanonicalActionParameters = serde_json::from_value(request.args.clone()).ok()?;
    let command = String::from_utf8(params.script_bytes?).ok()?;
    let cwd = params.working_directory?;
    if std::path::Path::new(&cwd) != root || params.digest.is_empty() {
        return None;
    }
    crate::approval::attach_shell_confinement(&mut request, &root);
    let proposal = ShellApprovalProposal {
        command,
        working_directory: cwd,
        shell: params.shell_identity?,
        attempt_id: request.attempt_id?,
        call_id: request.call_id,
        parameter_digest: params.digest,
        confinement_warnings: request
            .missing_controls
            .iter()
            .map(|w| format!("{}: {}", w.control, w.reason))
            .collect(),
    };
    let id = format!("shell-{}", uuid::Uuid::new_v4());
    let digest = proposal.digest();
    let org = scope.organization_id;
    let row = store
        .write({
            let (org, team, id, digest) = (org.clone(), team.clone(), id.clone(), digest.clone());
            move |db| {
                db.propose_effect_approval(tetonic_memory::ProposeEffectApproval {
                    actor: &scope.principal_id,
                    org: &org,
                    team: &team,
                    approval_id: &id,
                    proposal_digest: &digest,
                    request_id: &id,
                    // The attempt's current clock is authoritative, including
                    // a resumed lease. Never mint a fresh allowance here.
                    expires_at: db.live_work_execution_deadline(
                        &org,
                        &team,
                        &work,
                        &proposal.attempt_id,
                        chrono::Utc::now().timestamp() as u64,
                    )? as i64,
                    work_id: Some(&work),
                    proposal: Some(&proposal),
                })
            }
        })
        .await
        .ok()?
        .ok()?;
    loop {
        if chrono::Utc::now().timestamp() >= row.expires_at {
            return Some(false);
        }
        let result = store
            .write({
                let (org, team, id, digest) =
                    (org.clone(), team.clone(), id.clone(), digest.clone());
                move |db| {
                    db.consume_shell_approval(
                        &org,
                        &team,
                        &id,
                        &digest,
                        chrono::Utc::now().timestamp(),
                    )
                }
            })
            .await
            .ok()?
            .ok()?;
        if result.is_some() {
            return result;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}
