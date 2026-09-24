//! Checkpoint Manager for Moveable Execution Volumes (SAE-404).
//!
//! Provides atomic snapshot persistence and data integrity verification for cloud PVCs.
//! When a runner node is rescheduled or preempted, another node restores the agent
//! in milliseconds from the latest valid uncorrupted checkpoint.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use tracing::{debug, warn};

use tetonic_domain::checkpoint::{AgentStateCheckpoint, AgentStateCheckpointHeader, CheckpointError};
use tetonic_domain::ids::AgentId;

/// Manages atomic checkpoint persistence and verification on a moveable execution volume.
#[derive(Debug, Clone)]
pub struct CheckpointManager {
    volume_mount_path: PathBuf,
}

impl CheckpointManager {
    /// Creates a new CheckpointManager rooted at the designated volume mount path.
    pub fn new(volume_mount_path: impl Into<PathBuf>) -> Self {
        Self {
            volume_mount_path: volume_mount_path.into(),
        }
    }

    /// Root directory where checkpoints are stored.
    pub fn volume_path(&self) -> &Path {
        &self.volume_mount_path
    }

    /// Saves a checkpoint atomically using write-to-temp and atomic rename.
    pub fn save_checkpoint(
        &self,
        checkpoint: &AgentStateCheckpoint,
    ) -> Result<PathBuf, CheckpointError> {
        fs::create_dir_all(&self.volume_mount_path)?;

        let filename = format!(
            "agent-{}-{}.chk.json",
            sanitize_id(&checkpoint.agent_id.0),
            sanitize_id(&checkpoint.checkpoint_id)
        );
        let final_path = self.volume_mount_path.join(&filename);
        let temp_path = self.volume_mount_path.join(format!("{}.tmp", &filename));

        let serialized = serde_json::to_string_pretty(checkpoint)?;

        // Write to temp file and sync to disk
        {
            let mut file = File::create(&temp_path)?;
            file.write_all(serialized.as_bytes())?;
            file.sync_all()?;
        }

        // Atomic rename to final target path
        fs::rename(&temp_path, &final_path)?;

        debug!(
            agent_id = %checkpoint.agent_id.0,
            checkpoint_id = %checkpoint.checkpoint_id,
            path = %final_path.display(),
            "Persisted atomic state checkpoint"
        );

        Ok(final_path)
    }

    /// Loads the latest valid checkpoint for an agent, with automatic resilient fallback
    /// to prior valid snapshots if the most recent file was corrupted during preemption.
    pub fn load_latest_checkpoint(
        &self,
        agent_id: &AgentId,
    ) -> Result<Option<AgentStateCheckpoint>, CheckpointError> {
        if !self.volume_mount_path.exists() {
            return Ok(None);
        }

        let prefix = format!("agent-{}-", sanitize_id(&agent_id.0));
        let mut candidate_paths = Vec::new();

        for entry in fs::read_dir(&self.volume_mount_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with(&prefix) && name.ends_with(".chk.json") {
                        candidate_paths.push(path);
                    }
                }
            }
        }

        // Sort descending by filename / checkpoint sequence
        candidate_paths.sort_by(|a, b| b.cmp(a));

        for path in candidate_paths {
            match self.read_and_verify_checkpoint(&path) {
                Ok(chk) => {
                    debug!(
                        agent_id = %agent_id.0,
                        checkpoint_id = %chk.checkpoint_id,
                        path = %path.display(),
                        "Successfully rehydrated agent from valid checkpoint"
                    );
                    return Ok(Some(chk));
                }
                Err(err) => {
                    warn!(
                        agent_id = %agent_id.0,
                        path = %path.display(),
                        error = %err,
                        "Checkpoint corruption detected; falling back to previous snapshot"
                    );
                }
            }
        }

        Ok(None)
    }

    /// Reads a checkpoint file from disk and validates its data integrity checksum.
    fn read_and_verify_checkpoint(&self, path: &Path) -> Result<AgentStateCheckpoint, CheckpointError> {
        let content = fs::read_to_string(path)?;
        let checkpoint: AgentStateCheckpoint = serde_json::from_str(&content)?;
        checkpoint.verify_integrity()?;
        Ok(checkpoint)
    }

    /// Lists metadata headers for all checkpoints belonging to an agent.
    pub fn list_checkpoints(
        &self,
        agent_id: &AgentId,
    ) -> Result<Vec<AgentStateCheckpointHeader>, CheckpointError> {
        if !self.volume_mount_path.exists() {
            return Ok(Vec::new());
        }

        let prefix = format!("agent-{}-", sanitize_id(&agent_id.0));
        let mut headers = Vec::new();

        for entry in fs::read_dir(&self.volume_mount_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with(&prefix) && name.ends_with(".chk.json") {
                        if let Ok(chk) = self.read_and_verify_checkpoint(&path) {
                            headers.push(AgentStateCheckpointHeader {
                                checkpoint_id: chk.checkpoint_id,
                                agent_id: chk.agent_id,
                                squad_id: chk.squad_id,
                                created_at: chk.created_at,
                                checksum: chk.checksum,
                            });
                        }
                    }
                }
            }
        }

        headers.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(headers)
    }

    /// Prunes older checkpoints for an agent, retaining only `keep_last` newest snapshots.
    pub fn prune_checkpoints(
        &self,
        agent_id: &AgentId,
        keep_last: usize,
    ) -> Result<usize, CheckpointError> {
        if !self.volume_mount_path.exists() {
            return Ok(0);
        }

        let prefix = format!("agent-{}-", sanitize_id(&agent_id.0));
        let mut paths = Vec::new();

        for entry in fs::read_dir(&self.volume_mount_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with(&prefix) && name.ends_with(".chk.json") {
                        paths.push(path);
                    }
                }
            }
        }

        paths.sort_by(|a, b| b.cmp(a));

        let mut pruned = 0;
        if paths.len() > keep_last {
            for old_path in &paths[keep_last..] {
                if fs::remove_file(old_path).is_ok() {
                    pruned += 1;
                }
            }
        }

        Ok(pruned)
    }
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_atomic_checkpoint_save_and_load() {
        let dir = tempdir().expect("tempdir");
        let mgr = CheckpointManager::new(dir.path());
        let agent = AgentId("agent-404".into());

        let chk = AgentStateCheckpoint::new(
            "chk-001",
            agent.clone(),
            None,
            None,
            "digest-1",
            "{\"turns\":[1,2,3]}",
            vec!["fs".into()],
        );

        let path = mgr.save_checkpoint(&chk).expect("save failed");
        assert!(path.exists());

        let loaded = mgr
            .load_latest_checkpoint(&agent)
            .expect("load failed")
            .expect("should find checkpoint");

        assert_eq!(loaded.checkpoint_id, "chk-001");
        assert_eq!(loaded.working_memory_buffer, "{\"turns\":[1,2,3]}");
        assert_eq!(loaded.checksum, chk.checksum);
    }

    #[test]
    fn test_corrupt_checkpoint_recovery_fallback_to_prior() {
        let dir = tempdir().expect("tempdir");
        let mgr = CheckpointManager::new(dir.path());
        let agent = AgentId("agent-resilient".into());

        // 1. Save valid snapshot 1
        let chk1 = AgentStateCheckpoint::new(
            "chk-001",
            agent.clone(),
            None,
            None,
            "digest-1",
            "{\"turns\":[1]}",
            vec!["fs".into()],
        );
        mgr.save_checkpoint(&chk1).expect("save 1");

        // 2. Save valid snapshot 2
        let chk2 = AgentStateCheckpoint::new(
            "chk-002",
            agent.clone(),
            None,
            None,
            "digest-2",
            "{\"turns\":[1,2]}",
            vec!["fs".into()],
        );
        let path2 = mgr.save_checkpoint(&chk2).expect("save 2");

        // 3. Corrupt snapshot 2 by overwriting payload content (simulating crash during disk flush)
        fs::write(&path2, "{\"corrupted\":\"truncated payload").expect("corrupt write");

        // 4. Load latest must detect corruption and automatically fall back to snapshot 1!
        let recovered = mgr
            .load_latest_checkpoint(&agent)
            .expect("load")
            .expect("should find fallback");

        assert_eq!(recovered.checkpoint_id, "chk-001");
        assert_eq!(recovered.working_memory_buffer, "{\"turns\":[1]}");
    }

    #[test]
    fn test_cross_node_migration_rehydration() {
        let dir = tempdir().expect("shared volume");
        let agent = AgentId("agent-migrator".into());

        // Node A runs agent and takes checkpoint
        let node_a_mgr = CheckpointManager::new(dir.path());
        let chk = AgentStateCheckpoint::new(
            "chk-node-a-final",
            agent.clone(),
            None,
            None,
            "mem-digest-99",
            "{\"status\":\"ready_for_migration\"}",
            vec!["network".into(), "fs".into()],
        );
        node_a_mgr.save_checkpoint(&chk).expect("node A save");

        // Node B takes over volume after preemption and rehydrates
        let node_b_mgr = CheckpointManager::new(dir.path());
        let rehydrated = node_b_mgr
            .load_latest_checkpoint(&agent)
            .expect("node B load")
            .expect("checkpoint present");

        assert_eq!(rehydrated.checkpoint_id, "chk-node-a-final");
        assert_eq!(
            rehydrated.working_memory_buffer,
            "{\"status\":\"ready_for_migration\"}"
        );
        assert_eq!(rehydrated.active_world_adapters, vec!["network", "fs"]);
    }
}
