# Tetonic World Protocol (TWP) Wire Specification (VIL-101)

> The official, domain-neutral wire contract for connecting external game engines, physical simulations, robotic systems, and virtual environments to the Tetonic Standing Agent Engine.

---

## 1. Architectural Mission & Principles

1. **Strict Domain Neutrality**: The Tetonic core engine contains zero game-specific, simulation-specific, or proprietary domain code. The engine communicates with external worlds exclusively through this standard protocol.
2. **Transport Agnostic**: The protocol operates over any bidirectional stream:
   * **WebSockets**: Standard text frames (ideal for browser games, Node.js/Colyseus, and web services).
   * **TCP Streams**: Newline-Delimited JSON (`NDJSON`) framing (ideal for high-throughput headless engines, microservices, and game servers).
   * **Unix Domain Sockets / In-Memory Duplex Streams**: For co-located sidecars and zero-network overhead testing.
3. **Latest-Value & Zero-Compute Filtering**: The engine expects world ticks to report signal deltas and urgency levels, enabling automatic sensory filtering ($>80\%$ tick suppression on idle states).

---

## 2. Framing & Envelope Specification

Every transmission across the stream is a JSON object conforming to the `StreamMessage` envelope:

```json
{
  "type": "<message_type>",
  "data": { ... }
}
```

### Supported Message Types (`StreamMessage`)

| Message Type | Direction | Description |
| :--- | :---: | :--- |
| `perception` | World $\rightarrow$ Tetonic | Sensory tick delivered to an agent's continuous cognitive loop. |
| `action` | Tetonic $\rightarrow$ World | Motor decision issued by the agent's brain upon safety gate clearance. |
| `action_result` | World $\rightarrow$ Tetonic | Execution confirmation, actuator feedback, or error diagnostics. |
| `estop` | Bidirectional | Authoritative Emergency Stop signal immediately freezing actuator execution. |
| `resume` | Bidirectional | Resumption clearance re-authorizing agent actuator execution. |
| `heartbeat` | Bidirectional | Liveness ping/pong packet to detect socket dropouts. |

---

## 3. Wire Schema Definitions

### A. Perception Message (`type: "perception"`)

Emitted by the game simulation on each tick:

```json
{
  "type": "perception",
  "data": {
    "when": "2026-09-24T00:00:00Z",
    "sequence": 1420,
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
        "name": "timber_reserve",
        "value": 140,
        "changed": false,
        "trend": "stable",
        "urgency": "low"
      }
    ],
    "events": [
      {
        "kind": "visitor_petition_submitted",
        "source": "guest_42",
        "payload": {
          "title": "Rebuild the footbridge",
          "priority": "high"
        },
        "urgency": "high"
      }
    ],
    "state": {
      "schema_id": "village_v1",
      "data": {
        "time_of_day": "dusk",
        "weather": "clear",
        "agent_position": [45, 12],
        "nearby_entities": ["anvil", "timber_stack", "scout"]
      }
    }
  }
}
```

#### Fields:
* `sequence` (integer): Monotonically increasing tick counter to detect packet drops or re-orderings.
* `urgency` (string): `"background"` | `"low"` | `"medium"` | `"high"` | `"critical"`.
* `signals` (array): Pre-computed metrics for System 1 fast pattern matching. Values can be boolean, integer, float, or string.
* `events` (array): Discrete occurrences since the last tick (e.g. messages, petitions, hazards).
* `state` (object): Arbitrary world state snapshot opaque to core engine infrastructure.

---

### B. Action Message (`type: "action"`)

Emitted by Tetonic when an agent's brain decides on a motor action:

```json
{
  "type": "action",
  "data": {
    "agent_id": "agent-mayor-01",
    "kind": "post_notice",
    "parameters": {
      "board": "town_square",
      "title": "Bridge Reconstruction Notice",
      "content": "Timber allocation approved for southern crossing.",
      "priority": "urgent"
    },
    "issued_at": "2026-09-24T00:00:01.250Z"
  }
}
```

#### Fields:
* `agent_id` (string): The identity of the acting agent.
* `kind` (string): The affordance verb (e.g., `"move_to"`, `"place_tile"`, `"craft"`, `"speak"`, `"post_notice"`).
* `parameters` (object): Arbitrary JSON payload required by the world's actuator.
* `issued_at` (string): Timestamp of brain decision.

---

### C. Action Result Message (`type: "action_result"`)

Emitted by the world actuators back to Tetonic:

```json
{
  "type": "action_result",
  "data": {
    "success": true,
    "feedback": "Notice pinned to town square bulletin board at coordinates [45, 15].",
    "state_changed": true
  }
}
```

---

### D. Emergency Stop & Resume (`type: "estop"` / `type: "resume"`)

Trips physical interlocks across both the engine and world simulator:

```json
{
  "type": "estop",
  "data": {
    "reason": "Physical boundary violation: agent stepped into restricted hazard area"
  }
}
```

```json
{
  "type": "resume",
  "data": null
}
```

---

## 4. Minimal Reference Client (Node.js / TypeScript)

External game servers (such as `the-village/world`) can implement TWP in under 50 lines of code:

```typescript
import { WebSocket } from 'ws';

const ws = new WebSocket('ws://localhost:8000/api/v1/stream');

ws.on('open', () => {
  console.log('Connected to Tetonic Engine');
  
  // Send 1Hz perception tick
  setInterval(() => {
    const perceptionMsg = {
      type: 'perception',
      data: {
        sequence: Date.now(),
        when: new Date().toISOString(),
        urgency: 'low',
        signals: [{ name: 'health', value: 100, changed: false, trend: 'stable', urgency: 'low' }],
        events: [],
        state: { schema_id: 'grid_v1', data: { x: 10, y: 15 } }
      }
    };
    ws.send(JSON.stringify(perceptionMsg) + '\n');
  }, 1000);
});

ws.on('message', (raw: string) => {
  const msg = JSON.parse(raw);
  if (msg.type === 'action') {
    console.log(`Agent executed action: ${msg.data.kind}`, msg.data.parameters);
    // Apply action to game physics, then send result:
    ws.send(JSON.stringify({
      type: 'action_result',
      data: { success: true, feedback: 'OK', state_changed: true }
    }) + '\n');
  }
});
```
