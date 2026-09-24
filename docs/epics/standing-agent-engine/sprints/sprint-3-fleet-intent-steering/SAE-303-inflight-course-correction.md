# SAE-303: In-Flight Course Correction & Real-Time Steering Vectors

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 3 — Standing Fleet, Multi-Dimensional Intent & Steering  
**Layer:** `engine/core/tetonic-domain` & `engine/mantle/tetonic-orchestrator`  
**Status:** Complete

---

## 1. Context & Objective
When an operator observes drift between actual fleet execution and desired outcomes, killing and redeploying agents is disruptive and loses valuable episodic context. The engine must support real-time in-flight course correction via dynamic **Steering Vectors** injected as high-urgency sensory events.

## 2. Requirements
1. Implement `SteeringVector` in `tetonic-domain`:
   * `vector_id`: String identifier for audit and tracking.
   * `urgency`: `Urgency` level (defaults to `High` or `Critical`).
   * `directive`: Operator instruction realigning current behavior.
   * `boundary_adjustments`: Dynamically tightened or loosened operational boundaries.
   * `issued_at`: Timestamp and optional expiry.
2. In-flight injection pipeline:
   * Provide `inject_steering(&self, vector: SteeringVector)` on `Squad` / `FleetSupervisor`.
   * Converts the steering vector into a high-urgency `WorldEvent` (`kind: "steering.course_correction"`).
   * Injects the event directly into the perception channel of target agents.
3. Verify cognitive realignment:
   * System 1 immediately routes the high-urgency event to trigger System 2 deliberation.
   * The agent adjusts active goals and execution trajectory without dropping working memory.

## 3. Acceptance Criteria
- [x] Steering vectors arrive as high-urgency events in running agent loops.
- [x] Operational boundary updates take effect immediately on subsequent action validations.
