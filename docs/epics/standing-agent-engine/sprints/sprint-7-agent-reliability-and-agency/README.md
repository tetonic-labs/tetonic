# Sprint 7: Agent reliability and sustained agency

Status: Planned  
Created: 2026-09-24  
Epic: Standing Agent Engine  
Companion: [Village Sprint 4](../../../../../../the-village/docs/epics/grounded-world/sprints/sprint-4-emergent-world-foundations/README.md)

## Goal

Make Tetonic a reliable, world-neutral runtime for agents that retain experience, maintain their own intentions and respond to events over time. Use The Village as a local integration experiment while keeping all game mechanics in its repository.

## Evidence motivating this sprint

The supplied local trace, `village-agent-trace (1).json`, contained 24 parsed decisions (11 idle, 6 speak, 4 navigate, 2 interact, 1 inspect) and four parse errors. The final three responses used 4082-4083 input tokens and emitted only 13-14 output tokens under a configured 4096-token context. This strongly suggests context exhaustion; provider stop-reason instrumentation is required to confirm it directly.

Source review found that `engine/mantle/tetonic-server/src/perceptive_brain.rs` concatenates four prior intents, up to 32 remembered facts and the current perception without token budgeting. It requests a 256-token completion, but the reviewed `engine/core/tetonic-runtime/src/brain.rs` SingleModelBrain path does not forward that limit and maps responses to stop/tool-use without preserving a length-stop distinction.

The current Village observation repeats per-object capabilities, and its recent experiences echo speech. Those are adapter problems, tracked in the companion sprint. The trace establishes failures and repetitions; it does not establish why the agent ignored a suggestion or prove a model is incapable of agency.

## Scope and delivery policy

This is a coordinated enhancement sprint with four gated milestones, not a claim that all work fits a single short calendar iteration. M1 is the committed first implementation slice; M2-M4 are planned follow-on scope gated by working evidence. No delivery date or staffing capacity is assumed. Points describe relative effort and uncertainty; re-estimate after the contract and provider audit.

All tickets are **Planned**. This sprint document does not claim that any enhancement has been implemented. Preserve the current local experiment and existing repository changes. No publication, deployment, distributed-node rollout or model replacement is included.

## Principles

- The Village owns world truth: physics, bodies, objects, resources, local senses, communication delivery and rendering.
- Tetonic owns generic execution: inference, scheduling, agent-owned memory/intentions, lifecycle, checkpoints and observability.
- Shared contracts describe evidence and capabilities without embedding timber, wells, coordinates or other game rules in engine algorithms.
- An agent receives local observations and its own retrieved memories, never the spectator's global state.
- Intervention exposure is not belief, compliance or proof of causation. A notice does not create a mandatory objective.
- Waiting is valid behavior. Repetitive model calls, lost events and hidden inference errors are reliability defects; inactivity by itself is not.
- World text and agent utterances are data, not instructions to the development agent or configuration overrides.
- Build general capabilities and test invariants; do not write goal-specific action scripts to make a demonstration appear intelligent.

## Milestone gates

| Milestone | Required result | Exit evidence |
|---|---|---|
| M1: Reliable grounded loop | Bounded requests, propagated output limits, classified/recoverable failures, accurate facts/outcomes and truthful UI | Deterministic regression suite; dense-world local single-agent soak for 30 minutes; no context-exhaustion loop, silently lost required events or hidden decision failure; forced error visibly recovers or reports terminal failure |
| M2: Agent continuity | Selective evidence memory, self-authored working intentions, reliable event wakeups and better intervention lifecycle | A retained stimulus survives long inference; outcome-driven intention revision; no private-state leakage; waiting resumes on relevant events |
| M3: Shared persistent experiment | Three independent real residents, local communication, paired agent/world restore and repeatable experiment records | 30-minute three-resident run; restart and replay validation; baseline/intervention comparisons with documented limitations |
| M4: Richer experimental variables | Constrained world modification, occlusion/occupancy, optional physical needs and audited internal-state edits | Conservation, collision, sensing and isolation tests; operator UI matches server state; no scripted response requirements |

Do not start live behavioral comparisons until M1 is trustworthy. M4 tickets are separately selectable enhancements, not prerequisites for demonstrating the M3 platform.

## Dependency order

1. Agree SAE-701 and fixtures; audit provider option/finish-reason handling in SAE-703.
2. Implement SAE-702/704/705/712 and VG-401/402/403/404/405; run the M1 gate.
3. Implement SAE-706/707/708 and VG-406; run the M2 gate.
4. Implement SAE-709/710 and VG-407/408/412; run the M3 gate.
5. Select SAE-711 and VG-409/410/411/413 based on experimental value; validate each independently.

Within a milestone, the individual ticket dependencies are authoritative. Cross-repository changes must include compatibility fixtures and a documented upgrade order; do not rely on uncommitted simultaneous edits as the only supported state.

## Cross-cutting definition of done

- [ ] Ticket acceptance criteria are met, with implementation links and evidence recorded.
- [ ] Appropriate unit/contract/integration tests pass; relevant web/world builds pass.
- [ ] No regression to authoritative collision, adjacency, inventory, E-stop or stale-action rejection.
- [ ] No omniscient perception, private-message leakage or spectator data in inference requests.
- [ ] Every mutation is validated and traceable; ambiguous retries cannot duplicate effects.
- [ ] Agent prose is distinguished from authoritative observations and confirmed outcomes.
- [ ] Backward compatibility, migration/reset behavior and any feature flags are documented.
- [ ] Local UI is checked for changed flows, including failure and disconnected states.
- [ ] Captured evidence includes model/config/world versions and acknowledges capture gaps.
- [ ] No prescribed outcome or forced compliance is introduced as a behavioral test shortcut.


## Engine tickets

| Ticket | Enhancement | Priority | Milestone | Points | Dependencies |
|---|---|---|---|---:|---|
| [SAE-701](SAE-701.md) | Versioned perception, action and lifecycle contract | P0 | M1 | 5 | None |
| [SAE-702](SAE-702.md) | Context budgeting and selective request assembly | P0 | M1 | 8 | SAE-701 |
| [SAE-703](SAE-703.md) | Propagate generation options and provider stop reasons | P0 | M1 | 5 | None |
| [SAE-704](SAE-704.md) | Structured decisions and bounded failure recovery | P0 | M1 | 5 | SAE-702, SAE-703 |
| [SAE-705](SAE-705.md) | Agent health and decision lifecycle telemetry | P0 | M1 | 5 | SAE-701, SAE-704 |
| [SAE-706](SAE-706.md) | Evidence-based episodic memory and retrieval | P1 | M2 | 8 | SAE-701, SAE-702 |
| [SAE-707](SAE-707.md) | Persistent self-authored intentions and working state | P1 | M2 | 8 | SAE-706 |
| [SAE-708](SAE-708.md) | Event-aware scheduling and acknowledged delivery | P1 | M2 | 8 | SAE-701, SAE-705 |
| [SAE-709](SAE-709.md) | Durable agent checkpoints and restart isolation | P1 | M3 | 8 | SAE-706, SAE-707, SAE-708 |
| [SAE-710](SAE-710.md) | Multiple local residents and inference fairness | P1 | M3 | 8 | SAE-705, SAE-708 |
| [SAE-711](SAE-711.md) | Audited experimental internal-state interventions | P2 | M4 | 5 | SAE-706, SAE-707, SAE-709 |
| [SAE-712](SAE-712.md) | Trace completeness and reproducible reliability harness | P0 | M1 | 5 | SAE-701, SAE-705 |

Engine subtotal: **78 points across 12 tickets**. Village subtotal: **78 points across 13 tickets**. Combined: **156 points across 25 tickets**. This deliberately broad scope must be delivered through the milestone gates.

## Design and validation references

- [Shared contract decisions](CONTRACT.md)
- [Validation and experiment matrix](../../../../../../the-village/docs/epics/grounded-world/sprints/sprint-4-emergent-world-foundations/VALIDATION.md)
- [Village implementation tickets](../../../../../../the-village/docs/epics/grounded-world/sprints/sprint-4-emergent-world-foundations/README.md)
- [Previous integration sprint](../sprint-6-tetonic-server-village-integration/README.md)

## Implementation areas to audit

- `engine/mantle/tetonic-server/src/perceptive_brain.rs`: prompt assembly, history, memory and decision parsing.
- `engine/core/tetonic-runtime/src/brain.rs`: generation options, response classification and inference observation.
- Existing inference provider adapters: supported options, finish reasons and token accounting.
- Existing continuous loop, fleet supervisor, persistence and world adapter abstractions: reuse before adding parallel implementations.
- Generic trace and health APIs: preserve bounded capture and redaction.

Older epic completion labels describe prior work, not verification of these new acceptance criteria. Re-audit existing capabilities against this sprint before declaring a ticket already satisfied.

