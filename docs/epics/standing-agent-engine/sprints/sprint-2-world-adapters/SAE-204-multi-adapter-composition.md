# SAE-204: Multi-Adapter Composition & Environment Routing

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 2 — World Adapters & Actuator Safety Gates  
**Layer:** `engine/core/tetonic-domain` & `engine/core/tetonic-core`  
**Status:** Complete

---

## 1. Context & Objective
Real-world missions often require an agent or squad to touch multiple environments simultaneously (e.g. reading telemetry from a metrics stream, editing code in a repository, and reporting status to a Slack channel). An agent cannot be locked into a single adapter.

## 2. Requirements
1. Implement `CompositeWorldAdapter` in `tetonic-domain` or `tetonic-runtime`:
   * Combines multiple named child adapters (e.g. `adapter["code"]`, `adapter["metrics"]`, `adapter["comms"]`).
   * Multiplexes incoming `Perception` streams into a unified stream, tagging each event with its source adapter.
   * Demultiplexes outgoing `WorldAction` packets, routing each action to its target adapter based on prefix or namespace (`code.checkout_branch`, `metrics.adjust_threshold`).
2. Aggregate affordance manifests from all child adapters into a combined manifest.

## 3. Acceptance Criteria
- [x] Agent receives interleaved perceptions from two distinct mock adapters.
- [x] Outgoing actions are accurately routed to the corresponding child adapter.
