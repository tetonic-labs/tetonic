# MVP Specification: Tetonic Live Agent Engine for "The Village"

> Scoping, integration contracts, and architecture for the Minimum Viable Product (MVP) powering *The Village* sandbox application.

---

## 1. MVP Objective

Deliver a fully operational instance of the **Tetonic Agent Engine** capable of hosting a live, self-governing team of agents embedded in the Pixi.js *Village* sandbox.

* **World Real-Time Loop:** The Village advances on its own clock; Tetonic agents perceive changes, maintain roles, converse, make decisions, and manipulate the world state.
* **Dual Cognition in Action:** Demonstrate fast reflexive reactions ($<500\text{ms}$) combined with deep, asynchronous planning ($2-5\text{s}$) without pausing the simulation.
* **The Looking Glass:** Provide an ingress channel where site visitors submit proposals, petitions, or resources to the Village, which agents organically consider and debate.

---

## 2. In Scope vs. Deferred for MVP

| Capability | In Scope (MVP) | Deferred (Post-MVP) |
| :--- | :--- | :--- |
| **Cluster Topology** | Single-node deployment (developer workstation or single cloud instance). | Multi-node distributed actor clustering across physical racks. |
| **Agent Fleet** | 3 to 5 specialized agents (e.g., Mayor, Blacksmith, Scout, Builder). | 50+ agent populations across multiple settlements. |
| **Brain Architecture** | `DualProcessBrain` (System 1 fast reflex + System 2 reasoning) & `SingleModelBrain`. | Dynamic ensemble voting with real-time token auction bidding. |
| **Inference Backing** | Local Ollama (fast 8B model for System 1) + Claude/Gemini (System 2). | Fully pooled, fault-tolerant bare-metal inference cluster. |
| **World Binding** | WebSocket / JSON-RPC stream between Tetonic and the Village backend. | Direct memory IPC / shared-memory zero-copy ring buffers. |
| **Tool Execution** | In-universe game actions (`move`, `speak`, `craft`, `post_notice`). | Arbitrary OS shell execution and Wasm sandboxes. |

---

## 3. The Integration Seam: World Adapter Contract

The Village backend and Tetonic Engine communicate over a bidirectional **WebSocket** connection.

```text
┌───────────────────────┐                    ┌───────────────────────┐
│      THE VILLAGE      │   WebSocket / IPC  │    TETONIC ENGINE     │
│  (Pixi.js / Server)   │ ══════════════════ │  (Mantle & Core)      │
│                       │                    │                       │
│  [World State Tick]   │ ── Perception ───► │ [Perception Adapter]  │
│                       │                    │        │              │
│                       │                    │   Dual Brain Loop     │
│                       │                    │        │              │
│  [World Mutation API] │ ◄── Actions ────── │ [Motor Adapter]       │
└───────────────────────┘                    └───────────────────────┘
```

### A. Village $\rightarrow$ Tetonic: Perception Packet

Sent by the Village on each world tick or event trigger:

```json
{
  "sequence": 1420,
  "timestamp": "2026-09-23T18:30:00Z",
  "urgency": "medium",
  "signals": [
    {
      "name": "granary_fill_ratio",
      "value": 0.28,
      "changed": true,
      "trend": "falling",
      "urgency": "medium"
    },
    {
      "name": "unresolved_petitions",
      "value": 2,
      "changed": false,
      "trend": "stable",
      "urgency": "low"
    }
  ],
  "events": [
    {
      "kind": "visitor_petition_submitted",
      "source": "looking_glass_guest_42",
      "payload": {
        "text": "The northern bridge is washed out. Can we rebuild it?",
        "suggested_priority": "high"
      },
      "urgency": "high"
    }
  ],
  "state": {
    "schema_id": "village_v1",
    "data": {
      "time_of_day": "dusk",
      "weather": "clear",
      "agents": {
        "mayor": { "pos": [45, 12], "action": "idle", "energy": 82 },
        "blacksmith": { "pos": [18, 30], "action": "forging", "energy": 64 }
      },
      "resources": { "timber": 140, "iron": 35, "grain": 45 }
    }
  }
}
```

### B. Tetonic $\rightarrow$ Village: Action Packet

Emitted by Tetonic when an agent's brain decides on an action:

```json
{
  "agent_id": "mayor",
  "action_id": "act_89f02a",
  "kind": "post_notice",
  "decided_at": "2026-09-23T18:30:01.250Z",
  "pathway": {
    "kind": "deliberative",
    "model": "claude-sonnet-4-5"
  },
  "payload": {
    "board": "town_square",
    "title": "Northern Bridge Reconstruction",
    "content": "A petition has arrived. Scout Eldon to inspect northern coast timber; Smith Torin to ready 20 iron nails.",
    "priority": "urgent"
  }
}
```

Supported Action Kinds for MVP:
* `move_to`: Navigate to tile coordinates `[x, y]`.
* `interact_with`: Interact with a building, container, or resource node.
* `speak`: Send a local in-world chat message to nearby agents.
* `post_notice`: Add an entry to the shared Town Notice Board.
* `craft_or_build`: Dedicate work cycles to an in-world construction or item.

---

## 4. MVP Fleet Composition

| Agent | Core Identity | Primary Drives | Cognitive Split |
| :--- | :--- | :--- | :--- |
| **The Mayor** | Civic administrator & mediator. | Civic stability, visitor hospitality, resource balance. | System 2 heavy (deep deliberation on notices & petitions). |
| **The Blacksmith** | Craftsperson & resource steward. | Industry, structural integrity, material efficiency. | Balanced (System 1 for shop tasks, System 2 for commissions). |
| **The Scout** | Explorer & ranger. | Alertness, perimeter security, foraging discovery. | System 1 heavy (fast reactive pathfinding & hazard warnings). |

---

## 5. Implementation Roadmap (Tetonic Engine Side)

1. **Phase 1: Domain & Protocol Contracts (`tetonic-domain`)**
   * Finalize `Perception`, `Signal`, `WorldEvent`, `WorldAction`, and `WorldAdapter`.
2. **Phase 2: Continuous Runtime Loop (`tetonic-core`)**
   * Implement `run_continuous` on `Agent` with `watch` channel ingestion and cancellation token mechanics.
3. **Phase 3: DualProcessBrain (`tetonic-runtime`)**
   * Wire fast reflex provider (System 1) with asynchronous deliberative provider (System 2) and preemption triggers.
4. **Phase 4: Village WebSocket Adapter (`tetonic-runtime` / `mantle`)**
   * Provide a plug-and-play WebSocket client that connects Tetonic to the Village application backend.
5. **Phase 5: Looking Glass Ingress Conduit**
   * Standardize the petition pipeline from external web visitors into the perception event stream.
