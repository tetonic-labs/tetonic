# SAE-606 — End-to-End Demonstration & Runner Retirement

| Field        | Value                                              |
|--------------|----------------------------------------------------|
| **Ticket**   | SAE-606                                            |
| **Sprint**   | Sprint 6 — Tetonic Server + Village Integration    |
| **Epic**     | Standing Agent Engine                              |
| **Type**     | Validation / Cleanup                               |
| **Priority** | P0 — Sprint Gate                                   |
| **Estimate** | 3 pts                                              |
| **Depends**  | SAE-601, SAE-602, SAE-603, SAE-604, SAE-605        |

---

## Objective

Validate the full end-to-end pipeline: Village world server → WebSocket TWP → `tetonic-server` → OllamaProvider → Barnaby brain → WorldAction → Village state update → Pixi.js render. Then retire the fake TypeScript runner.

This is the **sprint gate** — if this works, the sprint is done.

## Acceptance Criteria

### End-to-End Validation

- [ ] **Three-process startup** (all on localhost):
  1. `the-village/world` — Colyseus world server on `:3001`
  2. `the-village/web` — Pixi.js frontend on `:3000`
  3. `tetonic-server --standalone --world ws://127.0.0.1:3001/world/gateway --model qwen3.5:latest --agent-id barnaby`
- [ ] Barnaby appears in the Pixi.js viewport as a sprite.
- [ ] Within 60 seconds of connection, Barnaby produces at least one visible action (movement or speech) driven by real Ollama inference through the Tetonic engine.
- [ ] Engine logs show the perception → brain → action cycle:
  ```
  [INFO] perception received | tick=42 | events=[]
  [DEBUG] brain.perceive() | model=qwen3.5:latest | latency=3.2s
  [INFO] action dispatched | kind=move_to | payload={"x":150,"y":200}
  ```
- [ ] E-Stop works: triggering E-Stop from the operator panel (or via the world adapter) freezes Barnaby's actions.
- [ ] `Ctrl+C` on `tetonic-server` cleanly disconnects the agent and exits.

### Runner Retirement

- [ ] `the-village/runner/` directory is deleted or moved to `the-village/_archive/runner/`.
- [ ] Any npm scripts or documentation referencing the runner are updated.
- [ ] The Village README is updated to reference `tetonic-server` as the agent runtime.

## Validation Procedure

### Step 1: Start the Village
```bash
cd the-village/world && npm run dev    # Port 3001
cd the-village/web && npm run dev      # Port 3000
```

### Step 2: Start Tetonic Server
```bash
cd engine && cargo run -p tetonic-server -- \
    --mode standalone \
    --world ws://127.0.0.1:3001/world/gateway \
    --model qwen3.5:latest \
    --agent-id barnaby \
    --agent-charter "You are Barnaby, a friendly villager. Explore and interact."
```

### Step 3: Observe
- Open `http://localhost:3000` in browser.
- Watch Barnaby's sprite. It should begin moving or speaking within ~30-60s (first inference is slow due to model loading).
- Check `tetonic-server` terminal for perception/action logs.

### Step 4: E-Stop Test
- If operator controls exist in the web UI, click E-Stop.
- Verify Barnaby freezes (no new actions dispatched).
- Click Resume, verify actions resume.

### Step 5: Shutdown
- `Ctrl+C` on `tetonic-server`.
- Verify clean disconnect logged.
- Verify Village continues running without the agent.

## Definition of Done

> An operator opens `localhost:3000`, sees Barnaby's sprite, and watches him autonomously move and speak in the village — all driven by the Tetonic engine runtime, not a hardcoded TypeScript loop.

## Out of Scope

- Performance benchmarks / optimization
- Multi-agent demo
- Persistent state across restarts
