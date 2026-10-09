# Durable coordinator dispatch receipts

Date: October 8, 2026. OCT-203 prerequisite; full team waiting/recovery stays open.

## Delivered

Previously, `dispatch_assignment` carried the chosen keys into an in-memory
controller queue and returned an in-memory response. Individual work bindings and
contributions were durable, but the coordinator's exact pending selection and
returned result were not. Recovery would have had to infer that missing state.

The existing controller now:

1. Receives the model's actual tool-call ID alongside its ordered assignment keys.
2. Rechecks the original managed parent handle and credential.
3. Saves the selection under the approved plan, run/attempt, current lease fence
   and hierarchical stop-generation binding before child admission.
4. Uses the existing dependency/capacity controller and managed child admission.
5. Saves the exact success/partial/failure response and which contributions it
   contains before advancing the finish guard and returning to the coordinator.
6. Replays a completed matching receipt without more worker or model calls.

A request ID reused with different keys, order or single/grouped shape conflicts.
Stale leases, changed attempts/runs, expired execution and inaccessible scopes
cannot accept or complete a dispatch. A response is immutable. Only accepted
successful child tasks can be marked delivered; a human wait is not completion.
The collector uses a provisional delivery cache until persistence succeeds.
Early group errors without a contribution payload cannot claim prior collected
results were delivered.

## Ownership and storage

- `tetonic-app::team_work_controller::receipts` coordinates these boundaries.
- `tetonic-run::managed::DelegationParent::authorize_dispatch` verifies current
  runtime authority. It returns a fence, not permission to bypass admission.
- `tetonic-memory::control::huddle_execution::dispatch` validates and persists
  receipts inside the existing store. Its current-lease and stop checks share the
  write transaction with each record mutation.
- Existing work records, run journal, registered admission and usage accounting
  retain ownership of execution. There is no second scheduler or budget ledger.

Schema 71 adds `huddle_dispatch_receipts`, keyed by organization, security team,
source work, coordinator attempt and tool-call ID. A foreign key binds the approved
huddle execution. Historical reads require current team access and confer no
execution authority. Receipts contain already-shared team results, not private
Guide conversation history. Bounds are 4096 calls and 16 MiB of serialized receipts
per plan, with 2 MiB per result. Exceeding them fails explicitly; stored contributions
remain available. These limits are independent of model and work budgets.

Existing upgrade ownership, verified pre-upgrade backup and atomic migrations
apply. Older writers refuse the newer schema. Tests use temporary databases; the
user's running engine/database was not restarted or upgraded. UI layout and public
HTTP contracts are unchanged.

## Verification

New application tests use real managed team execution and a local scripted provider:

- One worker waits for a human while another completes. Pending and completed
  receipts survive an independent database reopen.
- A newly constructed controller returns the stored completed response, adjusts
  only its delivered-contribution guard, and makes no worker/model calls. This is
  receipt replay under the existing live parent, **not** process-level team restore.
- Changed request selections/shapes, foreign scope, wrong run/attempt, stale lease,
  changed response, expired deadline, false completed contribution and cancellation
  are denied.
- A real SQLite write failure after collection cannot clear the live finish guard.
  After removing the failure, the same call reconciles the already-completed worker
  and saves its result without repeating inference.
- The managed suspension test checks the new dispatch authorization API: a parked
  parent and an old handle after resume cannot borrow the new owner's lease.

Final command results:

- `cargo test -p tetonic-memory --lib`: **184 passed**, including rollback and
  abrupt-process-exit tests at every migration marker through schema 71.
- `cargo test -p tetonic-app --lib`: **171 passed, four ignored**. This includes
  the new receipt fault/replay tests and existing parallel, mixed-provider,
  human-question, continuation and accounting scenarios.
- `cargo test -p tetonic-run --test managed_service_tests suspended_parent_cannot_dispatch`:
  **one passed**, including the new authorization checks before parking, while
  parked and after a different execution incarnation resumes.
- `cargo run -p tetonic-arch-gate -- verify package`: **passed** workspace formatting,
  all-target Clippy with warnings denied, architecture and static quality.
- Documentation file links and `git diff --check`: **passed**.

The first storage run exposed two legacy migration fixtures that removed only
version 70 while leaving the newer version marker. They now remove all later
markers and assert the current target version; the full suite passed afterward.

## Remaining before enabling durable team waits

Follow-on: [coordinator checkpoints](coordinator-checkpoints-2026-10-08.md) implements
the first prerequisite below on the production dispatch path. The other recovery
requirements remain open.

1. Bind the coordinator's exact pending dispatch call to its checkpoint and rebuild
   delivery state from the calls represented in that checkpoint. Do not blindly
   treat all completed assignments or all historical receipts as model-acknowledged.
2. Quiesce and checkpoint supported child attempts, then restore ownership under
   new fences without replaying interrupted effects. Reject unsupported tool state.
3. Separate bounded human response time from remaining active execution time and
   pin that authorization horizon at original admission. Answers cannot extend
   permission, reset tokens or erase stop generations.
4. Wire persisted human answers into the restored controller and preserve the
   original usage reservations and completed contributions.
5. Prove process death during acceptance, child completion, human response and
   result delivery, including competing restorers, revocation and emergency stop.

The root-only suspension/reconstruction guards are intentionally still present.
No automatic team resume, recurrence, real-model usefulness or 48-hour soak is
claimed by this slice.
