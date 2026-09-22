use std::collections::VecDeque;
use std::sync::Mutex;

use sha2::{Digest, Sha256};
use tetonic_domain::artifact::{ArtifactDeclaration, ArtifactKind, ArtifactStore, RetentionPolicy};
use tetonic_domain::ids::AttemptId;

use crate::types::{ContextPack, ContextRequest};
use tetonic_domain::workspace::ContentDigest;

/// A cache entry keyed by all policy/version inputs that affect context generation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    /// Fingerprint of the workspace state (git head + dirty digest)
    workspace_fingerprint: String,
    /// Includes scope, ownership, full workspace state, retrieval parameters,
    /// artifacts and budget. Conservative misses are preferable to scope reuse.
    request_digest: String,
}

impl CacheKey {
    pub fn from_request(request: &ContextRequest) -> Option<Self> {
        let bytes = serde_json::to_vec(request).ok()?;
        Some(Self {
            workspace_fingerprint: request.workspace_version.state_fingerprint(),
            request_digest: format!("{:x}", Sha256::digest(bytes)),
        })
    }
}

pub struct ContextCache {
    entries: Mutex<VecDeque<(CacheKey, ContextPack)>>,
    max_entries: usize,
}

impl Default for ContextCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextCache {
    pub fn new() -> Self {
        Self::with_capacity(64)
    }

    /// Zero disables caching. Eviction only affects recomputation, not content.
    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            max_entries,
        }
    }

    /// Look up a cached pack. Returns `None` if absent or if the workspace/policy key
    /// does not match — **never** returns a pack for an incompatible workspace version.
    pub fn get(&self, request: &ContextRequest) -> Option<ContextPack> {
        let key = CacheKey::from_request(request)?;
        let mut guard = self.entries.lock().ok()?;
        let index = guard.iter().position(|(cached, _)| cached == &key)?;
        let entry = guard.remove(index)?;
        let result = entry.1.clone();
        guard.push_back(entry);
        Some(result)
    }

    /// Store a sealed pack under its fully-qualified cache key.
    pub fn insert(&self, request: &ContextRequest, pack: &ContextPack) {
        let Some(key) = CacheKey::from_request(request) else {
            return;
        };
        if self.max_entries == 0 {
            return;
        }
        let Ok(mut guard) = self.entries.lock() else {
            return;
        };
        guard.retain(|(cached, _)| cached != &key);
        while guard.len() >= self.max_entries {
            guard.pop_front();
        }
        guard.push_back((key, pack.clone()));
    }

    /// Invalidate all entries for a given workspace fingerprint.
    /// Called whenever workspace state changes.
    pub fn invalidate_workspace(&self, fingerprint: &str) {
        let Ok(mut guard) = self.entries.lock() else {
            return;
        };
        guard.retain(|(k, _)| k.workspace_fingerprint != fingerprint);
    }

    /// Flush all entries — used when a policy version (tokenizer, exclusions) changes.
    pub fn invalidate_all(&self) {
        let Ok(mut guard) = self.entries.lock() else {
            return;
        };
        guard.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.lock().map(|g| g.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Persist a sealed `ContextPack` to the artifact store under M3-3 rules.
pub async fn persist_pack(
    pack: &ContextPack,
    request: &ContextRequest,
    store: &dyn ArtifactStore,
) -> Result<ContentDigest, String> {
    let bytes = serde_json::to_vec(pack).map_err(|e| format!("serialize context pack: {e}"))?;

    let declaration = ArtifactDeclaration {
        kind: ArtifactKind::ContextPack,
        producer_run_id: request.run_id.clone(),
        producer_task_id: request.task_id.clone(),
        // Context packs aren't tied to a specific attempt; use a synthetic id.
        producer_attempt_id: AttemptId::new(format!("ctx_{}", pack.context_pack_id)),
        worker_id: None,
        // Pass workspace_version as None — the pack itself carries full provenance.
        workspace_version: None,
        data_class: request.data_class_ceiling,
        retention_policy: RetentionPolicy::UntilRunCompletes,
    };

    let mut writer = store
        .begin_write(declaration)
        .await
        .map_err(|e| format!("begin_write: {e}"))?;

    writer
        .write_chunk(&bytes)
        .await
        .map_err(|e| format!("write_chunk: {e}"))?;

    let meta = writer
        .seal()
        .await
        .map_err(|e| format!("seal artifact: {e}"))?;

    Ok(meta.content_digest)
}
