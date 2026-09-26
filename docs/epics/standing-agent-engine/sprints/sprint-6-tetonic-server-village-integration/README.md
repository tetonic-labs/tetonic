# Sprint 6 — Tetonic Server + Village Integration

**Epic:** Standing Agent Engine
**Sprint Goal:** Create the `tetonic-server` binary daemon and use it to drive Barnaby in The Village through the real Tetonic engine — replacing the fake TypeScript runner entirely.

---

## Sprint Summary

This sprint bridges the gap between the Tetonic engine (Rust crates) and The Village demo (TypeScript/Colyseus). By the end, an operator starts `tetonic-server`, points it at the Village's WebSocket gateway, and watches Barnaby autonomously navigate and interact — all inference routed through `OllamaProvider`, all actions flowing through the `Agent → Brain → WorldAdapter` pipeline.

## Tickets

| ID      | Title                                  | Points | Priority | Depends On     |
|---------|----------------------------------------|--------|----------|----------------|
| SAE-601 | `tetonic-server` Binary Scaffold       | 3      | P0       | SAE-401, 402   |
| SAE-602 | WebSocket World Adapter                | 5      | P0       | SAE-203, 601   |
| SAE-603 | Inference Provider Wiring              | 3      | P0       | SAE-601, 403   |
| SAE-604 | Agent Lifecycle & World Connection     | 5      | P0       | 601, 602, 603  |
| SAE-605 | TWP Alignment & Village Brain Prompt   | 3      | P1       | 602, 604       |
| SAE-606 | End-to-End Demo & Runner Retirement    | 3      | P0 Gate  | All above      |

**Total:** 22 points

## Dependency Graph

```
SAE-601 (binary scaffold)
   │
   ├──► SAE-602 (WebSocket adapter)
   │        │
   ├──► SAE-603 (inference wiring)
   │        │
   └──► SAE-604 (agent lifecycle) ◄── SAE-602 + SAE-603
            │
            ▼
        SAE-605 (TWP alignment + VillageBrain)
            │
            ▼
        SAE-606 (E2E demo + runner retirement) ★ Sprint Gate
```

## Architecture Context

```
┌─────────────────────────────────────────────────────┐
│                  tetonic-server                      │
│                  (Standalone mode)                    │
│                                                      │
│  ┌─────────────┐    ┌──────────────────────────┐    │
│  │ OllamaProvider│   │  Agent (tetonic-core)     │    │
│  │ (tetonic-     │   │   ├─ IntentCharter        │    │
│  │  inference)   │   │   ├─ SensoryFilter        │    │
│  └──────┬────────┘   │   └─ WorkScope            │    │
│         │            └──────────┬─────────────────┘   │
│         ▼                       │                     │
│  ┌──────────────┐               │                     │
│  │SingleModelBrain│◄─────────────┘                     │
│  │(tetonic-runtime)│  .perceive() / .complete()        │
│  └──────────────┘                                     │
│         │                                             │
│         ▼                                             │
│  ┌───────────────────────┐                            │
│  │ StreamWorldAdapter     │                            │
│  │ (WebSocket transport)  │                            │
│  └──────────┬────────────┘                            │
│             │ ws://127.0.0.1:3001/world/gateway       │
└─────────────┼─────────────────────────────────────────┘
              │
              ▼
┌─────────────────────────────┐     ┌──────────────┐
│  Village World Server        │     │ Village Web   │
│  (Colyseus + TetonicGateway) │────►│ (Pixi.js)    │
│  :3001                       │     │ :3000         │
└─────────────────────────────┘     └──────────────┘
```

## Key Decisions

1. **Single binary, single agent** — For this sprint, one `tetonic-server` process hosts one agent. Multi-agent fleet support is a future sprint.

2. **Open world manifest** — The Village world manifest starts with no affordance constraints (any action kind is accepted). This lets us iterate on what actions the LLM produces without breaking on manifest validation.

3. **VillageBrain wrapper** — Rather than modifying `SingleModelBrain`, we wrap it in a `VillageBrain` that implements `perceive()` by converting perceptions to prompts and parsing LLM responses into actions.

4. **localhost only** — All binds on `127.0.0.1`. No TLS, no auth.

## Definition of Done

> Three terminals: Village world (`:3001`), Village web (`:3000`), `tetonic-server`. Open the browser. Barnaby moves and speaks autonomously. Engine logs show the real perception→brain→action cycle. The TypeScript runner is archived.

---

> [!NOTE]
> **No hosted deployment this sprint.** The end-to-end demo runs fully locally (`127.0.0.1` only). We are not deploying to the hosted web server or public website today. A deployment sprint will be planned separately once the local integration is validated and stable.
