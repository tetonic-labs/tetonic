# Reviewed twelve-assignment qualification

Date: October 14, 2026. Engine revision: `69935a2`.
Follow-up to [contextual source search qualification](contextual-search-qualification-2026-10-14.md).

## Verdict: parallel execution demonstrated; useful project delivery failed

One reviewed project ran through the existing managed execution path with six
saved specialists and twelve assignments. All six initial workers failed budget
admission before finishing. Six dependent assignments never started. There were
**zero committed deliverable files** and no executed validation or repair pass.
Draft tool writes are not successful deliveries.

The coordinator escalated to a human instead of claiming completion. After the
workers settled, the operator canceled the root through the normal API to close
the failed test and release its remaining reservation. The recorded root state
is therefore `canceled`, not `completed`; child failures remain recorded.

## Authorization and reviewed plan

The owner allowed up to **1,200,000 tokens for one reviewed twelve-task run**.
Existing host and plan validators cap an individual plan at 1,000,000 tokens.
Those limits were not changed. The reviewed allocation was **992,000**:
928,000 for workers and 64,000 for coordination. No second project run, automatic
budget increase, manual completion of agent output or extra repair was attempted.

This used the previous model-authored assignment graph, with explicit operator
review changes: a fresh output directory, allocations, a shorter shared brief,
the already-granted contextual search tool, and up to 24 steps per assignment.
Each assignment retained a 900-second limit. This is not evidence that the Guide
independently devised or corrected these execution choices.

The independent source auditor was also assigned to create and commit the
validator before the later validation assignment. This was necessary because
ordinary `run_shell` sees committed files, whereas `read_file` can inspect the
calling agent's staged writes. Initial validation would execute the committed
script rather than create and run a staged script. This was a reviewed plan
adjustment, not an engine change or a passing validation result.

| Assignment | Allocation | Reported use | Calls | Outcome |
|---|---:|---:|---:|---|
| Setup guide | 80,000 | 71,051 | 9 | Failed; wrote, revised and inspected a draft |
| Agents, tools and permissions | 80,000 | 63,477 | 9 | Failed; wrote, revised and inspected a draft |
| Coordination guide | 80,000 | 70,018 | 9 | Failed; wrote and inspected a draft |
| Budgets, stop and recovery | 80,000 | 65,065 | 9 | Failed during source research |
| HTML shell and contract | 64,000 | 43,462 | 5 | Failed; wrote, partially inspected and revised two drafts |
| Source audit and validator | 112,000 | 94,337 | 10 | Failed during source research |
| Acceptance exercises | 48,000 | 0 | 0 | Not started |
| Readiness checklist | 48,000 | 0 | 0 | Not started |
| HTML integration | 96,000 | 0 | 0 | Not started |
| Initial validation | 96,000 | 0 | 0 | Not started |
| One repair pass | 64,000 | 0 | 0 | Not started |
| Final revalidation | 80,000 | 0 | 0 | Not started |
| Coordinator | 64,000 | 4,246 | 2 | Escalated; subsequently canceled by operator |
| **Total** | **992,000** | **411,656** | **53** | **No completed assignment** |

## What actually happened

All six specialists used the saved OpenAI `gpt-6.1-sol` configuration and real
granted local tools. Durable events place their starts between 05:23:23.754 and
05:23:24.718 UTC. The first worker failed at 05:23:50.381 UTC: all six were active
together for approximately 25.7 seconds. This establishes overlapping attempts,
not six model requests continuously processing at the provider simultaneously.

All failures were local admission rejections before another model request was
sent. Recorded estimates for the next request plus minimum output headroom were
19,695–30,498 tokens; remaining worker allowances were 8,949–20,538 tokens. The
request no longer fit, even though the full project still had unused allocation.
Unstarted dependent assignments' allocations were not silently reassigned.

The reported total is **392,498 input + 19,158 output = 411,656 tokens**. Input is
approximately 95.3% of reported usage. Worker transcripts repeatedly carried
their assignment, shared brief, all twelve team output contracts, accumulated
source observations and generated file content. This supports investigating
cumulative context cost; it does not establish that every input token was an
unnecessary reread, nor does it establish billed dollars or cache discounts.

The setup, permissions and coordination workers reached staged-file inspection
but never finalized. The HTML worker inspected portions of its first drafts and
then rewrote them without a subsequent successful inspection/finalization turn.
The budget and source-audit workers never reached writing. Filesystem inspection
found no committed files under `verified-kit/reviewed-12-20261014/`. All thirteen
source snapshot hashes remained unchanged.

The coordinator received a generic `did not complete (failed)` result with
`result: null` for each worker. It did not receive their actionable budget error,
despite that error being retained in task details and attempt events. It asked
for failure details or permission to stop rather than fabricating a result or
retrying. This exposes a separate orchestration feedback gap.

## Accounting and cleanup

- No reported over-limit usage, pending calls or unknown calls in this project.
- After cancellation, the project's remaining held tokens were zero. Reported
  usage remained 411,656. An unrelated older unknown-usage record was untouched.
- Original saved agent limits were restored through revision-checked APIs: Setup
  65,536 tokens, the other five specialists 32,768 each; all six at 16 steps and
  900 seconds. Other saved configuration and grants were preserved.
- The qualification host was restored to 384,000 work tokens and 32,768
  coordination tokens, with its original 24-step, 900-second and 32,768-context
  settings. Only the verified qualification process on port 18740 was restarted.
- Task IDs/states and saved agent revisions survived restart. Other running
  engine profiles were not stopped. No credentials are included in this report.

## Next engineering work indicated by this result

1. **Make the existing harness finish within its allowance.** Trace repeated
   request content and budget estimation together. Provide bounded, task-relevant
   context and sufficient inspection/finalization headroom without silently
   dropping required evidence, breaking tool exchanges or weakening accounting.
   Prove this against captured failure shapes with deterministic tests before
   spending on another hosted project.
2. **Return actionable worker failures to the coordinator.** Extend the existing
   assignment result boundary with bounded, authorized failure details and actual
   output state. Do not make it infer budget failure from a generic failure string
   or treat retained event history as delivered files.
3. **Resolve staged-write validation semantics explicitly.** The reviewed plan
   workaround is not a general solution for agents that write code and then run
   it. Reuse the existing transactional verification owner rather than adding an
   independent executor or granting a bypass around workspace policy.

Raising the test ceiling did not qualify project delivery. A later qualification
still needs produced files, executed independent checks, one bounded repair and
revalidation through the actual product path. It requires a separately reviewed
run; this authorization was used once.

## Evidence references

- Source work: `68015fd4-f83e-456c-bfca-bacd0d24ca6d`.
- Root work: `9a1c555e-5b88-4a1d-93bd-22111b40c9ec`.
- Managed run: `run_activation_3b9bfa44bb63084f2f25eafcda772aba528efcdfb61775e4f77553b13e3bfc72`.
- Local raw evidence: ignored `.lokai/project-width-87372f80/reviewed-project-*`,
  including reviewed plan, per-task receipts, tool-call records, durable events,
  usage snapshots, source hashes and before/after restoration snapshots.
- Reproduction uses the existing plan continuation, revision, agreement, start,
  inspection and cancellation APIs. The ignored helper
  `.lokai/reviewed-project-proof.py` refuses a second start for this authorization.

No engine or UI implementation changed during this qualification. Checks were
the live hosted run, artifact/hash inspection, accounting settlement and restart
preservation; no additional Rust or UI test suite was needed for this report.
