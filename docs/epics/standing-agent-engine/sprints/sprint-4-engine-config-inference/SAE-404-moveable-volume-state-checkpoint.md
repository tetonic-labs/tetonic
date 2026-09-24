# SAE-404: Moveable Execution Volume & State Checkpoint Contract

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 4 — Engine Configuration & Decoupled Inference  
**Layer:** `engine/core/tetonic-domain` & `engine/core/tetonic-core`  
**Status:** Complete

---

## 1. Context & Objective
In cloud-native Kubernetes environments, runner pods are ephemeral and can be preempted or rescheduled at any time. An agent's persistent identity and state must be decoupled from the physical runner node. By implementing a standardized `StateCheckpoint` on moveable storage volumes (cloud PVCs), any node can restore and resume an agent in milliseconds.

## 2. Requirements
1. Implement `AgentStateCheckpoint` in `tetonic-domain`:
   * `checkpoint_id`: Unique monotonic identifier.
   * `agent_id`: Target agent identity.
   * `squad_id`: Associated squad.
   * `charter_snapshot`: Working intent charter and operational boundaries.
   * `working_memory_digest`: Digest and serialized working memory buffer.
   * `active_world_adapters`: List of bound world adapter descriptors.
   * `created_at`: Timestamp.
2. Implement `CheckpointManager`:
   * Writes atomic checkpoint files to `volume_mount_path` (write to temp + atomic rename).
   * Restores latest valid checkpoint upon node recovery or rescheduling.
   * Verifies data integrity via content digest before restoration.

## 3. Acceptance Criteria
- [x] Atomic checkpoints write cleanly to the designated volume mount path.
- [x] Corrupted checkpoints are detected and fallback to the latest valid prior snapshot.
- [x] Agent state restores identically on a different node instance.
