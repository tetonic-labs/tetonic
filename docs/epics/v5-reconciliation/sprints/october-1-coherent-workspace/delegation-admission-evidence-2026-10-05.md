# COORD-A: durable allocation and inherited stops

October 5, 2026. Allocation-admission substep implemented; **COORD-A remains open**.
This extends the existing work/resource/store path. It does not enable plan
dispatch, install a second scheduler, or claim that provider spending is governed.

## Changes

`ResourceService::authorize_work_budget` records an explicit, immutable root
allowance. Current team-management authority is checked again in the storage
transaction. Payer attribution comes from the verified actor. A child cannot
open a new allowance or switch payer. A caller-supplied parent budget is now
only an assertion against this stored allowance.

`create_work_delegation` reserves from that allowance in the **same SQLite
IMMEDIATE transaction** that creates the child and existing delegation record.
Sibling allocations and the parent's own `reserve_work_budget` reservations
compete for one remaining balance. A child can divide its allocation again,
without increasing the root commitment. Different connections cannot both claim
the same remaining capacity. This is local durable admission, not distributed
consensus or cross-node budget enforcement.

Reservations retain their full amount across failure, cancellation, unknown
usage and restart. No refund, resizing or settlement API is exposed. A lost
response can retrieve the same receipt without reserving twice; changed request
attributes conflict. Receipt retrieval is not permission to execute again.
Measured team effort stays separate from reserved capacity.

New child IDs must be fresh. Existing work cannot be reparented or given another
parent, and stop scopes cannot be spoofed with an ID prefix. Scoped lineage
walks reject cycles, ambiguous legacy parents and depth beyond 32 work items.
Nested children inherit the original payer and root stop scope.

Work-level stop lookup, parking and managed cancellation-target discovery now
include descendants. Target discovery and the stop write share a transaction.
Binding/resuming work checks inherited stops and parent parking in its own write
transaction. If the stop wins after managed admission but before binding, the
application cancels the fresh run. Unconfirmed cancellation returns an explicit
failure and attempts to record the unresolved effect against the stop.

Schema **55** adds root envelopes and own-effort reservations. Child allocations
remain in `work_delegations`. Existing claimed budgets are not backfilled into
authority; legacy delegations remain inspectable but cannot acquire a new
allowance through this API. The existing pre-migration backup path is retained.

## Integration and verification

- `cargo test -p tetonic-memory --lib`: **145 passed**. This includes eleven new
  budget/lineage tests and existing migration crash/recovery coverage.
- Independent SQLite connections exercise duplicate delivery, sibling-versus-
  sibling and sibling-versus-parent contention, and stop-versus-work-binding races.
  A stop either captures the admitted run or denies its binding. Unrelated work
  remains usable.
- The ResourceService test creates an allowance, delegates, reserves parent
  effort and reads the combined balance through authenticated APIs.
- The application activation test pauses managed admission before the work bind,
  records a stop, then verifies that binding fails and another root can use the
  same registered agent. It also keeps the independent-root child bypass closed.
- The first memory run caught a version-54 migration fixture which needed to
  delete subsequent schema markers when reconstructing old state; corrected.
- `cargo test -p tetonic-app --lib`: **189 passed**, including the production
  registered-runtime path, stop/admission race, private-context isolation and
  local workspace/shaping tests.
- `git diff --check`: passed. Changes remain local in the existing mixed worktree;
  no unrelated edits were staged, committed or pushed.

## Remaining COORD-A work

1. Derive a child execution grant from current parent authority and the selected
   registered revision; enforce context/tool/artifact limits at the real runtime.
2. Bind allocation receipts to durable parent/task/attempt lineage and managed
   admission. Current allocation receipts do not authorize execution, and normal
   root execution does not yet call the new own-effort reservation API.
3. Reserve/enforce bounded model and tool effort before dispatch, including
   orchestration and retries. The existing provider-reported token ceiling is
   not a hard aggregate spending guarantee and is not a substitute for this.
4. Use the existing managed submission owner for child execution, with inherited
   cancellation, quiescence, restart reconciliation and bounded retry/coordination.
5. Only then connect agreed plans to dispatch and project real child activity
   into the team-work map. `execution_available` remains false.

No UI surface or running preview database was changed by this substep. No live
team execution, automatic restart recovery, reservation refunds or complete
coordination gate is claimed.
