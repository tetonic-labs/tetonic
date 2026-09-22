# Memory store — v2 (clean-slate design)

**Status:** **Partially implemented.** The audit spine (`sessions`, `messages`, `tool_calls`, `events`, `egress_log`) ships in Phase A; `file_changes` (undo) and the **time-travel layer** (`checkpoints` + `workspace_head` + `restores`, with undo/redo/restore) ship as the first Phase D slice. `approvals` / `app_state` / encryption-at-rest are designed here but **not yet built**. See "Implementation status" below.
**Owner:** `engine/crates/lokai-memory` (Rust, `rusqlite` bundled). Schema at **v3** (`schema_versions`).
**Supersedes (as design input):** `memory-store-v1` (the Go/Wails prototype's schema). v2 is a **fresh schema** for a coding agent, not a migration of v1.

> **Implementation status (2026-06-27).** Built: schema migrations v1–v3; sessions/messages/tool_calls/events/egress_log; `file_changes` with zstd blobs; mark-based **checkpoints**, the **`workspace_head`** cursor, and a **`restores`** log powering `--undo` / `--redo` / `--restore` in `lokai-cli`. Not yet built: `approvals`, `app_state`, `checkpoint_files`-style snapshots (superseded — see §checkpoints), `zstd-diff` edit encoding (edits currently store `zstd-full`), encryption-at-rest, retention/purge. Shipped `sessions` columns are `mode` + `model` (the `model_tier` / `policy_version` stamps land with the swarm/approval policy).

## Why this exists

A private agent that edits your code must keep a **legible, local, reversible record** of what it did — which sessions ran, what the model said, which tools were called, which files changed, and what the user approved. This is both a trust feature (you can see and undo the agent's actions) and the foundation for later capabilities (checkpoints/undo in Phase D, optional learning much later). It is **entirely on-disk**; there is no network path, by construction.

v2 keeps the *good bones* of v1 (runs / events / messages as the lifecycle spine) and drops the swarm-chat-specific tables (blackboards, channel boards) in favor of **coding-agent** concerns: tool calls, file changes, diffs, approvals, and checkpoints.

## File on disk

This store owns **`lokai.db`** only — the **precious, append-heavy audit/memory**. The code index lives in a **separate, disposable `index.db`** (owned by `lokai-index`, see `code-index-v1`). The split is deliberate (`performance-and-scale` §4.5): the index is read-heavy and **rebuildable**, so it must be nukeable to reclaim space or recover from corruption **without ever risking the audit trail**; the two also have very different access patterns.

| Property | Value |
|---|---|
| Path | `<data_dir>/lokai/lokai.db` (XDG/OS-appropriate; documented + user-purgeable) |
| Sibling | `<data_dir>/lokai/index.db` — **disposable** code index (owned by `lokai-index`; safe to delete) |
| Engine | SQLite via **`rusqlite` with the `bundled` feature** (static, no system lib) |
| Pragmas | `foreign_keys=ON`, `journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout` set |
| Charset | UTF-8 |
| At rest | Optional encryption-at-rest (key in OS keychain); off unless enabled |
| Concurrency (H2-2) | Production uses `SharedStore`: one writer OS thread (`mpsc`), WAL + read-only connection pool; async `.read` via `spawn_blocking`. Do not share a single `Mutex<Store>` across tasks. |

Migrations run on open; the highest applied version is recorded in `schema_versions` (append-only, frozen once shipped — same discipline as v1).

**Write economy.** The audit log is append-heavy and on the agent's hot path, so writes are **batched in transactions** (no per-event fsync), backed by a **prepared-statement cache** and indices on the hot lookups (`(session_id, seq)`). Persistence is best-effort and must never stall the loop (see end of §Repository surface).

## Core tables

### `sessions`
One row per user-initiated agent session (a chat that may span many tool calls). The lifecycle anchor (v1's `runs`, renamed for the editor domain).

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PK | `sess_<hex>` |
| `workspace_root` | TEXT | absolute path of the open project |
| `mode` | TEXT | `single-agent` \| `swarm` |
| `model_tier` | TEXT | requested tier (router/coder/...) |
| `policy_version` | TEXT | stamp of the effort/approval policy in effect |
| `status` | TEXT | `running` \| `ok` \| `error` \| `canceled` |
| `started_at` / `ended_at` | DATETIME | UTC |
| `error` | TEXT | empty unless `status='error'` |

### `messages`
Canonical conversation record.

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | |
| `session_id` | TEXT FK | CASCADE |
| `seq` | INTEGER | monotonic per session; `(session_id, seq)` unique |
| `role` | TEXT | `user` \| `assistant` \| `system` \| `tool` |
| `agent_id` | TEXT | which specialist (swarm); empty for single-agent/user |
| `content` | TEXT | full body |
| `tool_calls_json` | TEXT | assistant tool-call payload, when present |
| `tool_name` | TEXT | tool-result name (H3-1) |
| `tool_call_id` | TEXT | links a tool result to the assistant call id (H3-1) |
| `created_at` | DATETIME | UTC |

### `events`
Append-only structured log of everything that happened in a session. JSON payload so new kinds land without schema changes (kept from v1; kinds re-scoped for coding).

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | |
| `session_id` | TEXT FK | CASCADE |
| `seq` | INTEGER | monotonic; `(session_id, seq)` unique |
| `kind` | TEXT | `message` \| `tool_call` \| `tool_result` \| `file_change` \| `approval` \| `handoff` \| `egress` \| `session_status` |
| `actor` | TEXT | agent id, `user`, or `system` |
| `payload` | TEXT | JSON; readers MUST tolerate unknown keys |
| `created_at` | DATETIME | UTC |

### `tool_calls`
First-class record of each tool invocation (the coding agent's defining activity).

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PK | `tc_<hex>` (matches the `tool_call_id` in `agent-rpc`/`coding-tools`) |
| `session_id` | TEXT FK | CASCADE |
| `tool` | TEXT | `read_file` \| `grep` \| `edit_file` \| `run_shell` \| ... |
| `args_json` | TEXT | the validated args |
| `status` | TEXT | `proposed` \| `approved` \| `denied` \| `ok` \| `error` |
| `result_summary` | TEXT | short, model-facing |
| `error_kind` | TEXT NULL | from `coding-tools` `ToolError` |
| `created_at` / `settled_at` | DATETIME | UTC |

### `file_changes`
Every write the agent applied, with enough to **render and reverse** it.

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | |
| `session_id` | TEXT FK | CASCADE |
| `tool_call_id` | TEXT FK → tool_calls | |
| `path` | TEXT | workspace-relative |
| `change_kind` | TEXT | `edit` \| `create` \| `delete` |
| `blob_encoding` | TEXT | `zstd-diff` \| `zstd-full` \| `raw` — how `before`/`after` are stored |
| `before_blob` | BLOB NULL | prior content (for undo); NULL on create |
| `after_blob` | BLOB NULL | new content; NULL on delete |
| `applied_at` | DATETIME | UTC |

**Blob economy.** `before`/`after` blobs are the heaviest data in the store, so they are **compressed (zstd)**. *Planned:* for `edit` changes, store a **diff** against the prior content (`zstd-diff`) rather than two full copies. *Shipped today:* every side is a `zstd-full` blob (`blob_encoding = 'zstd-full'`); the `blob_encoding` column is already recorded so `zstd-diff` is a drop-in size optimization later. This keeps the precious DB small without losing reversibility.

### Time-travel: `checkpoints`, `workspace_head`, `restores` (implemented)

Instead of snapshotting a file set per checkpoint, the implementation treats **`file_changes.id` as a global, monotonic timeline** and models undo/redo/checkpoint as **cursor movement** over it. This reuses the `before`/`after` blobs already captured for every write — no duplicate snapshots — and it naturally yields **redo** and **cross-session** reach. (This supersedes the earlier `cp_<hex>` + `checkpoint_files(checkpoint_id, path, blob)` snapshot sketch; same one-click-restore capability, far less duplication.)

**Semantics.** Moving the cursor **back** reverts changes via `before_blob` (newest-first); moving **forward** re-applies via `after_blob` (oldest-first). `--undo` steps to the nearest earlier **boundary** (a checkpoint *or* a session start); `--redo` returns to the pre-undo position; `--restore <ref>` jumps to a checkpoint's mark in either direction. A new agent write advances the head and invalidates any pending redo.

`checkpoints` — a **named saved position** (a "mark" = the `file_changes.id` high-water at creation). Workspace-scoped so a checkpoint can span the sessions that ran in a project.

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PK | `ckpt_<hex>` |
| `workspace_root` | TEXT | which project this marks |
| `label` | TEXT | e.g. "before-refactor" |
| `mark` | INTEGER | `file_changes.id` high-water at creation (0 = empty) |
| `kind` | TEXT | `manual` \| `auto` |
| `session_id` | TEXT NULL | creating session, if any |
| `created_at` | DATETIME | UTC |

`workspace_head` — the per-workspace **cursor**: which change is currently applied on disk, plus the mark redo can move forward to. Absent ⇒ head is implicitly the tip (everything applied).

| Column | Type | Notes |
|---|---|---|
| `workspace_root` | TEXT PK | |
| `head_mark` | INTEGER | highest `file_changes.id` currently applied |
| `redo_mark` | INTEGER NULL | forward target set by undo; NULL = nothing to redo |
| `updated_at` | DATETIME | UTC |

`restores` — append-only **audit log** of every undo/redo/restore move.

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | |
| `workspace_root` | TEXT | |
| `from_mark` / `to_mark` | INTEGER | cursor move |
| `reason` | TEXT | `undo` \| `redo` \| `restore` |
| `applied` | INTEGER | number of files written/deleted |
| `created_at` | DATETIME | UTC |

### `approvals`
Audit of every approval decision (what the user allowed and whether they chose to remember it).

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PK | `ap_<hex>` |
| `session_id` | TEXT FK | CASCADE |
| `kind` | TEXT | `run_shell` \| `egress` \| `write` |
| `detail` | TEXT | the command / destination / path shown to the user |
| `decision` | TEXT | `allow` \| `deny` |
| `remembered` | INTEGER | 0/1 — became a persisted rule |
| `decided_at` | DATETIME | UTC |

### `app_state`
Generic key/value (effort policy, default model tier, dismissed onboarding, etc.). Settings only — never project content.

### `egress_log`
Local sink for `EgressEvent` (`egress-guard-v1`) so the Network Activity Panel has history and the privacy gate can audit a session after the fact.

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | |
| `session_id` | TEXT NULL | NULL for daemon-lifecycle traffic |
| `ts` | DATETIME | UTC |
| `initiator` | TEXT | |
| `host` / `resolved_ip` / `port` | TEXT/TEXT/INTEGER | |
| `decision` | TEXT | `allow` \| `deny` |
| `matched_rule` | TEXT NULL | |

### Estate tables (planned — N0.1 / N0.4)

Coordinator-only. See [`estate-v1.md`](./estate-v1.md), [`node-enrollment-v1.md`](./node-enrollment-v1.md).

**`owner_identity`** — one row per local operator estate.

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PK | `estate_<hex>` |
| `label` | TEXT | user-facing |
| `operator_pubkey` | BLOB | signs policy snapshots |
| `created_at` | DATETIME | UTC |

**`worker_enrollments`** — replaces bilateral `NodeCredential` sketch.

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PK | `worker_<hex>` |
| `estate_id` | TEXT FK | → `owner_identity` |
| `label` | TEXT | |
| `addr` | TEXT | host:port at enroll time |
| `worker_pubkey` / `audit_pubkey` | BLOB | pinned |
| `fabric_port` | INTEGER | |
| `coordinator_pubkeys_json` | TEXT | JSON array |
| `enrolled_at` / `last_seen` | DATETIME | |

**`worker_policy_snapshots`** — **not live.** No such table in `lokai-memory`. Do not treat signed worker-policy JSON as a source of truth. A later package may add persist; this is not a current store.

### `fabric_ledger` (planned — N0.4 / L1)

Metadata receipts for all fabric jobs (local + estate + circle). See [`fabric-transport-v1.md`](./fabric-transport-v1.md), [`PRODUCT-PLAN.md`](../../PRODUCT-PLAN.md) § Compute receipt ledger.

| Column | Type | Notes |
|---|---|---|
| `job_id` | TEXT PK | |
| `session_id` | TEXT NULL | |
| `worker_id` | TEXT NULL | local empty |
| `direction` | TEXT | `local` \| `estate_out` \| `estate_in` \| `circle_*` |
| `disclosure_tier` | TEXT | |
| `prompt_tokens` / `eval_tokens` | INTEGER | |
| `duration_ms` | INTEGER | |
| `status` | TEXT | |
| `created_at` | DATETIME | append-only |

Worker headless nodes use a sibling **`worker_ledger`** in **`worker.db`** at `<data_dir>/lokai/worker.db` (same `lokai-memory` crate, separate file — **LD4**).

## Repository surface (illustrative)

```rust
impl Store {
    // Audit spine (implemented)
    fn start_session(&self, ws: &str, mode: &str, model: &str) -> Result<String>;
    fn end_session(&self, id: &str, status: &str, err: Option<&str>) -> Result<()>;
    fn append_message(&self, id: &str, role: &str, agent: &str, body: &str,
                      tool_calls_json: Option<&str>) -> Result<i64>;
    fn append_event(&self, id: &str, kind: &str, actor: &str, payload: &str) -> Result<i64>;
    fn record_tool_call(&self, ...) -> Result<()>;       // settles in one shot today
    fn record_file_change(&self, ...) -> Result<()>;     // zstd before/after; advances head
    fn record_egress(&self, ev: ...) -> Result<()>;
    fn list_recent_sessions(&self, limit: u32) -> Result<Vec<SessionRow>>;
    fn transcript(&self, id: &str) -> Result<Vec<(i64, String, String)>>;

    // Time-travel (implemented)
    fn current_head(&self, ws: &str) -> Result<i64>;     // cursor, or tip if none
    fn head_state(&self, ws: &str) -> Result<Option<(i64, Option<i64>)>>; // (head, redo)
    fn set_head(&self, ws: &str, head_mark: i64, redo_mark: Option<i64>) -> Result<()>;
    fn create_checkpoint(&self, ws: &str, label: &str, kind: &str,
                         session: Option<&str>) -> Result<(String, i64)>;
    fn list_checkpoints(&self, ws: &str) -> Result<Vec<CheckpointRow>>;
    fn find_checkpoint(&self, ws: &str, reference: &str) -> Result<Option<CheckpointRow>>;
    fn previous_boundary(&self, ws: &str, cur: i64) -> Result<i64>; // undo target
    fn workspace_changes_in_range(&self, ws: &str, lo_excl: i64, hi: i64)
        -> Result<Vec<FileChangeRow>>;                  // the deltas to apply
    fn record_restore(&self, ws: &str, from: i64, to: i64, reason: &str, applied: i64) -> Result<()>;

    // Planned: record_approval, app_state get/set, retention/purge.
}
```

> The cursor *bookkeeping* (which marks to apply, undo/redo direction, redo invalidation) lives in `lokai-cli`'s `restore_to`; the store provides the timeline primitives above. This keeps the store a thin, testable persistence layer.

The engine degrades gracefully if the store is unavailable (the agent still runs; the audit trail is best-effort and surfaced as a warning) — persistence failure must never block the user.

## Retention & deletion

- **User-driven purge** (Phase D, not yet built): delete a session (cascades events/tool_calls/file_changes via FK), or wipe all history. One-click "forget everything." Note checkpoints/`workspace_head`/`restores` are **workspace-scoped** (not session-FK'd), so a per-workspace purge clears them together with that project's timeline.
- **Cap-based pruning** (later): rotate `events`/`egress_log` past a row cap with consent; `before_blob`/`after_blob` (in `file_changes`) are the heaviest data — old changes past a checkpoint horizon may shed blobs while keeping metadata, at the cost of restorability before that horizon.
- Until then growth is bounded mostly by file blobs; metadata stays small.

## Privacy posture

- 100% local SQLite; **no network path** from this crate at all (it never depends on `lokai-egress`).
- Stores **project content** (messages, file blobs) — which is exactly why it stays on-device, is purgeable, and is optionally encrypted-at-rest.
- The DB is **never** transmitted, synced, or included in any export that leaves the machine.

## Compatibility & extensibility

- `schema_versions` is append-only; shipped migrations are frozen.
- Tables are additive within v2; `events.payload` / `tool_calls.args_json` are JSON to absorb new shapes without migrations.
- This is a clean v2 (not migrated from the prototype's v1); the prototype DB is not read.

---

## Capacity plane (ES5, schema v10–v11)

Runtime inference profiles and optimize jobs. Owned by `lokai-capacity`; persisted in `lokai-memory`.

### `runtime_profiles` (append-only)

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PK | `profile_<hex>` |
| `node_id` | TEXT | estate node (`local` on workstation) |
| `role` | TEXT | `coder` \| `fast` \| `hard` \| `embed` |
| `fingerprint` | TEXT | hardware identity at bench time |
| `created_at` | TEXT | UTC RFC3339 |
| `gates_passed` | INTEGER | 0/1 |
| `json` | TEXT | full `RuntimeProfile` document |

### `capacity_bindings` (active pointer)

| Column | Type | Notes |
|---|---|---|
| `node_id` | TEXT | PK (with `role`) |
| `role` | TEXT | PK |
| `active_profile_id` | TEXT | FK → `runtime_profiles.id`, nullable |
| `updated_at` | TEXT | UTC |

### `capacity_jobs` (optimize job log, v11)

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PK | `capjob_<hex>` |
| `state` | TEXT | `queued` \| `running` \| `succeeded` \| `failed` \| `cancelled` \| `interrupted` |
| `json` | TEXT | job options or outcome JSON |
| `updated_at` | TEXT | UTC |

Running jobs are marked `interrupted` on daemon restart.

---

## Mission alignment

| Principle | How honored |
|---|---|
| Inspectable & reversible | Tool calls, file changes (before/after), checkpoints, approvals — every action is logged and undoable. |
| Private by architecture | On-disk only; the one crate that holds project content has zero network capability. |
| Sovereignty | User-purgeable, optionally encrypted, never exported off-box. |
| Capable | Checkpoints + file_changes power undo and let the verification loop reason about what changed. |

---

**Last updated:** 2026-06-27 (resynced to the implementation: schema v1–v3 shipped; **mark-based time-travel** — `checkpoints` + `workspace_head` + `restores` with undo/redo/restore — supersedes the per-checkpoint file-snapshot sketch; repository surface and status reconciled with `engine/crates/lokai-memory`).
