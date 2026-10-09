# tetonic-memory

Tetonic's durable SQLite store: control records, execution journals, scoped
context, usage accounting, artifact bindings and audit history. Application
services authorize use cases; `tetonic-run` owns lifecycle transitions; this
crate enforces transactional integrity. It has no network client.

## Where changes belong

| Area | Contents |
|---|---|
| [`control/`](src/control/mod.rs) | Organizations and membership, agent identities/revisions, grants, work intent, plans, approvals/stops, workspace skills/MCP, presentation metadata and estate enrollment |
| [`execution/`](src/execution/mod.rs) | Run event/projection transactions, worker trust pins/TLS and workstation placement records |
| [`context/`](src/context/mod.rs) | Scoped access, transcript reads, publication, recall and retained project context |
| [`usage/`](src/usage/mod.rs) | Work allocations, attempt reservations, provider usage, settlement/resume accounting, execution capacity and inference compute reservations |
| [`artifacts/`](src/artifacts/mod.rs) | Artifact/context ownership bindings and result dispositions; payloads live in `tetonic-artifact` |
| [`lib.rs`](src/lib.rs) | `Store`, `SharedStore`, public compatibility exports, base session/audit APIs, file-change and checkpoint records |
| [`schema.rs`](src/schema.rs), [`backup.rs`](src/backup.rs) | Schema upgrades through v72, migration ownership, verified backups and restore |
| `blob`, `payload_digest`, `sync_lock`, `util` | Bounded audit blobs, canonical event digests, locking, IDs/timestamps and workspace storage keys |

These are module boundaries within the existing store, not independent services
or databases. Public row types and `Store` methods remain available through the
crate root. Keep cross-area transaction helpers inside the owning transaction;
do not replace one atomic operation with several writer calls.

## Durability and concurrency

[`SharedStore`](src/lib.rs) provides a dedicated writer thread and queue, WAL,
and a pool of read-only connections. Async `.write` and `.read` calls keep SQL
off the async executor; reads use `spawn_blocking`. Sync helpers remain available
for synchronous callers. Store waits emit `tetonic-telemetry` metrics.

Persistent audit and worker connections explicitly use WAL with
`synchronous=FULL`; acknowledgment follows SQLite's commit sync. Connections also
request `fullfsync` and `checkpoint_fullfsync` where supported. Reopened connections
retain that policy. In-memory stores are ephemeral. Process-exit tests do not
certify physical power-loss behavior or hardware that ignores flushes.

The run-command transaction commits events, projection, receipts, capacity and
any permitted accounting-fence transfer together. Provider reporting and usage
settlement are distinct transactions. External artifact payloads and tool effects
are not atomic with SQLite. Unknown provider usage retains its reservation;
requesting cancellation does not establish quiescence or refund funds.

Current local-workspace execution requires durable storage. Some lower-level
audit/testing APIs still permit optional or in-memory stores. Worker persistence
and placement records do not provide replicated control storage or distributed
agent execution.

## Contracts and validation

- [Durable state, atomicity, budget trace and public projections](../../../docs/architecture/durable-state-and-contracts.md)
- [Current system ownership](../../../docs/architecture/ownership.md)
- [Migration and recovery](MIGRATION-RECOVERY.md)

From `engine`, run `cargo test -p tetonic-memory`. Coverage includes schema
rollback/process exit, durable reopen, backup/restore, writer ordering, scoped
access, exact retries, competing allocations, pending/unknown usage, quiescence,
lease fences and resume accounting. Grouped tests exercise the same public
methods; the directory move adds no migration and changes no SQL.
