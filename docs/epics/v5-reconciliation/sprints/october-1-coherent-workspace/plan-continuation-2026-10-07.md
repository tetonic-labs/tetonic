# Continue unfinished team work

Date: 2026-10-07

## Product change

A failed or stopped team plan previously ended in an inspection screen. Rebuilding the plan meant remembering which contributions were finished, copying evidence, and avoiding actions that might already have happened.

The existing work panel now offers **Review unfinished work** after the earlier execution has fully stopped. This saves and opens a continuation proposal. It retains completed contributions and answered questions in the shared brief, carries forward the latest directions for unfinished assignments, and removes dependencies already satisfied by retained evidence. The owner can discuss or edit the proposal and use the existing **Start this plan** control.

If every assignment finished but coordination failed, the proposal contains one visible assignment to assemble their results. It does not redispatch the original assignments. The original plan, contributions, attempts and usage remain inspectable through links. The map distinguishes the proposal with a `Continue:` title.

## Existing owners reused

- `LocalWorkspace` assembles the proposal from the existing execution receipt, scoped task outcomes, questions and directions. The new endpoint is `POST /api/local/plans/{source}/continue`.
- `ResourceService` authorizes reads and owner changes through the existing organization/team boundary.
- The existing team-work, versioned brief and huddle stores write the new source/brief/proposal atomically. Their transaction wrappers now expose internal composition helpers; their public behavior remains unchanged.
- Schema 63 adds only a provenance link between the earlier execution and the continuation source. A unique source key makes duplicate requests, multiple tabs and retries reopen the same proposal.
- Starting uses the existing huddle agreement, managed runtime, coordinator, agent revision pins, parallel dispatch, tool approvals and budget ledger. There is no second executor or retry queue.
- Read-only execution projection moved into `plan_execution/projection.rs`, leaving admission/dispatch in its existing owner and keeping the architecture size gate satisfied.
- A prepared continuation is a valid anchor for the existing Guide conversation. The original private exploration transcript is not copied into shared context.

## Execution and accounting rules

Preparing and inspecting a continuation never invokes inference or grants execution. A matching earlier root ID is required. An active, waiting or unresolved interrupted execution is rejected. The storage transaction independently checks terminal journal state; partial validation failures roll back the new source and brief.

The proposal requests a new allowance for its remaining work. It does not refund, reset or relabel earlier spending, including unknown provider usage and retained reservations. New starts resolve current saved agent/model configurations and existing provider-consent requirements. Historical work keeps its original pins.

For a started, unfinished assignment, the host examines its pinned prior tool grants. Anything outside the known local read-only set requires the owner to inspect earlier activity and explicitly confirm that the continuation is safe to start. Changing the agent's tools later cannot hide that earlier authority. This is a conservative review requirement, not a claim that a side effect occurred or was rolled back. UI review resets when the proposal revision changes; an uncertain start retains the original command and review decision.

## Scope and remaining work

This is an explicit continuation after terminal failure/cancellation, not durable suspension or in-place replay. Restarted unresolved attempts remain blocked. This does not complete OCT-203's broader parked-work/resumption requirements or the MVP.

The shared brief remains bounded at 12,000 bytes. Oversized retained evidence and directions that exceed the existing 4,000-byte assignment limit are rejected with guidance; evidence is not silently truncated. Completed assignments need a readable recorded contribution. Selection/summarization of large evidence sets and crash reconciliation remain future work.

## Validation

Controlled provider tests cover partial failure, synthesis failure, retained results, latest direction changes, answered questions, old tool grants after agent edits, required action review, exactly one continuation, restart persistence, fresh execution, unchanged earlier usage, unknown reservations, private-context exclusion, active/crashed-run rejection, access checks and atomic rollback. They also verify that preparing a proposal makes no model requests and completed workers are not dispatched again.

UI tests cover the proposal/review path, uncertain responses across remounts, blocked recovery, reset review on plan edits and preservation of confirmed review on an uncertain start. Updated one stale Guide-settings copy assertion to match the previously shipped coordination-model behavior. The v61 upgrade test now removes all later schema markers/tables when constructing its historical fixture.

Checks passed: 237 application tests (three opt-in live-model tests ignored); 171 memory tests; 166 frontend tests; production frontend build/typecheck; strict application/CLI Clippy with tests; CLI build; architecture gate; Rust and changed-frontend formatting checks. Targeted continuation and local-workspace suites also passed. The frontend retains its existing bundle-size advisory. No paid provider inference was used.

Browser verification used a separate database and copied artifact store produced by the controlled provider-failure test. One finished and one failed assignment became a saved draft containing only the unfinished assignment, one retained contribution, a 2,000-token additional allowance and one required action review. The review checkbox gated Start; no execution was launched. Copying only the test database initially exposed the missing-artifact guard, so the UI now explains missing saved results before offering continuation. The owner's actual workspace was not seeded with test work.

Local screenshot: `.lokai/manual-testing/plan-continuation-review.jpg`. Logs and the isolated fixture are under `.lokai/manual-testing/`. The existing server on port 3004 and UI on port 5177 were refreshed after confirming no active work. Its 27 prior tasks remained present. Temporary preview services are stopped after verification.
