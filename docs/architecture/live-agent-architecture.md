# Live Agent & Autonomous Organization Architecture

> Architectural design specification for continuous cognition, dual-process brains, institutional coordination (Mantle), and distributed compute scaling in Tetonic.

---

## 1. Architectural Mission

Traditional LLM agent engines are turn-based: an external trigger pauses the universe, an LLM evaluates the state synchronously, produces an output, and the world unpauses. 

This architecture introduces **Continuous Cognition** and **Autonomous Organizations**:
1. **The World Never Pauses:** The environment advances on a continuous clock.
2. **Dual-Process Cognition (System 1 & System 2):** Fast reflexive regulation runs continuously at world cadence; deep deliberative planning runs asynchronously across multiple ticks.
3. **Mantle Institutional Fabric:** Multi-agent organizations, teams, and shared memory commons instantiated, budgeted, and governed as first-class primitives.
4. **General-Purpose Portability:** The engine is world-agnostic. The exact same agent fleet definition can run inside a game simulation (e.g., *The Village*), a cloud DevOps infrastructure, or a software development repository.

---

## 2. Five-Layer Alignment

```text
┌─────────────────────────────────────────────────────────────┐
│ Litho: Multi-User Portals, Architect TUI, The Looking Glass │
├─────────────────────────────────────────────────────────────┤
│ Mantle: Organizations, Teams, Fleets, Lifecycles, Broker    │
├─────────────────────────────────────────────────────────────┤
│ Core: Continuous Agent Loop, Dual Brain, Senses, Limbs      │
├─────────────────────────────────────────────────────────────┤
│ Strata: Episodic Memory, Institutional Commons, Chronicles  │
├─────────────────────────────────────────────────────────────┤
│ Atmos: Inference Fabric, Model Tier Routing, Egress Guard   │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. The Organism Anatomy (`Core`)

Every agent operates as an autonomous organism with an intrinsic core and extrinsic boundary adapters:

```text
       ┌────────────────────────────────────────────────────────┐
       │                       THE AGENT                        │
       │                                                        │
       │   Identity & Drives  ◄───►  Durable & Working Memory   │
       │              ▲                          ▲              │
       │              └────────────┬─────────────┘              │
       │                           ▼                            │
       │                    THE DUAL BRAIN                      │
       │             (Fast Reflex ◄──► Slow Reason)             │
       │                           ▲                            │
       └───────────────────────────┼────────────────────────────┘
                                   │  (Senses & Limbs)
     ══════════════════════════════╪══════════════════════════════  [Boundary]
                                   ▼
                       THE WORLD ADAPTER (Medium)
```

### Intrinsic Substrates (Travels with the Agent)
* **Identity & Baseline Drives:** Immutable agent charter, personality, core imperatives (curiosity, survival, duty, mastery).
* **Memory Substrate:** Private working memory buffer, episodic history, and mental sketches of peer agents.
* **The Dual Brain:**
  * **System 1 (Reflexive Pilot):** Continuous, low-latency ($10\text{ms} - 200\text{ms}$), state-maintenance, pattern-matching, fast motor adjustments.
  * **System 2 (Deliberative Advisor):** Episodic, deep-planning, long-horizon synthesis, counterfactual reasoning ($2\text{s} - 10\text{s}$).

### Extrinsic Boundary (Interface Adapters)
* **Perception Adapter (Senses):** Compresses raw environment delta into:
  * **Signals:** Pre-computed metrics with trend and urgency tags (`grain_level: 0.23, falling`).
  * **Events:** Discrete occurrences since the last tick (messages, alerts, visitor petitions).
  * **State:** Full opaque world snapshot passed to System 2 when needed.
* **Motor Adapter (Limbs):** Expresses high-level intent (`WorldAction`) into validated environment commands.

---

## 4. Temporal Mechanics & The Two Clocks

* **The World Clock ($T_w$):** Continuous, wall-time ($1\text{s} = 1\text{s}$).
* **The Cognitive Clock ($T_c$):** Bursty, variable-latency ($0.1\text{s} - 8\text{s}$).

### Cognitive Coupling Mechanics
1. **Active Postures / Stances:** While System 2 deliberates, System 1 holds the line by executing background behavioral postures (holding perimeter, foraging, maintaining conversation posture).
2. **Salience Interrupts:** System 1 detects anomalies or urgent state violations and interrupts System 2.
3. **Preemption & Plan Invalidation:** If System 1 observes critical world mutations while System 2 is calculating a plan, it cancels the in-flight generation. Stale plans are never executed.
4. **Arbitration Hierarchy:** 
   * **Reflex (System 1)** has immediate veto power for survival and posture adjustments.
   * **Deliberation (System 2)** establishes long-term policy, high-level intent, and tactical plans.

---

## 5. Mantle: Organizations and Fleet Coordination

Mantle is the institutional orchestrator that provisions and maintains multi-agent groups.

```text
                      ┌────────────────────────────────┐
                      │     ORGANIZATION RUNTIME       │
                      │  - Identity & Institutional ID │
                      │  - Charter & Global Policies   │
                      │  - Resource Quota (Tokens/$)   │
                      └───────┬────────────────┬───────┘
                              │                │
            ┌─────────────────┴────┐      ┌────┴─────────────────┐
            │     TEAM / SQUAD     │      │     TEAM / SQUAD     │
            │  - Mission & Scope   │      │  - Mission & Scope   │
            │  - Shared Workpad    │      │  - Shared Workpad    │
            └─────────┬────────────┘      └─────────┬────────────┘
                      │                             │
                      ▼                             ▼
               [ Agent Fleet ]               [ Agent Fleet ]
```

### Mantle Primitives
1. **Organization Blueprint:** Declarative specification containing the institutional charter, role taxonomy, resource ceilings, and governance policies.
2. **Living Organization Instance:** Stateful cluster presence maintaining agent registries, institutional memory commons in Strata, and policy enforcement.
3. **Team / Squad Container:** Colocated micro-groups sharing high-bandwidth, ephemeral working whiteboards.
4. **World Binding Seam:** The docking mechanism that binds an organization's agents to a target application without leaking application-specific types into core cognition.

---

## 6. Distributed Compute Topology (At Scale)

At scale, the system splits into four specialized compute planes:

1. **Coordination Plane (Mantle / Litho):** Multi-tenant lifecycle, consensus, task routing, policy enforcement. (CPU-bound, low-latency network I/O).
2. **Agent Runner Nodes (Core):** Continuous tick loops, System 1 heuristics, perception parsing, action verification. (Multi-core CPU, high RAM, colocated with World Adapter).
3. **Inference Fabric (Atmos):** Decoupled, pooled GPU clusters hosting fast local models (System 1) and heavy reasoning models (System 2), with failover to external hosted providers.
4. **State & Memory Fabric (Strata):** Distributed vector search, event-sourced chronicle logs, partitioned organizational commons. (Fast NVMe, low-latency KV store).
