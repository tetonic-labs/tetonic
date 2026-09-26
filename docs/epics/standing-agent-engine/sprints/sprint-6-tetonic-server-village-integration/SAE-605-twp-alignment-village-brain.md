# SAE-605 — TWP Message Alignment & Village Brain Prompt

| Field        | Value                                              |
|--------------|----------------------------------------------------|
| **Ticket**   | SAE-605                                            |
| **Sprint**   | Sprint 6 — Tetonic Server + Village Integration    |
| **Epic**     | Standing Agent Engine                              |
| **Type**     | Feature                                            |
| **Priority** | P1 — High                                          |
| **Estimate** | 3 pts                                              |
| **Depends**  | SAE-602, SAE-604                                   |

---

## Objective

Ensure the TWP (Tetonic World Protocol) messages between the Village's TypeScript gateway and the Rust engine are semantically aligned, and that the `Brain.perceive()` implementation produces world actions the Village can actually execute.

This ticket bridges the "it connects" (SAE-602/604) to "it actually works end-to-end."

## Problem Statement

### 1. Perception → Brain Prompt Translation

The `Brain.perceive()` method receives a `Perception` struct. But `SingleModelBrain` doesn't implement `perceive()` — it only implements `complete()`. The default `perceive()` on the `Brain` trait returns `Ok(None)` (no action).

We need a **Village-aware perceive implementation** that:
1. Converts the `Perception` into a chat prompt describing what the agent sees.
2. Calls `complete()` with the prompt to get the LLM's decision.
3. Parses the LLM response into a `WorldAction`.

### 2. TWP Field Mapping

Verify and document the exact field mapping between:

| Village TS (`ProtocolTypes.ts`) | Rust (`StreamMessage` / `Perception`) |
|---------------------------------|---------------------------------------|
| `PerceptionData.agentId`        | `Perception.???` (may need mapping)   |
| `PerceptionData.worldState`     | `Perception.state: WorldState`        |
| `PerceptionData.nearbyEntities` | `Perception.events` or custom         |
| `PerceptionData.time`           | `Perception.timestamp`                |
| `ActionData.kind`               | `WorldAction.kind`                    |
| `ActionData.payload`            | `WorldAction.payload`                 |
| `ActionData.agentId`            | `WorldAction.agent_id`               |

### 3. Action Response Format

The Village gateway expects actions in a specific format. The LLM's text response needs to be parsed into a `WorldAction` struct.

## Acceptance Criteria

- [ ] Create a `VillageBrain` (or `PerceptiveBrain` wrapper) in `tetonic-server` that:
  - Implements `Brain` trait.
  - `perceive()`: converts `Perception` → prompt → `complete()` → parse → `WorldAction`.
  - Handles `<think>...</think>` tag stripping from qwen3.5 responses.
  - Extracts JSON from LLM freeform text using regex fallback.
  - Returns `Ok(None)` for idle/no-op decisions.
- [ ] Verify and fix any field name mismatches between Village TS types and Rust `StreamMessage` serde.
- [ ] At least one passing integration test: mock perception → VillageBrain → parsed action.
- [ ] Document the perception prompt template used for Barnaby.

## Implementation Notes

### VillageBrain Wrapper

```rust
pub struct VillageBrain {
    inner: SingleModelBrain,
}

#[async_trait]
impl Brain for VillageBrain {
    async fn perceive(&self, perception: Perception) -> Result<Option<WorldAction>, BrainError> {
        // 1. Build prompt from perception
        let prompt = format_village_prompt(&perception);

        // 2. Call the inner brain's complete()
        let req = BrainRequest {
            messages: vec![
                BrainMessage { role: BrainRole::System, content: BARNABY_SYSTEM_PROMPT.into(), .. },
                BrainMessage { role: BrainRole::User, content: prompt, .. },
            ],
            tools: vec![],
            max_tokens: Some(256),
            trace_label: format!("village-perceive-{}", perception.timestamp),
        };

        let mut sink = |_: &str| {};
        let response = self.inner.complete(req, &mut sink).await?;

        // 3. Parse response into WorldAction
        parse_village_action(&response.content)
    }

    // ... delegate other methods to inner ...
}
```

### Perception Prompt Template

```
You are standing in the village. Here is what you observe:

World State: {world_state_summary}
Nearby: {nearby_entities}
Time: {time_of_day}
Events: {recent_events}

Decide your next action. Respond with ONLY a JSON object:
{"kind": "move_to", "payload": {"x": 150, "y": 200}}
or {"kind": "speak", "payload": {"message": "Hello neighbor!"}}
or {"kind": "idle", "payload": {}}
```

## Out of Scope

- Conversation memory / multi-turn context
- Tool use (function calling) — just plain text prompting for now
- Multi-agent perception deconfliction
