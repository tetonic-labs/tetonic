# lokai-orchestrator

Session orchestration between RPC/CLI and the agent loop: briefing, routing, specialists, critic, turn tracking.

## Role in the stack

`lokaid` and the **`lokai` CLI** both call `run_orchestrated_turn` from this crate (`turn.rs`) for `chat/send` / `--orchestrate auto`. `SessionHost` runs at session start; `agent/spawn` and in-loop `spawn_agent` use `run_spawned_specialist`. Does not perform inference or tool I/O itself.

## Modules

| Module | Purpose |
|--------|---------|
| `host.rs` | `SessionHost`, `SessionStartPlan` — briefing, verify, project context, session-end consolidate |
| `briefing.rs` | Repo map, index working set, LSP hint, memory stats (D5 v4) |
| `router.rs` | Keyword router + file/index hints + hard tier (D11) |
| `router_llm.rs` | Optional LLM router with keyword fallback + eval fixtures |
| `handoff.rs` | Compact spawn return `{ summary, pointers }` (A13 v5) |
| `spawn_host.rs` | Reusable in-loop spawn driver (nested spawn, budget carve) |
| `spawn_budget.rs` | `SpawnBudgetGate` trait — ledger admit/release before child Agent (H2-3) |
| `spawn_session.rs` | `SpawnSessionTrack` (`Arc` + poison-tolerant `Mutex`) shared with `lokai-app` live sessions |
| `specialist.rs` | Role overlays + tool subsets |
| `critic.rs` | Problem-driven critic + LSP diagnostic snippets (D12 v4) |
| `run.rs` | `SpawnLimits`, `TurnTracker`, child agent ids |
| `turn.rs` | `run_orchestrated_turn`, `run_spawned_specialist` |
| `spawn.rs` | `spawn_agent` JSON schema helpers |
| `turn_tests.rs` | Integration tests (mock agent loop, spawn host, LLM router) |

## Key API (v5)

- `run_orchestrated_turn`, `run_spawned_specialist`, `SpawnHost`, `OrchestratedTurnInput`, `SpawnLimits`
- `SpawnBudgetGate`, `SpawnAdmitCtx`, `SpawnAdmitError` — fan-out paid via app `LedgerSpawnBudgetGate` before `build_agent` (H2-3); `SpawnLimits` remains the coarse ceiling
- `SpawnHandoff`, `SpawnPointer`, `carve_max_steps` — parent never sees child transcript
- `resolve_route`, `llm_route_task`, `RouteSource`, `format_router_log`, `format_orchestration_log`
- `should_run_critic_enhanced`, `SessionHost::on_session_start`

### Critic gating (D12 v4)

- Runs after **any** routed turn (specialist or root `a0`) when there were mutating edits.
- Triggers when **LSP diagnostics** reported issues, or when **verify failed** and the session configured a verify command (`verify_gated`).
- Optional revision coder pass when critic returns `REVISE:`.

### Workspace keys

Index and briefing paths use `lokai_index::workspace_storage_key` so lookups align with `lokai-memory` v14 canonical keys and Windows `\\?\` drift.

## Session / env

| Source | Default | Purpose |
|--------|---------|---------|
| `session/start.llm_router` | `false` | One-line LLM router before keyword fallback |
| `LOKAI_LLM_ROUTER` | off | Env fallback for LLM router |
| `LOKAI_MAX_SPAWN_PER_TURN` | `4` | In-loop `spawn_agent` budget per user turn |
| `LOKAI_MAX_SPAWN_DEPTH` | `2` | Max nesting depth (`a0` → `a0_s0` → `a0_s0_s0`) |
| CLI `--llm-router` | off | Same as session flag |

## Fabric & estate alignment (inference outsourcing)

Orchestration v5 is designed to sit on the **homelab / Circle** model without moving tools or workspace off the coordinator:

| Orchestration concern | Local (coordinator) | Remote (fabric worker / Circle peer) |
|----------------------|---------------------|----------------------------------------|
| Tools, shell, edits | Always | Never (charter invariant) |
| Router LLM call | Default | Optional — same `InferenceProvider` |
| Specialist / spawn child turns | Default | Policy + tier gated |
| **`model_tier: hard`** | Fallback | **Preferred** when enrolled workers expose the hard model (`PooledProvider` placement) |
| Turn affinity | N/A | Same user turn sticks to one worker (N1.2) |
| Spawn handoff | `{ summary, pointers }` only | Pointers reference index/memory — not raw child transcript |
| Data class | `session/start.data_class` | `FabricCallMeta.data_class` — blocks remote when `private` (D1 stub) |

**Estate (N0):** enrolled workers are same-owner nodes; hard-tier routes naturally offload to GPU boxes while the coordinator keeps the workspace and audit spine.

**Circle (N2+, future):** same split — **think locally · infer remotely · act locally**. Orchestrator spawn budgets and compact handoffs limit context shipped across the fabric; disclosure tiers (V1–V6) gate which jobs a peer will run.

**Receipts (L1–L4, planned):** each fabric `chat` already carries `FabricCallMeta` (`session_id`, `agent_id`, `turn_id`, `model_tier`); ledger rows will attach to these ids without prompt bodies.

## Product plan

| ID | Feature | Status |
|----|---------|--------|
| D5 / A16 | Session briefing | Done (v4 memory stats; per-turn consolidate deferred to session end) |
| D11 / A11 | Router + specialists | Done (v5 handoff, LLM session flag, fabric hard tier) |
| D12 / A12 | Critic loop | Done (v4 LSP snippets, root-agent critic, verify_gated) |
| A13 | `spawn_agent` | Done (nested spawn, budget carve, compact handoff) |

## Tests

`cargo test -p lokai-orchestrator` — 52 tests: handoff carve, spawn host dispatch, router eval fixtures, critic gating, hostile-repo fixtures, full turn loop integration (`turn_tests.rs`).

## Related docs

- [agent-rpc-v1](../../../docs/implementation/contracts/agent-rpc-v1.md)
- [circle-implementation-plan](../../../docs/implementation/circle-implementation-plan.md)
- [memory-and-context-v1](../../../docs/implementation/memory-and-context-v1.md)
