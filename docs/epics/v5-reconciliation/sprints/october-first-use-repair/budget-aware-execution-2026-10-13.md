# Budget-aware execution and scoped exhaustion — October 13, 2026

**Implementation and deterministic qualification complete. Live project acceptance
remains open.** This addresses the first execution blocker from the
[wide-project trial](project-width-proof-2026-10-13.md): repeated input was not
allowed for in output limits, and one worker's reported overrun closed unrelated
branches. This record does not turn the previous failed live projects into passes.

## Product behavior

- Each funded model request must fit its estimated input plus 256 tokens of
  completion headroom. Input includes messages, tools, schemas, structured output,
  private provider continuation and the budget notice. The output cap uses what
  remains after that estimate and respects any smaller existing output cap.
- Workers receive current usage allowance feedback on every outgoing request,
  including the cost of rereading context. They can use existing completion or
  human-handoff tools to report actual completion or unfinished work. This is
  feedback to the model, not a guarantee that it chooses a good stopping point.
- An unaffordable request stops before provider invocation. It creates no pending
  provider charge, and the inspector explains that no request was sent. Existing
  saved contributions remain available through the normal recovery flow.
- A worker overrun stops its branch. Independent assignments can continue within
  their own allowances and the actual shared headroom. Assignments that depend on
  the failed contribution do not run. A real ancestor overrun stops that scope.
- Unconfirmed calls retain their holds, including across restart. Reported excess
  is charged through the allocation tree, so it cannot be allocated a second time.
  Concurrent callers cannot reserve the same remaining shared headroom.
- A proposed continuation that only assembles preserved results retains the
  selected assignment's allocation instead of silently shrinking it to 2,000
  tokens. It still requires the existing review and managed admission path.

## Existing owners extended

`tetonic-inference` owns request sizing. `tetonic-app` applies the budget notice
and output cap through its existing `WorkUsageProvider`. `tetonic-memory` owns
transactional admission, reservations, usage reports and allocation liabilities.
No second ledger, executor, orchestration loop or UI-owned lifecycle was added.
There is no schema migration or frontend layout change.

Per-call admission distinguishes reported spend and unresolved calls from future
assignment allocations. It checks the attempt and every funded ancestor in the
same immediate transaction that inserts the pending call. Live grant validation
uses reported overruns, not pending holds: a worker holding its last tokens must
not revoke its own already-admitted request. Existing claim, activation, lease,
revision, grant, stop and settlement checks remain in force.

## Verification

- Application library: **222 passed, five existing ignored**. Includes provider
  tool parity, revocation, process recovery, usage settlement, agent configuration,
  Guide interaction and human handoffs.
- Memory library on the final accounting implementation: **208 passed**. New
  cases cover atomic preflight rollback, restart with unknown usage, nested
  allocation overflow, fully allocated projects, two competing SQLite writers,
  shared overspend and funding previously unbounded work with unknown usage.
- Inference library: **156 passed, two existing ignored**. Request sizing covers
  non-ASCII text, tools/results, structured output and private continuation.
- Final plan execution suite: **25 passed, three existing ignored**. Both
  six-worker/20-assignment completion and the three-slot queued variant pass.
  These use scripted providers and prove scheduler behavior, not model quality.
- New registered-execution failure case: one of four assignments reports 4,001
  tokens against 4,000. Its returned completion is rejected; an independent
  branch and that branch's successor complete; the failed branch's dependent
  remains unstarted. The coordinator can raise a human question with the partial
  results. The real ledger retains 4,121 reported tokens and one over-limit row.
- Package quality gate: formatting, workspace/all-target Clippy, architecture
  and static quality checks passed.
- CLI build passed. The idle engine on port 18741 was restarted using the same
  profile. Authenticated workspace reads confirmed all 23 work IDs/states and
  five saved agent identities/configuration digests were preserved. No new paid
  model call or live project was submitted during this chunk.

Several preexisting scripted-provider fixtures had allowances smaller than the
input they actually sent (some only 40–500 tokens). Those fixtures now explicitly
fund their context. Their reported usage, grant checks, failure behavior and
settlement assertions remain meaningful. Production defaults were not raised.
The long-history Guide fixture explicitly provisions more capacity; a real long
conversation with too little allowance can still correctly stop at preflight.

## Remaining limits and next acceptance check

Input sizing is a conservative UTF-8-byte heuristic, not a vendor tokenizer or a
hard billing cap. Actual usage can exceed the estimate; it remains recorded and
the over-budget response cannot execute its returned tool calls. Provider-internal
retries are not separately metered. Ambiguous transport and some provider-side
validation failures still retain unknown usage. No automatic hold release or
budget increase was added.

This does not add automatic general-worker context compaction, dollar billing,
new time-budget accounting, or guaranteed model cooperation with budget feedback.
The remaining product acceptance is a bounded live, model-authored project across
5–10 agents and 10–30 assignments that delivers useful, inspected artifacts.
Deterministic success is a prerequisite for that run, not a replacement for it.
