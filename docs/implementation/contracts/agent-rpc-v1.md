# Agent RPC — v1 (clean-slate design)

**Status:** **Implemented (server, v1 subset).** The single boundary between the Rust daemon and the editor. The Rust side (framing + protocol + daemon dispatch + streaming + approvals) is built and tested; the TS client lands with the fork (Phase B.2).
**Owner:** `engine/crates/lokai-rpc` (transport + protocol types) + `engine/bins/lokaid` (dispatch/wiring) + the TS client inside the Code-OSS fork.
**Relates to:** `pivot-agentic-code-editor.md` §4.1; tool payloads defined in `coding-tools-v1` (TBD); network events from `egress-guard-v1`.

## Why this exists

The engine (Rust) and the editor (TypeScript/Electron) are two processes in two languages. They need exactly **one** well-defined boundary so that (a) the engine stays UI-agnostic and is reusable by a CLI, (b) the contract is small enough to audit, and (c) **the boundary itself never carries data off the machine**.

The boundary is **stdio**: the editor spawns the daemon (`lokaid`) and speaks JSON over its stdin/stdout. There is **no listening socket, no port, no auth surface, no network**. This is a deliberate privacy property — the most sensitive channel in the system (full project context flowing to the agent) physically cannot leave the box because it travels over a pipe, not a socket.

## Transport & framing

- **Channel:** daemon `stdin` (requests in) / `stdout` (responses + notifications out). `stderr` is reserved for human-readable diagnostics/logs (never structured protocol).
- **Framing:** LSP-style `Content-Length` headers + `\r\n\r\n` + UTF-8 JSON body. (Chosen over newline-delimited JSON because messages stream, are large, and contain multi-line code/diffs; the editor's stack already speaks this framing for LSP.)
- **Message model:** JSON-RPC 2.0 — `request` (has `id`, expects a `response`), `notification` (no `id`, fire-and-forget), and `response` (`result` or `error`).

```
Content-Length: 134\r\n
\r\n
{"jsonrpc":"2.0","id":1,"method":"chat/send","params":{...}}
```

## Versioning & type generation

- The first call MUST be `initialize`, which negotiates `protocol_version` (integer). v1 is additive: new methods/params/notifications may be added; both sides MUST tolerate unknown fields and ignore unknown notifications.
- **Single source of truth:** Rust types derive `schemars::JsonSchema`; the build emits JSON Schema, from which the editor's **TypeScript types are codegen'd**. Hand-written drift between the two sides is disallowed.

**Codegen bridge (implemented).** The pipeline has two stages, both derived from the Rust types so the editor cannot drift from the daemon:

1. `lokai_rpc::schema_bundle()` collects every request-param/result and notification-payload type into one shared `definitions` map (one `schemars` generator, so each type appears once and `$ref`s resolve), plus the wire-name constants (`methods`, `events`) and stable `error_codes`. `lokaid --print-schema` prints it to stdout and exits (no server loop).
2. `engine/scripts/gen_ts_protocol.py` turns that bundle into `engine/clients/ts/protocol.ts` — interfaces for structs, string-union aliases for enums, `?`/`| null` for optionals, and `as const` maps for `Methods`/`Events`/`ErrorCodes`. Output is deterministic; `--check` exits non-zero if the checked-in file is stale (CI gate).

```bash
# regenerate (from engine/)
cargo run -q -p lokaid -- --print-schema | python scripts/gen_ts_protocol.py
# verify up-to-date (CI)
cargo run -q -p lokaid -- --print-schema | python scripts/gen_ts_protocol.py --check
```

The generated `protocol.ts` lives under `engine/clients/ts/` — the home for the future editor JSON-RPC client (Phase B.2). The Python generator is a zero-dependency stopgap so the bridge works before the Node toolchain lands; it can be swapped for a standard JSON-Schema→TS tool later without changing stage 1.

## Methods (editor → daemon, `request`)

| Method | Params (summary) | Result |
|---|---|---|
| `initialize` | `{ protocol_version, workspace_root, rpc_token, client_info? }` | `{ protocol_version, daemon_info, capabilities, rpc_token? }` |
| `session/start` | `{ goal?, model_tier?, verify_cmd?, resume?, session_id? }` | `{ session_id, data_class, resumed?, messages_loaded?, resume_state? }` |
| `session/end` | `{ session_id, status?, error? }` | `{ ok: bool }` |
| `chat/send` | `{ session_id, text, attachments? }` | `{ accepted: true }` (work streams as notifications) |
| `session/cancel` | `{ session_id }` | `{ canceled: bool }` |
| `run/snapshot` | `{ run_id }` | `{ run_id, sequence, state, snapshot }` — durable `RunSupervisor` snapshot |
| `run/resume` | `{ run_id, after_sequence, limit? }` | `{ run_id, events, gap? }` — journal replay; `after_sequence: 0` is from the start; a positive sequence below the compaction floor returns `gap` |
| `approval/respond` | `{ session_id, approval_id, decision, remember? }` | `{ ok: true }` |
| `fabric/status` | `{}` | `FabricSnapshot` (see `inference-fabric-v1`) |
| `fabric/worker.trust.get` | `{ worker_id }` | `{ worker_id, trust, policy_epoch, audit[] }` |
| `fabric/worker.trust.set` | `{ worker_id, trust }` | `{ ok, worker_id, trust, policy_epoch }` — applies live, invalidates queued dispatches, and persists an audit row |
| `egress/policy.get` | `{}` | `EgressPolicy` (see `egress-guard-v1`) |
| `egress/policy.set` | `{ add?, remove? }` | `{ ok, allow }` — **disabled over stdio by default** (AR1-1); use `lokai estate egress allow` or `LOKAI_ALLOW_RPC_EGRESS=1` for dev |

### `LOKAI_STRICT_RPC` (SEC2-E2-006)

When `LOKAI_STRICT_RPC=1`, the editor/daemon stdio RPC session is **read-mostly** for trust-sensitive control plane:

| Allowed | Blocked |
|---------|---------|
| `session/start`, `chat/send`, `session/cancel`, approvals | `egress/policy.set` |
| `policy/get`, `fabric/status`, `fabric/worker.trust.get`, `model/list` | `policy/set`, `fabric/worker.trust.set` |
| `estate/capacity/status`, `doctor`, profile list/export | `estate/capacity/optimize`, `profiles/activate`, `profiles/rollback` |

Use the `lokai` CLI for egress allow rules, policy mode changes, and capacity profile activation. This is the recommended default for CI harnesses and unattended editor attach.
| `model/list` | `{}` | `{ models: [...], tool_capable: bool, ... }` |
| `estate/capacity/status` | `{}` | `CapacitySummary` |
| `estate/capacity/doctor` | `{}` | `CapacityDoctorResult` |
| `estate/capacity/optimize` | `{ depth?, auto_apply? }` | `{ job_id, accepted }` |
| `estate/capacity/cancel` | `{ job_id? }` | `{ cancelled }` |
| `estate/capacity/profiles/list` | `{ node_id?, role? }` | `{ profiles: [...] }` |
| `estate/capacity/profiles/activate` | `{ profile_id, node_id?, role? }` | `{ ok, profile_id, status }` |
| `estate/capacity/profiles/rollback` | `{ node_id?, role? }` | `{ ok, profile_id, status }` |
| `estate/capacity/profiles/export` | `{ profile_id }` | `{ profile }` |
| `estate/capacity/jobs/get` | `{ job_id }` | `{ job_id, state, json? }` |
| `estate/capacity/jobs/cancel` | `{ job_id? }` | `{ cancelled }` |
| `secret/rule/add` | `{ pattern, description }` | `{ status }` — in-memory custom detector (not durable) |
| `secret/fingerprint/allow` | `{ fingerprint, scope_kind?, scope_id?, durable? }` | `{ status, override_id? }` — R12 scoped allow; durable defaults true and persists in `lokai.db` |
| `secret/fingerprint/revoke` | `{ fingerprint, scope_kind?, scope_id? }` | `{ status, revoked_durable }` — clears memory + durable row |
| `shutdown` | `{}` | `{ ok: true }` then best-effort drain of in-flight turns (≤5s) and daemon exit |

- `chat/send` returns immediately; the actual run is delivered as a stream of notifications correlated by `session_id`. This keeps the UI responsive and matches how agentic loops actually behave (long, multi-step).
- `approval/respond.remember` lets the user create a session-scoped or persisted allow rule (e.g. "always allow `cargo test`").
- `session/start.verify_cmd` (optional) configures a **verify-before-finish** gate: when the agent calls `finish`, the daemon runs this command in the workspace; on non-zero exit the captured output is fed back and the agent must fix the problem and finish again. It is a trusted, host-supplied allowlist entry (not the model-driven `run_shell` path), so default-deny egress is unaffected.
- `session/start.resume` (optional, default false) reopens the latest audit session for the workspace (or `session_id` when set) and loads up to 200 **most recent** persisted messages into the working conversation (chronological order). Response includes `resume_state`: `fresh` (new session), `continued` (resumed after clean end), `incomplete` (resumed while prior status was still `running` with no operational row), or `recovery_required` (resumed with an in-flight operational turn row in `awaiting_approval`, `executing`, or `verifying`). Compaction state (`prefix_len`) is **not** restored — audit is the source of truth for transcript history only. **Pending live approvals do not survive a crash** (oneshots in `SessionLiveStore`). `recovery_required` means re-prompt; do not send `approval/respond` for a pre-crash `approval_id`.
- `session/end` closes the live session, persists terminal status to `lokai.db`, and removes the in-memory session handle.

## Notifications (daemon → editor)

All carry `session_id` and a monotonic `seq` (per session) so the editor can order/replay.

| Notification | Payload (summary) | Editor renders as |
|---|---|---|
| `event/run_status` | `{ status: started\|ok\|error\|canceled, error? }` | run lifecycle / spinner |
| `event/token` | `{ delta, role, agent_id? }` | streaming assistant text |
| `event/tool_call` | `{ tool_call_id, tool, args, phase: proposed\|started }` | "Agent is reading X / running Y" |
| `event/diff` | `{ tool_call_id, path, before?, after, kind: edit\|create\|delete }` | inline diff to accept/reject |
| `event/approval_request` | `{ approval_id, kind: run_shell\|egress\|..., detail, missing_controls?: [{control, risk_level, reason}], user_approval_required?: bool }` | blocking approval prompt; confinement gaps are typed, not a pre-formatted string |
| `event/tool_result` | `{ tool_call_id, ok, summary, error? }` | tool outcome / error |
| `event/egress` | `EgressEvent` (see `egress-guard-v1`) | **Network Activity Panel** |
| `event/plan` | `{ steps: [...] }` | optional plan/todo view |
| `event/log` | `{ level, message }` | dev console only |
| `event/capacity/progress` | `{ job_id, phase, percent, message }` | capacity optimize job progress |

### Approval flow (safety-critical)

Destructive or networked actions never execute on the model's say-so:

1. Daemon emits `event/approval_request { approval_id, kind, detail, missing_controls?, user_approval_required? }` and **pauses** that tool.
2. Editor shows the prompt (e.g. the exact shell command, plus any typed OS-confinement gaps). When `user_approval_required` is true, a remembered allow-rule must not auto-approve.
3. Editor calls `approval/respond { approval_id, decision }`.
4. Daemon proceeds or feeds a "denied by user" tool result back into the loop (the model self-corrects).

`run_shell` and any egress that requires a new allow rule are **always** gated this way. Read-only tools (`read_file`, `grep`, `glob`, `list_dir`) may be auto-approved per the approval policy.

## Example flow

```
editor → initialize            → result { capabilities }
editor → session/start         → result { session_id: "s1" }
editor → chat/send {s1,"add a test for parse_url"}
                               → result { accepted:true }
daemon → event/run_status {s1, started}
daemon → event/token {s1,"I'll look at the parser..."}
daemon → event/tool_call {s1, tc1, grep, {pattern:"fn parse_url"}, started}
daemon → event/tool_result {s1, tc1, ok, "3 matches"}
daemon → event/diff {s1, tc2, "tests/url.rs", create, "<new test>"}
daemon → event/approval_request {s1, ap1, run_shell, "cargo test url"}
editor → approval/respond {s1, ap1, allow}
daemon → event/tool_result {s1, tc3, ok, "test passed"}
daemon → event/run_status {s1, ok}
```

## Error model

- JSON-RPC `error` objects use stable integer `code`s (a small enum defined in `lokai-rpc`) plus a human `message` and optional structured `data`.
- Transport/protocol violations (bad framing, unknown required field on `initialize`) are fatal; mid-session tool errors are **not** — they flow as `event/tool_result { ok:false }` so the agent loop can recover.

## Security & privacy notes

- **stdio only.** No network listener; the optional loopback-HTTP debug transport is behind a build flag and bound to `127.0.0.1`, never shipped enabled.
- **RPC session token (AR2-2):** `lokaid` prints `LOKAI_RPC_TOKEN=<hex>` on stderr at startup. The editor MUST pass this as `initialize.rpc_token`. All other methods are rejected with `-32002` until a successful `initialize`. Set `LOKAI_RPC_INSECURE=1` only for dev/tests. This blocks casual same-user stdin injection but is not a secret from a determined same-user attacker who can read stderr.
- The RPC layer carries project content **between local processes only**; it is never a path off the machine. Any actual egress happens behind `egress-guard-v1`, which is independently logged and surfaced over `event/egress`.
- The daemon treats the editor as trusted once authenticated (same user, same machine); the defended boundary is the network edge (the Egress Guard).

## Implementation status (current)

Built in `engine/crates/lokai-rpc` (transport + types) and `engine/bins/lokaid` (the daemon):

- **Transport:** `Content-Length` framing over stdin/stdout (32 MiB max frame); one writer task owns stdout, logs go to stderr (stdout carries only protocol bytes).
- **Concurrency:** single-threaded tokio runtime + `LocalSet`. stdin stays responsive during a run, so `session/cancel` and `approval/respond` are handled mid-run; each `chat/send` runs as a `spawn_local` task. The engine's `!Send` audit sink needs no thread-safety ceremony.
- **Methods implemented:** `initialize`, `session/start`, `session/end`, `chat/send`, `session/cancel`, `approval/respond` (**`remember`** persists allow rules), `model/list`, **`egress/policy.get`** (live guard rules), **`egress/policy.set`**, **`fabric/status`** (local node includes `capacity` health), **`fabric/worker.trust.get|set`** (M5-3 live trust + audit), **`agent/spawn`**, **`project/consolidate`**, **`run/snapshot`**, **`run/resume`**, `policy/get|set`, **`estate/capacity/*`** (status, doctor, optimize, profiles, jobs), `shutdown` (still NotImplemented).
- **Session authority:** live conversation, cancel, pending approvals, and spawn tracking live in `lokai-app::SessionLiveStore`. `session/cancel` submits `CancelRun` through `RunSupervisor`. Granted/denied approvals persist in the audit store (`lokai.db`); pending live approvals do not survive process death (H3-3). The run journal is the authority for task/attempt progress.
- **`session/start` orchestration:** optional `orchestration: "auto"` enables the keyword router + specialist roles; optional `critic: true|false` (defaults on when orchestration is auto). Default remains single-agent (`a0`).
- **Orchestration events (v5):** `event/log` includes router + post-turn orchestration summary (v4). Spawn `tool_result` content is compact JSON `{ summary, pointers, agent_id }` — child transcript folded out of parent context. `session/start.llm_router` or `LOKAI_LLM_ROUTER` enables LLM routing. Spawn budget: `LOKAI_MAX_SPAWN_PER_TURN` (default 4), `LOKAI_MAX_SPAWN_DEPTH` (default 2). Hard tier prefers enrolled fabric workers when model is resident remotely. Post-turn **session consolidate** (D4) on successful turns via `SessionHost::on_turn_end`. **`project/consolidate`** forces digest merge on demand.
- **Continuity (E2):** Agent advertises **`recall`** when audit store is wired. Egress decisions are mirrored to `egress_log` per turn (in addition to `event/egress` stream).
- **Notifications implemented:** `event/run_status`, `event/token`, `event/tool_call`, `event/tool_result`, `event/diff`, `event/approval_request`, `event/egress`, `event/log`, `event/capacity/progress`, and an additive `event/context` (the Context Inspector's data source, Phase D).
- **Approvals:** `run_shell` is gated through an async approval hook in the agent loop; the daemon emits `event/approval_request` and pauses on a `oneshot` until `approval/respond` (a dropped channel / cancel resolves to deny — the loop never hangs). Read-only tools are not gated.
- **Egress:** the daemon snapshots the guard's activity log around each run, **persists to `egress_log`**, and streams the delta as `event/egress`.
- **Not yet implemented:** `shutdown` returns `NotImplemented` (-32000). Reserved namespaces: **`estate/*`** (**B11**), **`circle/*`** + `event/plan` (**B12**). Full Circle **D1** policy mode beyond estate stub.
- **Tested:** framing round-trip + protocol (de)serialization unit tests; an in-process integration test drives the full dispatch + notification stream (and the approval deny round-trip) with a mock provider — **no Ollama required**. A live smoke client (`engine/scripts/smoke_lokaid.py`) drives a real one-turn run.

### Error codes (stable)

Standard JSON-RPC (`-32700` parse, `-32600` invalid request, `-32601` method not found, `-32602` invalid params, `-32603` internal) plus the app range: `-32000` not-implemented, `-32001` unknown session, `-32002` not-ready (no `initialize` yet).

## Forward-compat: harness / sub-agent model (Phase C — partial)

Router + specialists + critic (D11–D12) and explicit spawn (A13) are **partially implemented** in `lokaid`:

- **`agent_id` on every notification.** Child specialists get ids like `a0_s0`, `a0_s1`; streaming events carry the active agent's id.
- **`chat/send` with `orchestration: auto`:** shared `run_orchestrated_turn`. Router v5: keyword + optional LLM (`session/start.llm_router` or env); hard tier → `model_hard` + fabric placement preference. Compact spawn handoff; nested spawn to depth 2 with carved budgets. Briefing v3 index outlines.
- **In-loop `spawn_agent` tool (A13):** when orchestration is auto and the router keeps the root agent (`a0`), the root advertises `spawn_agent` (`role`, `task`). The host runs `run_spawned_specialist` in-process; child steps stream under `a0_sN` ids.
- **`agent/spawn` RPC:** `{ session_id, role, task, parent_agent_id? }` → `{ agent_id, accepted }`. Same specialist turn path as the tool, invoked explicitly by the client.
- **Compact handoff shape (documented, not wired):** a child is invoked with `{ goal, role, tool_subset, budget, context_pointers }` and returns `{ summary, pointers }` — pointers into the index/memory, **never raw transcripts** (§4.4). The effort cap is treated as a splittable budget so child budgets can be carved from the parent.
- **Shared `EngineServices`.** The hot handles (provider, egress guard, tools/workspace, index, store) are grouped so a child agent is cheap to spawn — clone the `Arc`s, reuse the already-resident model (no swap), new conversation + persona + budget.

## Forward-compat: in-network expansion (control plane vs compute plane)

The product must scale from one workstation to an owned cluster on the user's secure LAN **without changing this contract**. The design that guarantees that is a clean split into two boundaries:

```
[ Editor ] --stdio JSON-RPC (local pipe)--> [ lokaid coordinator ]  ==enrolled mTLS, inference only==>  [ worker nodes ]
              (this contract; invariant)        control plane + data custody                                compute plane (fungible)
```

- **Boundary 1 — editor ↔ daemon (this contract).** stdio, local, no socket. It is **invariant under cluster expansion**: the editor always talks only to its co-located coordinator. The richest data channel stays a pipe, never a network socket.
- **Boundary 2 — daemon ↔ worker (added by cluster mode, not here).** Enrolled, mutually authenticated, `EgressGuard`-mediated, and carrying **inference traffic + fabric control only** — never project files. Defined in [`node-enrollment-v1`](./node-enrollment-v1.md), [`estate-v1`](./estate-v1.md), [`fabric-transport-v1`](./fabric-transport-v1.md), and [`inference-fabric-v1`](./inference-fabric-v1.md).

What stays on the coordinator daemon (the **control plane + data custodian**), regardless of cluster size: the workspace, tool execution, the audit store, approvals, and egress policy. What disperses (the **compute plane**): inference, behind the `InferenceProvider` trait — and, because sub-agents are orchestrated in-process on the coordinator, parallel swarm branches simply land their *model calls* on different nodes (`FabricSnapshot.effective_concurrency`) without moving live run-state across machines.

How this stays additive in the implementation:

- The daemon already holds the provider as `Arc<dyn InferenceProvider>` and constructs it in **one place** (`build_compute_plane` in `lokaid`) — the sole swap point for a `PooledProvider`/`ClusterRuntimeProvider`.
- `lokaid` reserves the `--node` (worker) execution mode now (errors until Phase F), so adding it is a branch, not a startup rewrite.
- `fabric/status` and `egress/policy.set` are reserved methods (return `NotImplemented` today); they surface the fabric topology and enrollment without a protocol bump. **`estate/worker/list`**, **`estate/ledger`**, **`circle/discover`** (names TBD) land additively in D3/N2. A future `node_id` may be added to notifications alongside `agent_id`.
- Every off-box byte already flows through the single `EgressGuard` and is surfaced as `event/egress`, so dispersing work cannot create an unaudited path.

**Scope choice (deliberate):** v1 disperses **inference only**; the workspace and tool execution stay on the coordinator. Shipping whole agent loops (file/tool execution) to workers would require moving project content and live run-state off-box and is intentionally excluded; any future "remote build/test execution" would be a separate, explicitly enrolled capability — not a change to this contract.

## Compatibility & extensibility

- Additive-only within v1: new methods, params, and notifications may land; consumers ignore unknowns.
- Breaking changes bump `protocol_version`; `initialize` lets an older editor and newer daemon (or vice versa) detect mismatch and degrade gracefully.

## Mission alignment

| Principle | How honored |
|---|---|
| Private by architecture | The richest data channel is a local pipe, not a socket — it *cannot* leak. |
| Capable | Async, streaming, approval-aware protocol matches real agentic loops. |
| Verifiable | One small, schema-generated contract is auditable end-to-end. |
| Durable | Versioned + additive; the CLI and the fork share the same daemon and protocol. |

---

**Last updated:** 2026-06-27 (server implemented: framing, protocol, daemon dispatch, streaming, approvals, cancel; harness/sub-agent seams in place; TS client + `egress/policy.set`/`fabric/status` pending).
