//! Test-only remote patch helper. Not a production fabric commit door.

use lokai_domain::workspace::{ContentDigest, PatchApproval, WorkspaceVersion};
use lokai_domain::{DataClass, TransactionId, VerificationRecord};
use lokai_fabric_protocol::ResultEnvelope;
use lokai_transaction::{SecurityLimits, WorkspaceTransactionService, WorkspaceTxnConfig};
use sha2::{Digest, Sha256};

use lokai_fabric_client::validate_patch_acceptance;
use lokai_fabric_client::FabricClientError;

#[derive(Clone, Debug)]
pub struct RemotePatchApplyRequest {
    pub envelope: ResultEnvelope,
    pub touched_paths: Vec<(String, Vec<u8>)>,
    pub current_workspace: WorkspaceVersion,
    pub authorization_granted: bool,
    pub owner: String,
    pub data_class: DataClass,
}

#[derive(Clone, Debug)]
pub struct RemotePatchApplyResult {
    pub transaction_id: TransactionId,
    pub patch_digest: ContentDigest,
    pub committed: bool,
}

/// Test helper. Remote bytes never mutate the workspace until envelope
/// validation, authorization, staged verification, and commit succeed.
pub fn apply_verified_remote_patch(
    workspace_root: &std::path::Path,
    req: &RemotePatchApplyRequest,
) -> Result<RemotePatchApplyResult, FabricClientError> {
    if !req.authorization_granted {
        return Err(FabricClientError::Http(
            "remote patch requires exact local authorization".into(),
        ));
    }
    if req.envelope.body.workspace_version.as_ref() != Some(&req.current_workspace) {
        return Err(FabricClientError::Http(
            "workspace version mismatch; remote patch rejected".into(),
        ));
    }

    for (path, bytes) in &req.touched_paths {
        if !lokai_artifact::is_safe_workspace_path(path) {
            return Err(FabricClientError::Http(format!(
                "unsafe patch path: {path}"
            )));
        }
        if lokai_artifact::looks_executable(bytes, Some(path)) {
            return Err(FabricClientError::Http(format!(
                "executable payload in patch path: {path}"
            )));
        }
    }

    let svc = WorkspaceTransactionService::new(
        workspace_root,
        WorkspaceTxnConfig {
            owner: req.owner.clone(),
            ..Default::default()
        },
    )
    .map_err(|e| FabricClientError::Http(e.to_string()))?;

    let mut txn = svc
        .begin()
        .map_err(|e| FabricClientError::Http(e.to_string()))?;

    for (path, bytes) in &req.touched_paths {
        let existed = workspace_root.join(path).is_file();
        txn.stage_write_bytes(path, bytes, existed)
            .map_err(|e| FabricClientError::Http(e.to_string()))?;
    }

    let preview = txn.preview();
    let patch_digest = preview.patch_digest.clone();

    let _verify_root = txn
        .begin_verification()
        .map_err(|e| FabricClientError::Http(e.to_string()))?;
    let verification = VerificationRecord {
        command: "m5_4_remote_patch_verify".into(),
        sandbox_policy: "staged_overlay".into(),
        workspace_version: req.current_workspace.clone(),
        exit_status: Some(0),
        output_digest: ContentDigest::new(format!(
            "sha256:{}",
            hex::encode(Sha256::digest(patch_digest.0.as_bytes()))
        )),
        success: true,
        unexpected_mutations: vec![],
    };
    txn.finish_verification(verification, &SecurityLimits::default())
        .map_err(|e| FabricClientError::Http(e.to_string()))?;

    let approval = PatchApproval {
        transaction_id: txn.id.clone(),
        patch_digest: patch_digest.clone(),
        base_version: req.current_workspace.clone(),
        verification_required: true,
        verification_passed: true,
    };

    validate_patch_acceptance(
        &req.envelope,
        &approval,
        &patch_digest,
        &req.current_workspace,
        true,
        true,
    )?;

    // Re-check digest immediately before bind — any staging change invalidates approval.
    let re_preview = txn.preview();
    if re_preview.patch_digest != approval.patch_digest {
        return Err(FabricClientError::Http(
            "patch changed after approval; approval invalidated".into(),
        ));
    }

    txn.bind_approval(approval)
        .map_err(|e| FabricClientError::Http(e.to_string()))?;
    let commit = txn
        .commit(&req.owner, req.data_class)
        .map_err(|e| FabricClientError::Http(e.to_string()))?;

    Ok(RemotePatchApplyResult {
        transaction_id: commit.transaction_id,
        patch_digest: commit.patch_digest,
        committed: true,
    })
}
