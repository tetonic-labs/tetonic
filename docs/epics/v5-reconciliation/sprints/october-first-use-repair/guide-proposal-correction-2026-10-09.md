# Guide proposal correction — October 9

MVP priority 2: make discussion → reviewed proposal → useful work dependable.
This slice closes the unbounded allocation-correction and partial-publication gaps.
It does not mark the whole Guide journey or EXP-004 complete.

## Product behavior

- Discussion remains in the existing Guide surface. No new page, modal or planning mode.
- A proposal rejected by current allocation validation returns specific feedback to
  the same Guide invocation. The Guide can make one correction using its remaining
  steps, time and tokens. There is no new model run or automatic budget increase.
- Correction may change assignment token allocations only. The shared direction,
  total allowance, contributors, instructions, tools, dependencies, deliverables,
  questions and other proposal fields must remain unchanged from the first candidate. A material change
  needs a subsequent human-directed conversation turn.
- The Guide activity row shows an allowance issue, an actual correction request,
  and either a saved proposal or a clear unsaved outcome. It uses actual recorded
  tool results, not timed phases or assistant prose claiming success.
- A valid result publishes its brief and one proposal revision together. A failed
  correction leaves the prior proposal intact. The user still reviews and starts
  the exact proposal through the existing launch path; correction dispatches nothing.

## Ownership and persistence

`work/director/control.rs` remains the Guide orchestration adapter. It uses current
roster and allowance checks, the existing `work_plan` managed tool, and authenticated
`ResourceService` management authority. Provider call identity travels through that
existing tool boundary; the model cannot choose the conversation or repair authority.

Schema 75 adds `guide_proposal_attempts` inside `tetonic-memory/control`. At most two
records per Guide turn retain exact requests and results. This is a proposal write
receipt and correction limit, not a scheduler or inference ledger. Exact retries
return their original outcome; changed requests cannot reuse a call ID. A third
distinct attempt cannot publish a proposal, even after reopening the database.

The same transaction rechecks ownership, current plan/brief versions and whether
execution has started, enforces correction invariants, and composes the existing
`save_work_brief_in_transaction` and `save_huddle_plan_in_transaction` owners.
Failure after brief insertion rolls it back along with the proposed receipt.
Owner edits made during planning win over a stale correction. Successful plan
retries retain the existing proposal identity and cannot create another revision.

The new migration follows the existing pre-migration backup/version mechanism.
Existing plans and briefs are unchanged. Repair receipts survive database reopen;
this does not imply that interrupted Guide executions automatically resume.

## Verification and remaining work

Storage tests cover restart, exact/changed retries, duplicate saves, one failed
correction, unchanged scope/total/contributors/tools/dependencies, foreign ownership,
owner edits, and an injected plan-write failure after brief insertion.

Managed-runtime tests use a scripted inference endpoint to submit a rejected plan,
receive feedback, correct it and save one reviewable version. A second scenario
changes a deliverable and then attempts another correction; neither is published.
The tests also check that no workers were dispatched. Existing plan execution tests
cover parallel admission, dependencies, contribution delivery and result assembly.

UI tests cover correction progress, success and failure in the existing activity
row. Scripted transport tests establish execution behavior, not model intelligence
or useful real-world results. Real-model decomposition, resource selection,
synthesis quality, fresh-user usability and malformed tool-argument repair still
need qualification. The separate structured-generation path retains its existing
validation; this correction policy applies to Guide-authored `work_plan` proposals.

The running preview was not restarted during this slice. It needs the updated
engine for schema 75 and the new tool behavior.

Automated application validation: 198 tests passed; five environment/subprocess
fixtures were ignored. The run includes the concurrently developed MCP changes in
this checkout; those features are not attributed to this slice. All 45 focused
Guide activity, team-work and Blackboard UI tests passed, together with TypeScript,
frontend architecture checks and the production Vite build. Vite retains the
existing large-chunk advisory.

All 189 storage tests passed, including the three new proposal correction tests
and the existing storage/migration/recovery coverage. Rust formatting and
`git diff --check` passed for the changes. No live-provider inference was used.
