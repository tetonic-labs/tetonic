# Review useful work after an agent stops — October 15, 2026

Status: implemented and verified offline. This advances interrupted-work
recovery and EXP-005/EXP-007; it does not close the sprint or qualify the large
project run. No actual-model attempt or paid inference was run for this change.

## Product gap and delivered behavior

An agent could write useful pending files, stop at a budget boundary or fail a
staged check, and leave only an inert local draft referenced by an audit note.
The person inspecting the assignment could not read those files in the product.
Repeating the whole assignment was too tempting when its partial output was
effectively invisible.

The existing individual-work inspector and expanded plan contribution now show
**Unfinished work saved** and **Review saved draft**. Opening it lists the files;
selecting a text file reads its exact pending contents in bounded sections.
There is no new modal, navigation destination or automatic execution. The view
distinguishes the draft from a verified result and from applied workspace files.
Read failures can be retried without retrying the agent. Disconnecting clears
the preview and cancels outstanding reads.

## Existing owners reused

- `tetonic-transaction` retains the exact inert draft and clears live staging.
  Unresolved commit recovery remains protected. Draft paths now check root
  ancestry and link/reparse targets before retention.
- Registered application assembly supplies the existing context artifact
  adapter and scoped audit to `execution::drafts::DraftRecorder`. The recorder
  checks the durable run/task/attempt and audit binding, including budget stops
  that occur before finalization's identity callback. Text is kept as text in
  the inspection copy so the existing artifact scanner can inspect it.
- `tetonic-memory` uses existing `events` and `context_artifacts` records for
  the scoped receipt. No new table, schema migration or execution owner.
- `WorkService` filters receipts to the exact attempt and reads through current
  artifact/context permissions. It validates provenance, size, complete digest
  and UTF-8 page boundaries, then rechecks access after the read. Hashing and
  parsing use a blocking worker.
- The local CLI adapter's task routes now live in `local_ui/tasks.rs` under the
  same parent authentication, origin and version checks. The existing browser
  client and inspectors consume that contract.

## Validation

All tests used temporary data, scripted providers or isolated browser fixtures.

| Check | Result |
|---|---|
| `cargo test --locked -p tetonic-app --lib` | 227 passed, 5 ignored. Includes two budget-stopped workers with separate readable drafts, rejection of a sibling work item's artifact, failed-check retention, reopen persistence, and altered-artifact rejection. |
| `cargo test --locked -p tetonic-memory --lib workspace_drafts` | Passed: exact context, bound history, private access, membership revocation and reopen. |
| `cargo test --locked -p tetonic-transaction --test matrix stopped_draft` | Passed: exact retained bytes, live-file preservation and no startup replay. |
| `cargo test --locked -p tetonic-cli --test local_ui_product` | Passed against a temporary real engine process: authentication, malformed/unknown fields, unavailable work, no-store responses and no work creation during inspection. |
| `npm test -- --maxWorkers=2` | 13 boundary/appearance checks and 326 Vitest tests passed. New tests cover on-demand inspection, paging, literal rendering of HTML-like file text, failed-read retry and disconnect cancellation. |
| `npm run build` | Passed; existing large-chunk advisory remains. |
| `cargo run --locked -p tetonic-arch-gate -- verify package` | Passed formatting, workspace/all-target Clippy, architecture and static quality. |
| Isolated Chromium fixture using the actual component and product styles | Inspected expanded file view at 900px and 440px widths; file selection works, no page errors or horizontal viewport overflow. This is visual evidence, not a live-model qualification. |

The first full application run exposed missing receipts on budget stops; the
recorder originally depended on the finalizer's later identity hook. Validation
now uses the existing durable attempt binding, and the complete suite passed
after that correction. Quality checks also caught a test-module ordering lint
and growth of the main HTTP adapter; both were fixed without exceptions.

## Limits and next required proof

- Existing raw drafts are not migrated. Newly retained drafts need the updated
  engine; the user's currently running engine was not restarted for this work.
- Binary data stays in the original local draft. The browser offers text
  inspection, not export, patch application or staged-effect restoration.
- If scoped publication/scanning fails, the original raw draft remains and an
  audit note explains that its inspection copy is unavailable. No false result
  or completion is recorded.
- This covers normal abort retention and failed commit/finalization retention.
  It is not crash-time checkpointing, automatic full-team restart, or a promise
  to recover a transaction requiring commit reconciliation.
- Existing reviewed continuation still retains completed contributions and
  prepares unfinished assignments; it does not automatically consume these
  draft files. Further recovery work must preserve current grants, budget
  accounting and review of potentially repeated external effects.
- The unsuccessful six-agent/twelve-task qualification remains unsuccessful.
  No new live attempt is authorized or implied by these passing offline tests.
