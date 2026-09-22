# lokaid

Editor-spawned daemon: thin JSON-RPC transport adapter over stdin/stdout. All application behavior delegates to `lokai-app`; lokaid authenticates, decodes, translates, invokes, encodes, and streams.

## Modes

| Invocation | Behavior |
|------------|----------|
| `lokaid` (default) | Stdio JSON-RPC coordinator; no TCP listen |
| `lokaid --supervise` | Parent restart loop around the coordinator (`LOKAI_SUPERVISE=1` also). Crash kills in-flight Infer/shell; restart rehydrates from `lokai.db`. |
| `lokaid --print-schema` | Emit protocol JSON Schema |
| `lokaid --node --enroll` | One-shot enrollment server |
| `lokaid --node` | Worker fabric listener |
| `lokaid --combined` | Stdio coordinator + local fabric scheduler (N1.2) |

Stdout carries **only** protocol bytes; logs go to stderr.

`session/inference` reads a live session's model/profile selection;
`session/setInference` changes it while idle using an expected revision. Both
delegate to the application layer. See [inference selection](../../product/lokai-app/INFERENCE-SELECTION.md).

## Modules

| Module | Purpose |
|--------|---------|
| `daemon.rs` | `Daemon` struct + RPC dispatch table |
| `daemon/handlers/` | Thin per-method RPC adapters (translate → app service → translate) |
| `daemon/rpc/` | App-error mapping, turn host builder, egress stream helper |
| `daemon/events.rs` | `ApplicationEvent` → JSON-RPC notification adapter |
| `daemon/tests/` | Integration tests + golden protocol fixtures |
| `main.rs` | CLI args, `--supervise` parent, shared compute-plane caller (`lokai_app::build_compute_plane`), index watcher, shutdown drain |
| `supervise.rs` | Restart policy (H3-3): crash → backoff → re-exec without `--supervise` |
| `node.rs` | Worker enrollment/serve entrypoints (isolated from coordinator dispatch) |

## RPC surface

`initialize`, `session/start`, `session/end`, `chat/send`, `session/cancel`,
`approval/respond`, `agent/spawn`, `model/list`, `egress/policy.get|set`,
`policy/get|set`, `fabric/status`, `fabric/worker.trust.get|set`,
`project/consolidate`, `run/snapshot`, `run/resume`, estate capacity methods.

Live conversation, cancel, pending approvals, and spawn tracking live in
`lokai-app::SessionLiveStore` (via `DefaultSessionService`). The daemon keeps
transport bookkeeping only (in-flight RPC, draining, capacity-busy).
`chat/send` snapshots saved capacity through `CapacityService` and calls
`admit_chat_turn` — the same kernel path as the CLI. Degraded doctor is a
warning; optimize-in-progress stays `CapacityBusy`.
`session/cancel` submits `CancelRun` through `RunSupervisor`. Session resume
hydrates through `lokai-app::rehydrate_messages` (one implementation; the
daemon has no private copy). Pending live approvals do not survive a crash;
`resume_state: recovery_required` means re-prompt (H3-3). `run/snapshot`
returns the durable journal snapshot; `run/resume` replays events after a
sequence (`after_sequence: 0` means from the start; a positive sequence below
the compaction floor returns a gap).

Named transport/infra exceptions (egress CRUD, worker trust RPC, secrets
handlers, CapacityJobRuntime): see `lokai-app` README **Named exceptions (R26)**.
Enrollment egress reload lives only in `lokai_app::estate_enrollment`.

`fabric/worker.trust.set` is the live M5-3 control path: it updates the
coordinator registry, bumps the policy epoch, invalidates queued dispatches and
capability cache state, and persists an audit row before returning.
The pooled transport also re-reads persisted trust and the global audit epoch at
the final pre-dispatch boundary, covering standalone CLI changes without a
daemon restart. Workspace state and referenced artifact digests are revalidated
against the initialized workspace and runtime artifact store at the same
boundary.

**Shutdown:** `Daemon::shutdown()` drains in-flight turns (5s timeout); there is no `shutdown` RPC yet.

Orchestration: pass `orchestration: "auto"` on `session/start` for keyword routing + critic; use `agent/spawn` for explicit specialist turns. Spawn budgets honor `LOKAI_MAX_SPAWN_PER_TURN` / `LOKAI_MAX_SPAWN_DEPTH` (read once at daemon startup).

## Transport backpressure and failure

Outbound frames pass through `lokai-rpc::OutboundQueue` (bounded, default capacity 512):

| Class | Examples | Under pressure |
|-------|----------|----------------|
| Terminal | JSON-RPC responses (`chat/send`, etc.) | Never dropped; blocks enqueue |
| Coalesce | `event/token` | Latest token per session/agent kept |
| Replace | `event/capacity_progress` | Latest progress replaces pending |
| Lossy | `event/log` | Dropped when queue full |

If stdout is blocked, the writer task stalls; coalesce/replace slots absorb streaming noise. Client disconnect closes the outbound channel; **in-flight application turns are not silently canceled** — they complete unless `session/cancel` or shutdown drain applies. Terminal `event/run_status` is never suppressed by the adapter.

Golden fixtures and compatibility tests: `daemon/tests/protocol_golden.rs`, `daemon/tests/fixtures/`.

## Dependencies

Direct deps are strictly `lokai-app` and `lokai-rpc` (plus external runtime crates). All 17 core engine crates are isolated behind the `Application` facade. Production agents are assembled inside `lokai-app` turn execution via `EngineRuntime::assemble_agent` — not in lokaid handlers. The compute plane is wired by `lokai_app`; the daemon handles stdio RPC dispatch.

## Tests

`cargo test -p lokaid` — in-process tests: RPC auth, streaming, approvals, orchestration, session resume, egress/policy gates, golden protocol fixtures, M0-4 parity. No Ollama required.

Smoke: `engine/scripts/smoke_lokaid.py` (live one-turn run).

## Related docs

- [agent-rpc-v1](../../../docs/implementation/contracts/agent-rpc-v1.md)
- [M1-4 completion evidence](../../../docs/epics/epic-1-app-kernel/sprints/milestone-1/M1-4-completion-evidence.md)
- [../../infrastructure/lokai-rpc/README.md](../../infrastructure/lokai-rpc/README.md)
