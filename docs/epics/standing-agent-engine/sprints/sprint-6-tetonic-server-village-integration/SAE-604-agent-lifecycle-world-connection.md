# SAE-604 — Agent Lifecycle & World Connection

| Field        | Value                                              |
|--------------|----------------------------------------------------|
| **Ticket**   | SAE-604                                            |
| **Sprint**   | Sprint 6 — Tetonic Server + Village Integration    |
| **Epic**     | Standing Agent Engine                              |
| **Type**     | Feature                                            |
| **Priority** | P0 — Critical Path                                 |
| **Estimate** | 5 pts                                              |
| **Depends**  | SAE-601, SAE-602, SAE-603                          |

---

## Objective

Wire the full agent lifecycle in `tetonic-server`: create an `Agent` from `tetonic-core`, attach the `SingleModelBrain` from SAE-603, connect to the Village via the WebSocket adapter from SAE-602, and run `agent.run_in_world()`.

This is the ticket that makes Barnaby think and act through the **real Tetonic engine**.

## Acceptance Criteria

- [ ] `tetonic-server` constructs an `Agent` with:
  - `agent_id` from CLI `--agent-id`
  - `Brain` from SAE-603's `SingleModelBrain`
  - `IntentCharter` from `--agent-charter` (or a hardcoded Barnaby default)
- [ ] Creates a `WorldManifest` for The Village with affordances:
  - `move_to` — Move to a location (params: `x`, `y`)
  - `speak` — Say something (params: `message`)
  - `interact` — Interact with an entity (params: `target`, `action`)
  - `idle` — Do nothing / wait
  - (Open world manifest is also acceptable — empty affordances list allows any action)
- [ ] Connects to the Village via `StreamWorldAdapter::connect_ws()` (or `WebSocketWorldAdapter`) using the `--world` URL.
- [ ] Calls `agent.run_in_world(adapter)` on a spawned tokio task.
- [ ] Handles graceful shutdown on `Ctrl+C` (`tokio::signal::ctrl_c()`):
  - Cancels the agent's work scope.
  - Logs agent shutdown and final brain cost summary.
- [ ] Agent logs each perception evaluation and action dispatch at `DEBUG` level.
- [ ] Agent logs brain decisions (move, speak, idle) at `INFO` level.

## Implementation Sketch

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ... CLI parsing, tracing init (SAE-601) ...
    // ... OllamaProvider + SingleModelBrain (SAE-603) ...

    // Build agent
    let agent = Agent::default()
        .with_id(AgentId::new(&args.agent_id))
        .with_brain(brain)
        .with_charter(IntentCharter {
            directive: args.agent_charter.clone(),
            ..Default::default()
        });

    // Build world adapter
    let manifest = WorldManifest::new("the-village", "1.0");
    // Open world — no affordance constraints for now
    let adapter = StreamWorldAdapter::connect_ws(&args.world_url, manifest);

    info!(agent = %args.agent_id, world = %args.world_url, "agent docked to world");

    // Run agent loop
    let agent_handle = tokio::spawn(async move {
        if let Err(e) = agent.run_in_world(adapter).await {
            error!(error = %e, "agent loop terminated with error");
        }
    });

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await?;
    info!("shutting down tetonic-server");

    // Cancel and join
    agent_handle.abort();
    let _ = agent_handle.await;

    Ok(())
}
```

## Village Integration Contract

When connected, the flow is:

```
Village World Server (Colyseus + TetonicGateway)
    │
    ├─► ws://127.0.0.1:3001/world/gateway
    │       │
    │       ▼
    │   StreamWorldAdapter (WebSocket transport)
    │       │
    │       ├── Perception ticks ──► Agent.run_in_world()
    │       │                            │
    │       │                            ▼
    │       │                        Brain.perceive()
    │       │                            │
    │       │                            ▼
    │       │                        OllamaProvider.chat()
    │       │                            │
    │       │                            ▼
    │       │                     WorldAction (move/speak/idle)
    │       │                            │
    │       ◄── Action frames ──────────┘
    │
    ▼
VillageRoom (Colyseus state update → Pixi.js render)
```

## Barnaby's Default Charter

If no `--agent-charter` is provided, use:

```
You are Barnaby, a friendly villager in a small medieval village.
You observe your surroundings and decide what to do each moment.
You can move around, speak to others, and interact with objects.
Respond with a JSON action: {"kind": "move_to"|"speak"|"idle", ...}
```

## Out of Scope

- Multi-agent support (one agent per `tetonic-server` process for now)
- Fleet supervisor integration
- Persistent memory / state checkpointing
- Village gateway authentication
