# Coordinator dispatch checkpoints

Date: October 8, 2026. OCT-203 prerequisite; automatic team restart stays disabled.

## Delivered

The coordinator now saves its exact general-harness checkpoint before the team
controller admits workers for a dispatch. This uses the existing core loop,
managed artifact owner, scoped dispatch receipts and controller. It adds no
scheduler, budget ledger or UI path.

The core exposes a checkpoint only at a valid single-call managed host boundary.
It retains the pinned harness and invocation, conversation and private provider
continuation state, pending call, consumed steps/tokens and loop monitor. Version
two also retains host calls whose responses actually entered the conversation:
their IDs, arguments and response digests. This small record survives model-context
compaction without duplicating every tool response. A new invocation clears it;
loading an ordinary audit transcript does not manufacture delivery history.

The application passes that boundary to the existing team controller. After
current parent authorization and request acceptance, it:

1. Matches previously received dispatch calls against scoped receipts, including
   their ordered selections and exact model-facing response digests.
2. Saves the checkpoint through the managed runtime's existing artifact store,
   marked Secret and owned by the same run/task/attempt, with the existing 2 MiB
   checkpoint limit and UntilRunCompletes retention policy.
3. Binds its immutable artifact reference to the accepted dispatch in a transaction
   checking the current lease, scope and hierarchical stop generation.
4. Rebuilds the contribution-delivery guard, then uses existing worker admission
   or returns the exact durable response on a matching retry.

The controller does **not** infer receipt from task completion. A completed worker
whose result is absent from that checkpoint stays outstanding. A persistence-error
response carries no contributions, even if a result committed before the error.
Changed checkpoint contents, missing/mismatched history, unavailable artifacts,
stale authority or a failed reference write prevent worker admission.

The artifact seal and SQLite reference binding are deliberately separate. Process
loss between them can leave an unbound artifact, but cannot authorize children or
mark delivery. New checkpoint artifacts are bounded individually; this slice does
not add a new aggregate artifact quota or garbage-collection service.

## Compatibility and ownership

- Schema **72** protects the added receipt field: older writers refuse the database
  instead of silently erasing checkpoint references. Existing transactional
  migrations and pre-upgrade backup ownership apply.
- Old completed receipts remain readable but cannot be retrofitted with invented
  pre-execution state. Old version-one root human-wait checkpoints remain readable;
  they are refused as coordinator delivery evidence.
- `tetonic-core` owns the checkpoint representation and received-call record.
- `tetonic-run::managed` owns protected artifact writes/reads and current executor
  authorization. Root human waits use the same extracted checkpoint writer.
- `tetonic-memory` owns immutable receipt/reference transactions.
- `tetonic-app::team_work_controller::checkpoint` owns delivery reconstruction and
  wiring, using existing plan progress and admission owners.

## Verification

Tests cover checkpoint serialization after simulated message compaction; all three
frontier protocol continuation formats; unknown/changed/duplicate call histories;
unchanged step/token accounting; secret artifact scope; exact saved-response replay;
completed-but-undelivered contributions; failed reference and response writes;
immutable reference replacement; cancellation and stale leases; and refusal of
batched host dispatch before any ordinary effect executes.

Application scenarios use the real managed/controller/store path and a local
scripted provider. They demonstrate checkpoint persistence and controller delivery
reconstruction under a live authorized parent, **not** process-level team resume.

Validation:

- `cargo test -p tetonic-core -p tetonic-memory -p tetonic-app --lib`:
  **40 core, 184 storage and 174 application tests passed; four application tests
  ignored**. Storage coverage includes migration rollback and abrupt process exit
  at every schema marker through 72.
- `cargo test -p tetonic-run --test managed_service_tests`: **48 passed**, including
  existing root wait/restart, competing restorers, retained allowance, cancellation,
  stale-parent fencing and the new dispatch-batch rejection.
- `cargo run -p tetonic-arch-gate -- verify package`: **passed** formatting,
  workspace/all-target Clippy with warnings denied, architecture and static quality.
- Changed documentation links and `git diff --check`: **passed**.

The first quality run found a needless test-only Box allocation; it was removed
and the complete package gate passed. No warning exemptions were added.

## Next boundary

Quiesce and checkpoint supported child attempts, restore the subtree under fresh
ownership fences, and integrate human answers while retaining original permission
horizons, remaining execution allowance and usage reservations. Keep unsupported
or interrupted effects non-replayable. Then prove process loss and competing
restorers at dispatch, child completion, answer and delivery boundaries.

No UI changes, live engine restart, real-model usefulness result, automatic team
resume or 48-hour soak is claimed by this slice.
