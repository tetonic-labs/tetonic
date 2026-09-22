//! Artifact garbage collection and storage quota (M3-3 / R4-2).

use std::collections::HashSet;

use tetonic_domain::artifact::{ArtifactError, ArtifactMetadata, RetentionPolicy};

use crate::store::LocalArtifactStore;

/// Evidence supplied by the lifecycle owner, never inferred from missing references.
///
/// A completed run must be terminal with no outstanding recovery/finalization work.
/// The caller must also protect references from other runs and serialize this
/// authorization with lifecycle changes for the duration of collection. An empty
/// or unavailable inventory authorizes no sealed-artifact deletion.
#[derive(Debug, Default, Clone)]
pub struct ArtifactGcRoots {
    pub completed_run_ids: HashSet<String>,
    pub active_artifact_ids: HashSet<String>,
}

/// Retention and explicit lifecycle evidence must both permit deletion.
pub fn is_eligible_for_gc(meta: &ArtifactMetadata, roots: &ArtifactGcRoots) -> bool {
    if roots.active_artifact_ids.contains(&meta.artifact_id.0)
        || !roots.completed_run_ids.contains(&meta.producer_run_id.0)
    {
        return false;
    }

    match meta.retention_policy {
        RetentionPolicy::Ephemeral => true,
        // Missing from an active-reference set is not proof of run completion.
        RetentionPolicy::UntilRunCompletes => true,
        RetentionPolicy::ProjectHistory => false,
        RetentionPolicy::SecurityAudit => false,
        RetentionPolicy::UserPinned => false,
    }
}

#[derive(Debug, Clone)]
pub struct ArtifactGcConfig {
    /// Soft/hard ceiling for sealed object bytes under the store root.
    pub max_total_bytes: u64,
}

impl Default for ArtifactGcConfig {
    fn default() -> Self {
        Self {
            // 1 GiB default workspace artifact budget (R4-2).
            max_total_bytes: 1024 * 1024 * 1024,
        }
    }
}

impl ArtifactGcConfig {
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(raw) = std::env::var("LOKAI_ARTIFACT_QUOTA_BYTES") {
            if let Ok(n) = raw.parse::<u64>() {
                if n > 0 {
                    cfg.max_total_bytes = n;
                }
            }
        }
        cfg
    }
}

#[derive(Debug, Default, Clone)]
pub struct GcReport {
    pub deleted: Vec<String>,
    pub cleaned_tmp: usize,
    pub usage_before: u64,
    pub usage_after: u64,
}

/// Production GC entry point: abandon stranded writes, delete eligible sealed
/// artifacts, then report usage against quota.
pub fn collect_garbage(
    store: &LocalArtifactStore,
    roots: &ArtifactGcRoots,
    config: &ArtifactGcConfig,
) -> Result<GcReport, ArtifactError> {
    let _ = config;
    let _lock = store.mutation_lock()?;
    let usage_before = store.usage_bytes()?;
    let cleaned_tmp = store.clean_abandoned_tmp_locked()?;
    // Replay only durable deletion requests, never infer orphan eligibility from
    // an absent lifecycle inventory.
    let mut deleted = store.recover_deletions_locked()?;

    let mut metas = store.list_metadata()?;
    metas.sort_by_key(|m| m.created_at);

    for meta in metas {
        let id = meta.artifact_id.0.clone();
        if is_eligible_for_gc(&meta, roots) {
            store.delete_sync_locked(&meta.artifact_id)?;
            deleted.push(id);
        }
    }

    let usage_after = store.usage_bytes()?;
    Ok(GcReport {
        deleted,
        cleaned_tmp,
        usage_before,
        usage_after,
    })
}

/// Ensure a prospective write of `incoming_bytes` fits under quota.
/// Does not run GC (seal must not delete siblings or wipe concurrent `tmp/` files).
/// This is a check, not a reservation; store writers hold the mutation lock from
/// this check through publication.
pub fn ensure_quota_for_write(
    store: &LocalArtifactStore,
    _active_artifact_ids: &HashSet<String>,
    config: &ArtifactGcConfig,
    incoming_bytes: u64,
) -> Result<(), ArtifactError> {
    let usage = store.usage_bytes()?;
    if usage.saturating_add(incoming_bytes) > config.max_total_bytes {
        return Err(ArtifactError::QuotaExceeded {
            usage,
            max: config.max_total_bytes,
        });
    }
    Ok(())
}

/// Clean abandoned writes and log quota pressure without deleting sealed artifacts.
/// Startup alone provides no proof that persisted runs no longer need their data.
/// Lifecycle owners can call `collect_garbage` with explicit roots separately.
pub fn enforce_at_startup(
    store: &LocalArtifactStore,
    config: &ArtifactGcConfig,
) -> Result<GcReport, ArtifactError> {
    let report = collect_garbage(store, &ArtifactGcRoots::default(), config)?;
    if report.usage_after > config.max_total_bytes {
        tracing::warn!(
            usage = report.usage_after,
            max = config.max_total_bytes,
            "artifact store over quota after GC; new seals will fail until space is freed"
        );
    } else if report.cleaned_tmp > 0 || !report.deleted.is_empty() {
        tracing::info!(
            deleted = report.deleted.len(),
            cleaned_tmp = report.cleaned_tmp,
            usage = report.usage_after,
            "artifact GC completed at startup"
        );
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tetonic_domain::artifact::{
        ArtifactKind, ArtifactLocation, ArtifactState, VerificationState,
    };
    use tetonic_domain::classify::DataClass;
    use tetonic_domain::ids::{ArtifactId, AttemptId, RunId, TaskId};
    use tetonic_domain::workspace::ContentDigest;

    fn test_metadata(policy: RetentionPolicy) -> ArtifactMetadata {
        ArtifactMetadata {
            artifact_id: ArtifactId::new("art_1"),
            kind: ArtifactKind::FileSnapshot,
            schema_version: 1,
            producer_run_id: RunId::new("run_1"),
            producer_task_id: TaskId::new("task_1"),
            producer_attempt_id: AttemptId::new("attempt_1"),
            worker_id: None,
            content_digest: ContentDigest::new("d1"),
            size_bytes: 100,
            workspace_version: None,
            data_class: DataClass::Secret,
            storage_location: ArtifactLocation::Remote("http://test".into()),
            lifecycle_state: ArtifactState::Sealed,
            verification_state: VerificationState::Unverified,
            origin: tetonic_domain::ArtifactOrigin::Local,
            created_at: Utc::now(),
            retention_policy: policy,
        }
    }

    #[test]
    fn gc_requires_positive_completion_and_no_active_reference() {
        let meta = test_metadata(RetentionPolicy::Ephemeral);
        let mut roots = ArtifactGcRoots::default();
        assert!(!is_eligible_for_gc(&meta, &roots));
        roots.completed_run_ids.insert("run_1".into());
        assert!(is_eligible_for_gc(&meta, &roots));
        roots.active_artifact_ids.insert("art_1".into());
        assert!(!is_eligible_for_gc(&meta, &roots));
    }

    #[test]
    fn retained_policies_survive_completed_runs() {
        let roots = ArtifactGcRoots {
            completed_run_ids: HashSet::from(["run_1".into()]),
            ..Default::default()
        };
        for policy in [
            RetentionPolicy::ProjectHistory,
            RetentionPolicy::SecurityAudit,
            RetentionPolicy::UserPinned,
        ] {
            assert!(!is_eligible_for_gc(&test_metadata(policy), &roots));
        }
        assert!(is_eligible_for_gc(
            &test_metadata(RetentionPolicy::UntilRunCompletes),
            &roots
        ));
    }
}
