# SAE-402: Node Roles: Standalone, Coordinator (Keeper), and Runner

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 4 — Engine Configuration & Decoupled Inference  
**Layer:** `engine/mantle/tetonic-node`  
**Status:** Ready

---

## 1. Context & Objective
In distributed deployments, nodes assume distinct roles:
* **Standalone:** Embedded coordinator, local runner, and local SQLite engine in a single binary.
* **Coordinator ("The Keeper"):** Manages metadata, lease proofs, agent location registry, cluster topology, health heartbeats, and cluster failover.
* **Runner:** Executes continuous agent cognitive loops, holds transient workpads/volumes, and streams heartbeats to the coordinator.

## 2. Requirements
1. Define `NodeRole` enum (`Standalone`, `Coordinator`, `Runner`) with capabilities matrix.
2. Implement `NodeLifecycle`:
   * Coordinates initialization based on role.
   * If `Coordinator`: starts metadata registry and heartbeat auditor.
   * If `Runner`: initiates registration handshake with Coordinator endpoint and begins streaming periodic heartbeats.
3. Handle graceful runner deregistration and cluster failover triggers.

## 3. Acceptance Criteria
- [ ] Nodes initialize according to configured role.
- [ ] Runners complete registration handshake with coordinator and maintain active lease proofs.
