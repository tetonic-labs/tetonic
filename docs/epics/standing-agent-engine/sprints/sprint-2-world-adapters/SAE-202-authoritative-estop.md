# SAE-202: Authoritative E-Stop & Actuator Interlock

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 2 — World Adapters & Actuator Safety Gates  
**Layer:** `engine/core/tetonic-domain` & `engine/core/tetonic-runtime`  
**Status:** Ready

---

## 1. Context & Objective
Emergency intervention cannot rely on prompting an agent to stop. In high-stakes environments (cloud infra, financial transactions, robotics), the Emergency Stop (E-Stop) must act as a physical actuator kill switch built into the `WorldAdapter` layer that immediately blocks all mutations, rolls back uncommitted staging, and freezes cognitive loops for forensic inspection.

## 2. Requirements
1. Add E-Stop state to `WorldAdapter`:
   * `fn trigger_estop(&self, reason: String) -> Result<(), WorldError>`
   * `fn resume(&self) -> Result<(), WorldError>`
   * `fn is_estopped(&self) -> bool`
2. Enforce zero-leakage actuator interlock:
   * When E-Stop is active, `WorldAdapter::execute()` unconditionally drops all `WorldAction` requests with `WorldError::ActionRejected { reason: "E-Stop active" }`.
   * Automatically triggers transactional rollback of any uncommitted staged operations (`abort_staged_mutations`).
3. Freeze the agent's continuous execution loop in place without discarding working memory, allowing forensic inspection through the Portal API.

## 3. Acceptance Criteria
- [ ] Triggering E-Stop causes 100% of subsequent action proposals to be dropped at the adapter level.
- [ ] Agent state remains intact for inspection during E-Stop and resumes cleanly when `resume()` is called.
