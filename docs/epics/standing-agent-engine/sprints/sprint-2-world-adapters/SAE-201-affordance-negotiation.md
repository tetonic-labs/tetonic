# SAE-201: World Manifest & Dynamic Affordance Negotiation

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 2 — World Adapters & Actuator Safety Gates  
**Layer:** `engine/core/tetonic-domain` & `engine/core/tetonic-runtime`  
**Status:** Complete

---

## 1. Context & Objective
Agents should not have hardcoded assumptions about what actions exist in a given environment. When an agent docks into a `WorldAdapter`, the world must authoritatively advertise its affordances (verbs, parameter schemas, and environmental constraints) via a `WorldManifest`. The agent dynamically adopts these affordances for its cognitive loop.

## 2. Requirements
1. Define `WorldManifest` and `Affordance` types in `tetonic-domain`:
   * `action_kind`: String (e.g. `"scale_replicas"`, `"move_to"`, `"create_pr"`).
   * `description`: Human and LLM readable explanation.
   * `parameters_schema`: JSON schema specifying accepted arguments.
   * `is_durative`: Boolean (instant vs. continuous action with progress tracking).
2. Add `manifest(&self) -> WorldManifest` to `WorldAdapter` trait.
3. Automatically translate advertised affordances into available tool/action schemas in `Agent::run_continuous`.

## 3. Acceptance Criteria
- [x] Agents dynamically expose the exact action verbs advertised by the target World Adapter.
- [x] Validates proposed actions against the world's parameter schema before submission.
