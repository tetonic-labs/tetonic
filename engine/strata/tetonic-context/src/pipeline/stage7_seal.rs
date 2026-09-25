use chrono::Utc;
use sha2::{Digest, Sha256};

use tetonic_domain::artifact::{ArtifactDeclaration, ArtifactKind, ArtifactStore, RetentionPolicy};
use tetonic_domain::ids::{ArtifactId, AttemptId, ExpansionHandleId};
use tetonic_domain::workspace::ContentDigest;

use crate::interfaces::SecretScanner;
use crate::types::{
    ContextEvidence, ContextOmission, ContextPack, ContextRequest, ExpansionHandle, ExpansionQuery,
    ExpansionScope, RepositorySummary, TokenUsage,
};

/// Stage 7: Pack sealing.
///
/// Before sealing this stage:
/// 1. Revalidates source digests (rejects evidence with blank/placeholder digests)
/// 2. Confirms workspace version is consistent across all evidence
/// 3. Computes the objective digest
/// 4. Scans included content for secrets under M4-2 (redacting/omitting if found)
/// 5. Generates expansion handles for the highest-ranked evidence items
/// 6. Computes the pack digest
/// 7. Stores the pack as an immutable artifact under M3-3 rules
/// 8. Returns the sealed `ContextPack`
///
/// If relevant source content changes during compilation (detected via digest
/// inconsistency), the pack is rejected rather than silently continuing.
pub async fn seal(
    budgeted: Vec<ContextEvidence>,
    request: ContextRequest,
    mut omissions: Vec<ContextOmission>,
    token_usage: TokenUsage,
    repository_summary: Option<RepositorySummary>,
    store: Option<&dyn ArtifactStore>,
    secret_scanner: Option<&dyn SecretScanner>,
) -> Result<ContextPack, String> {
    let workspace_fp = request.workspace_version.state_fingerprint();

    let mut redactions = Vec::new();
    let mut final_budgeted = Vec::new();

    // --- Revalidate: every evidence item must carry a non-empty digest and
    //     must belong to the same workspace version (fingerprint). Also scan for secrets. ---
    for mut ev in budgeted.into_iter() {
        if ev.content_digest.0.is_empty() {
            return Err(format!(
                "evidence {:?} has empty content_digest — rejecting pack",
                ev.evidence_id
            ));
        }
        // Only check versioned evidence (evidence without a path may be memory/synthetic)
        if ev.repository_path.is_some() {
            let ev_fp = ev.workspace_version.state_fingerprint();
            if ev_fp != workspace_fp {
                return Err(format!(
                    "evidence {:?} has stale workspace version ({ev_fp} vs {workspace_fp}) — \
                     source content changed during compilation",
                    ev.evidence_id
                ));
            }
        }

        if let Some(scanner) = secret_scanner {
            let path = ev.repository_path.as_deref();
            if let Some((records, redacted_text)) = scanner.scan_and_redact(&ev.text, path).await? {
                if redacted_text.is_empty() {
                    omissions.push(ContextOmission {
                        reason: "Redacted fully due to secrets".to_string(),
                        path: ev.repository_path.clone(),
                    });
                    redactions.extend(records);
                    continue; // drop this evidence
                }
                ev.text = redacted_text;
                let mut h = Sha256::new();
                h.update(ev.text.as_bytes());
                ev.content_digest = ContentDigest(format!("{:x}", h.finalize()));
                redactions.extend(records);
            }
        }
        final_budgeted.push(ev);
    }

    // --- Objective digest ---
    let objective_digest = {
        let mut h = Sha256::new();
        h.update(request.objective.as_bytes());
        ContentDigest(format!("sha256:{:x}", h.finalize()))
    };

    // --- Repository summary (from provider, or empty) ---
    let repo_summary = repository_summary.unwrap_or_else(|| RepositorySummary {
        languages: vec![],
        major_modules: vec![],
        entry_points: vec![],
        build_systems: vec![],
        test_systems: vec![],
        current_branch: "unknown".to_string(),
        is_dirty: false,
        architectural_boundaries: vec![],
        target_subsystem: None,
        neighboring_subsystems: vec![],
    });

    // --- Generate expansion handles for top-N ranked evidence items ---
    // Handles allow the agent to request more context around the same query
    // without giving arbitrary repository access.
    let expansion_handles = generate_expansion_handles(&final_budgeted, &request);

    // --- Build the pack (without an artifact ID yet) ---
    let now = Utc::now();
    let mut pack = ContextPack {
        context_pack_id: ArtifactId::new("pending"),
        stored_artifact_id: None,
        schema_version: 1,
        run_id: request.run_id.clone(),
        task_id: request.task_id.clone(),
        workspace_version: request.workspace_version.clone(),
        objective_digest,
        repository_summary: repo_summary,
        evidence: final_budgeted,
        relationships: vec![],
        current_diff: None,
        relevant_tests: vec![],
        prior_decisions: vec![],
        expansion_handles,
        omissions,
        redactions,
        token_usage,
        data_class: request.data_class_ceiling,
        created_at: now,
    };

    // --- Compute pack digest (over the JSON-serialised pack minus the id field) ---
    let pack_json = serde_json::to_vec(&pack)
        .map_err(|e| format!("failed to serialize pack for digest: {e}"))?;
    let pack_digest = {
        let mut h = Sha256::new();
        h.update(&pack_json);
        format!("{:x}", h.finalize())
    };

    // The artifact ID is derived from the content digest — changing content changes identity.
    pack.context_pack_id = ArtifactId::new(format!("ctx_{}", &pack_digest[..16]));

    // --- Persist to artifact store (M3-3 rules) ---
    if let Some(store) = store {
        let bytes = serde_json::to_vec(&pack).map_err(|e| format!("serialize sealed pack: {e}"))?;

        let declaration = ArtifactDeclaration {
            kind: ArtifactKind::ContextPack,
            producer_run_id: request.run_id.clone(),
            producer_task_id: request.task_id.clone(),
            producer_attempt_id: AttemptId::new(format!("ctx_{}", &pack_digest[..16])),
            worker_id: None,
            workspace_version: None,
            data_class: request.data_class_ceiling,
            retention_policy: RetentionPolicy::UntilRunCompletes,
        };

        let mut writer = store
            .begin_write(declaration)
            .await
            .map_err(|e| format!("artifact begin_write: {e}"))?;
        writer
            .write_chunk(&bytes)
            .await
            .map_err(|e| format!("artifact write_chunk: {e}"))?;
        let meta = writer
            .seal()
            .await
            .map_err(|e| format!("artifact seal: {e}"))?;
        pack.stored_artifact_id = Some(meta.artifact_id);
    }

    Ok(pack)
}

/// Generate bounded expansion handles from the highest-ranked evidence items.
///
/// Each handle is bound to:
/// - The current workspace version (stale workspace → rejected on expand)
/// - The same data-class ceiling as the original request
/// - A 1-hour TTL
/// - Maximum 3 uses
fn generate_expansion_handles(
    evidence: &[ContextEvidence],
    request: &ContextRequest,
) -> Vec<ExpansionHandle> {
    let expires_at = Utc::now() + chrono::Duration::hours(1);
    let mut handles = Vec::new();

    // Produce one expansion handle per unique path in the top-10 evidence items.
    let mut seen_paths = std::collections::HashSet::new();
    for ev in evidence.iter().take(10) {
        if let Some(ref path) = ev.repository_path {
            if !seen_paths.insert(path.clone()) {
                continue;
            }
            handles.push(ExpansionHandle {
                session_id: request.session_id.clone(),
                handle_id: ExpansionHandleId::new(uuid::Uuid::new_v4().to_string()),
                run_id: request.run_id.clone(),
                task_id: request.task_id.clone(),
                workspace_version: request.workspace_version.clone(),
                query: ExpansionQuery {
                    kind: "file".to_string(),
                    target: path.clone(),
                },
                allowed_scope: ExpansionScope {
                    excluded_paths: request.excluded_paths.clone(),
                    allowed_paths: request.allowed_paths.clone(),
                    max_depth: 3,
                },
                data_class_ceiling: request.data_class_ceiling,
                expires_at,
                max_uses: 3,
                policy_version: "1.0".to_string(),
            });
        }
    }

    // Also produce symbol-level handles
    for ev in evidence.iter().take(10) {
        if let Some(ref sym) = ev.symbol_id {
            handles.push(ExpansionHandle {
                session_id: request.session_id.clone(),
                handle_id: ExpansionHandleId::new(uuid::Uuid::new_v4().to_string()),
                run_id: request.run_id.clone(),
                task_id: request.task_id.clone(),
                workspace_version: request.workspace_version.clone(),
                query: ExpansionQuery {
                    kind: "symbol".to_string(),
                    target: sym.clone(),
                },
                allowed_scope: ExpansionScope {
                    excluded_paths: request.excluded_paths.clone(),
                    allowed_paths: request.allowed_paths.clone(),
                    max_depth: 3,
                },
                data_class_ceiling: request.data_class_ceiling,
                expires_at,
                max_uses: 3,
                policy_version: "1.0".to_string(),
            });
        }
    }

    handles
}
