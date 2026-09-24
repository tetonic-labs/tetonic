# SAE-302: Composite Intent Charter & Multi-Target Boundaries

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 3 — Standing Fleet, Multi-Dimensional Intent & Steering  
**Layer:** `engine/core/tetonic-domain` & `engine/mantle/tetonic-orchestrator`  
**Status:** Complete

---

## 1. Context & Objective
Intent is not a single prompt string or a single pointed repo. Operating a persistent autonomous fleet requires multi-dimensional intent encompassing strategic objectives, operational boundaries, regulatory invariants, and multiple target world bindings.

## 2. Requirements
1. Implement `IntentCharter` in `tetonic-domain`:
   * `strategic_intent`: Natural language mission statement and overarching objectives.
   * `operational_boundaries`: Array of `OperationalBoundary` rules (e.g. `PathFilter`, `ForbiddenAction`, `NamespaceAllowlist`, `ResourceLimit`).
   * `invariants`: Non-negotiable physical and safety invariants.
   * `target_worlds`: List of environment names or URIs the squad is aimed at.
2. Implement validation:
   * `charter.evaluate_action(world_action)` authoritatively checks action proposals against operational boundaries and invariants.
3. Implement context synthesis:
   * `charter.render_prompt_context()` generates concise, structured guidance for brain deliberation.

## 3. Acceptance Criteria
- [x] Actions violating an operational boundary (e.g. forbidden namespace or action kind) are authoritatively rejected.
- [x] Multi-target boundaries correctly constrain actions aimed at different child adapters.
