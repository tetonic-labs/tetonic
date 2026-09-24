# Epic: Standing Agent Engine & Platform

> Shaping Tetonic into an autonomous agent operating system that runs persistently on a desktop or cluster, enabling operators to formulate multi-dimensional intent, aim fleets at target worlds via secure World Adapters, and steer or E-Stop execution with zero friction.

---

## 1. Architectural Mission & Core Axioms

1. **"Expert-Level Engine Design; Brain-Dead Easy UI"**
   * Hardened systems architecture under the hood (actor location transparency, pluggable brains, actuator safety gates).
   * Frictionless human experience on the glass (interactive intent formulation, in-flight steering, 1-click E-Stop).
2. **"The 1-to-1,000 Scale Invariant"**
   * Runs as a single zero-config binary on a developer's desktop or as an enterprise cluster across 1,000 nodes via `tetonic.toml`.
3. **"The Aimed Fleet"**
   * Tetonic is a standing, 24/7 operating platform. Organizations and teams are aimed at target environments via `WorldAdapter` streams.
4. **"Pluggable Cognition"**
   * `SingleModelBrain` is the zero-friction default. Advanced multi-tier architectures (`DualProcessBrain`, `ScriptedBrain`, `EnsembleBrain`) plug into the exact same `Brain` trait.
5. **"Authoritative World Safety"**
   * The `WorldAdapter` acts as an unbypassable actuator interlock, providing physical E-Stop capabilities and affordance negotiation.

---

## 2. Refined Sprint Roadmap

```text
┌────────────────────────────────────────────────────────────────────────┐
│ SPRINT 1: Continuous Cognition & Pluggable Brain [COMPLETED]           │
│ - Domain re-exports (Perception, WorldAction, WorldAdapter)            │
│ - run_continuous() actor loop in tetonic-core with latest-value drops  │
│ - Pluggable Brain: SingleModelBrain (default) + DualProcessBrain       │
│ - Sensory filtering: decouple high-frequency ticks from memory context │
├────────────────────────────────────────────────────────────────────────┤
│ SPRINT 2: World Adapters & Actuator Safety Gates [COMPLETED]           │
│ - WorldAdapter affordance negotiation (world advertises its verbs)     │
│ - Authoritative E-Stop & Actuator Interlock (drop mutations, freeze)   │
│ - Multi-adapter composition (agents binding to multiple environments)  │
│ - WebSocket / Stream WorldAdapter implementation                       │
├────────────────────────────────────────────────────────────────────────┤
│ SPRINT 3: Standing Fleet, Multi-Dimensional Intent & Steering [COMPLETED]│
│ - Organization & Squad runtime structures (tenancy, budget quotas)     │
│ - Composite Intent Charter: multi-target boundaries & constraints      │
│ - In-Flight Course Correction: inject real-time steering vectors       │
│ - FleetSupervisor: persistent agent lifecycles, heartbeats, failover   │
├────────────────────────────────────────────────────────────────────────┤
│ SPRINT 4: Engine Configuration & Decoupled Inference (Atmos & Config)  │
│ - Declarative tetonic.toml parser (standalone vs coordinator vs runner)│
│ - Decoupled stateless GPU inference fabric routing                     │
│ - Moveable execution volume and state checkpoint contract              │
├────────────────────────────────────────────────────────────────────────┤
│ SPRINT 5: Fleet Portal & Human Experience (Litho)                      │
│ - Brain-dead simple creation API (POST /orgs, POST /agents)            │
│ - Zero-compute telemetry & live thought inspection streams             │
│ - Operator Control Surface: Intent builder, steering, and E-Stop UI    │
└────────────────────────────────────────────────────────────────────────┘
```
