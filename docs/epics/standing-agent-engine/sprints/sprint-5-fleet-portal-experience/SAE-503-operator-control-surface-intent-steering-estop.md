# SAE-503: Operator Control Surface: Intent Formulation, Steering & E-Stop

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 5 — Fleet Portal & Human Experience  
**Layer:** `engine/litho/tetonic-app` & `engine/mantle/tetonic-orchestrator`  
**Status:** Complete

---

## 1. Context & Objective
Operators interface with the aimed fleet across three primary modes:
1. **Intent Formulation:** Collaborative shaping of multi-dimensional goals and boundaries before launch.
2. **In-Flight Course Correction:** Real-time injection of `SteeringVector` signals when drift is observed.
3. **Authoritative Intervention (E-Stop):** 1-click immediate physical freeze and mutation abort across an agent, squad, or entire organization.

## 2. Requirements
1. Implement Operator Interaction Endpoints in `tetonic-app`:
   * `POST /api/v1/agents/:agent_id/steer`: Injects high-urgency steering vector with dynamic boundary adjustment.
   * `POST /api/v1/agents/:agent_id/estop`: Authoritative agent freeze, tripping `EstopSwitch` and aborting staged mutations.
   * `POST /api/v1/squads/:squad_id/estop`: Squad-wide coordinated E-Stop.
   * `POST /api/v1/orgs/:org_id/estop`: Organization-wide fleet killswitch.
   * `POST /api/v1/agents/:agent_id/resume`: Reauthorizes an estopped agent once cleared.
2. Web / TUI Operator Dashboard:
   * Real-time squad cards with status lights (`Running`, `Paused`, `Estopped`).
   * One-click E-Stop button with immediate visual confirmation.
   * In-flight steering input modal.

## 3. Acceptance Criteria
- [x] Steering endpoint immediately impacts agent sensory priority on next cognitive tick.
- [x] E-Stop endpoints instantly trip actuator interlocks across all target agents.
- [x] Resume endpoint safely clears emergency stop state after operator verification.
