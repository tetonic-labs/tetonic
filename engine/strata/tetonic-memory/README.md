# lokai-memory

Local-only audit and memory store (`lokai.db`): sessions, transcripts, tool calls, file changes, checkpoints, egress mirror, estate enrollment, project memory.

## Role in the stack

Optional but expected in production: `tetonicd` and `tetonic-cli` attach an `AuditSink` backed by `Store`. Time-travel (checkpoint/undo/redo) reads the file-change timeline from here.

Production sharing uses [`SharedStore`](src/lib.rs) (H2-2): a dedicated writer thread + `mpsc` queue, WAL, and a pool of read-only connections. Async callers use `.write` / `.read` (reads run in `spawn_blocking`). Store wait times export as `store.wait` / `store_op` via `lokai-telemetry`.

## Modules

Persistent audit and worker connections explicitly use WAL with
`synchronous=FULL`; acknowledgment follows SQLite's commit sync. Connections also
request `fullfsync` and `checkpoint_fullfsync` for platforms that support stronger
flushes. Reopened connections and the shared writer retain this policy. This
trades additional commit latency for stronger durability. It cannot compensate
for a filesystem/device that does not honor flushes, and does not make database
receipts atomic with external artifact or workspace publication. In-memory stores
are ephemeral. Process-exit regression tests do not certify physical power loss.

| Module | Purpose |
|--------|---------|
| `lib.rs` | `Store`, `SharedStore`, session/message/tool APIs |
| `schema.rs` | Atomic migrations v1–v28; version markers commit with schema/data changes |
| `run_store.rs` | Run projections/events; `compact_run_events_at_floor` + recovery snapshot (R25) |
| `secret_overrides.rs` | Durable scoped fingerprint allows + grant/revoke audit (R12) |
| `backup.rs` | Verified, versioned immutable snapshots and upgrade ownership |
| `sync_lock.rs` | Poison-tolerant `mutex_lock` / `RecoverMutex` (H3-3 coordinator hot paths) |
| `result_disposition.rs` | Durable M5-4 result dispositions + worker behavior signals |
| `compute_reservation.rs` | Durable M6-1 compute reservations |
| `blob.rs` | zstd file-change blobs (bounded, strict UTF-8) |
| `util.rs` | IDs, timestamps, workspace storage keys |
| `capacity_tables.rs` | Shared capacity-plane DDL/SQL |
| `projects.rs` | Project digest, notes, consolidation (D4) |
| `recall.rs` | FTS5 episodic recall (T8 / M8 v1) |
| `estate.rs` | Owner identity, worker enrollment, versioned trust + audit rows |
| `worker_store.rs` | Worker-side coordinator pins |
| `policy.rs` | Persisted policy mode |

## Key API

- Session: `start_session`, `end_session`, `append_message`, `record_tool_call`, `session_file_change_highwater`
- Operational (AC2-6): `upsert_turn_operation`, `get_turn_operation`, `clear_turn_operation`
- Time travel: `checkpoint`, `undo`, `redo`, `restore`, `list_checkpoints`
- Projects: `ensure_project`, `load_project_context`, `add_project_note`, `consolidate_session`
- Recall: `recall_history`, `recent_finish_outcomes`
- Trust: `record_approval`, `get_approval`, `add_approval_rule`, `approval_rule_matches`, `propose_tool_call`, egress allowlist CRUD
- Estate: `list_worker_enrollments`, enrollment CRUD, `set_worker_trust`,
  `worker_trust`, `list_worker_trust_audit`, `max_worker_trust_policy_epoch`
- Egress mirror: `record_egress`
- Concurrency: `SharedStore::open` / `.write` / `.read` (async) and `*_sync` for sync helpers only

## Dependencies

`lokai-domain`, `lokai-policy`, `lokai-telemetry` (store-wait metrics). No network crates.

## Product plan

| ID | Feature | Status |
|----|---------|--------|
| M1 | Audit spine (`lokai.db`) | Done |
| M3 | File-change blobs | Done |
| M4 | Checkpoints / undo / redo | Done |
| M6 / D4 | Project memory | Partial |
| T8 / M8 | Episodic recall (FTS) | Partial |
| N0 | Worker enrollment rows | Done |
| M5-3 | Worker trust persistence and audit epochs | Done |
| H2-2 | Writer actor + WAL read pool + store-wait metric | Done |
| R12 | Durable scoped secret fingerprint overrides | Done |
| R25 | Compaction replay floor + recovery snapshot | Done |

See [migration and recovery](MIGRATION-RECOVERY.md) for backup locations, upgrade failure behavior, and the tested restore workflow.

## Tests

`cargo test -p lokai-memory` — schema migrations (v1–v28), injected SQL failures and abrupt process exits, immutable backup/restore, disposition idempotency, checkpoint timeline,
project digest, estate/trust CRUD and audit, policy mode, blob bounds, workspace-key
canonicalization (H4-1: plain and Windows `\\?\` paths share one key), poison-tolerant mutex recover,
H2-2 concurrent FTS vs append (no HOL), WAL reopen after abrupt drop, write FIFO ordering.

## Related docs

- [memory-store-v2](../../../docs/implementation/contracts/memory-store-v2.md)
- [memory-and-context-v1](../../../docs/implementation/memory-and-context-v1.md)
