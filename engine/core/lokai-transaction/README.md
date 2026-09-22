# lokai-transaction

Workspace-versioned staging, conflict detection, cross-process writer lock, and journal-backed commit/recovery (M2-4 / R6-3).

## Role

`WorkspaceTransactionService` is the production mutation path used by `lokai-tools` (`RepositoryMutationService`). Writes stage under `.lokai/staging/`, commit under an exclusive `writer.lock`, and apply through a durable journal in `.lokai/journals/`.

## Key behavior (R6-3)

| Concern | Behavior |
|---------|----------|
| Cross-process lock | `WriterLock` (`fs2` exclusive) on `.lokai/writer.lock`; second commit fails with `LockContention` |
| Staging location | `.lokai/staging/{txn_id}/` (workspace-local, recoverable) |
| Incomplete stage | Startup `recover_at_startup` aborts orphaned Created/Staging/Staged/… meta and deletes staging; workspace files untouched |
| Incomplete commit | Journal rollback / `RecoveryRequired` (unchanged); journals copy new content into backups so recovery does not depend on live staging |
| Agent success path | `commit_staged_if_any` on `finish` |
| Cancel / fail | `abort_staged_if_any` from agent cancel, turn error, stuck, and effort-cap exits |
| Verify view (R10) | `verification_view_if_active` materializes `.lokai/staging/.../verify_overlay`; `begin_verification` idempotent while `Verifying`; failed verify → `Rejected` (no commit) |

## Product plan

| ID | Status |
|----|--------|
| M2-4 Workspace transactions | **Partial** (CAS / parallel writers out of scope) |
| R6-3 Cross-process txn + staged recovery | **Done** |

## Tests

```bash
cargo test -p lokai-transaction --test matrix
```

Covers the M2-4 matrix plus R6-3: cross-process commit contention, kill-after-stage recovery, and explicit abort.
