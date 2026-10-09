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
            let Some((team, work)) = work else {
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
    root: Option<PathBuf>,
    mut request: ApprovalRequest,
) -> Option<bool> {
    let params: CanonicalActionParameters = serde_json::from_value(request.args.clone()).ok()?;
    if params.digest.is_empty() {
        return None;
    }
    let is_shell = request.tool == "run_shell";
    let (command, cwd, shell) = if is_shell {
        let root = root?;
        let cwd = params.working_directory.clone()?;
        if std::path::Path::new(&cwd) != root {
            return None;
        }
        crate::approval::attach_shell_confinement(&mut request, &root);
        (
            String::from_utf8(params.script_bytes.clone()?).ok()?,
            cwd,
            params.shell_identity.clone()?,
        )
    } else {
        // The same receipt binds exact canonical arguments and the live attempt.
        (
            serde_json::to_string_pretty(&params.tool_arguments).ok()?,
            if request.tool.starts_with("mcp_") {
                params.resolved_path.clone().unwrap_or_default()
            } else {
                params.working_directory.clone().unwrap_or_default()
            },
            String::new(),
        )
    };
    let proposal = ShellApprovalProposal {
        tool: (!is_shell).then_some(request.tool.clone()),
        command,
        working_directory: cwd,
        shell,
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
