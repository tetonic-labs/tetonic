# SAE-301: Organization & Squad Runtime Structures

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 3 — Standing Fleet, Multi-Dimensional Intent & Steering  
**Layer:** `engine/core/tetonic-domain` & `engine/mantle/tetonic-orchestrator`  
**Status:** Complete

---

## 1. Context & Objective
Standing agents do not operate in a flat unstructured vacuum. They belong to **Organizations** (tenancy boundaries with global policy, budget tokens, and shared commons) and **Squads** (collaborative groups with shared mission objectives, shared memory pads, and team charters).

## 2. Requirements
1. Add `OrgId` and `SquadId` strongly typed identifiers to `tetonic-domain::ids`.
2. Define `BudgetQuota` specifying spending limits (max tokens, max concurrent active agents).
3. Implement `Organization`:
   * Tenancy boundary with unique `OrgId` and display name.
   * Enforces global `BudgetQuota` and tracks aggregate consumption.
   * Owns child `Squad` instances.
4. Implement `Squad`:
   * High-bandwidth collaborative unit grouping multiple agents (`Vec<AgentId>`).
   * Hosts a `SharedWorkpad` (in-memory bulletin board / scratchpad for peer synchronization).
   * Binds to a shared `IntentCharter`.

## 3. Acceptance Criteria
- [x] Organizations enforce token quotas and reject agent additions when limits are exceeded.
- [x] Squad members can publish and read peer bulletins on the `SharedWorkpad`.
