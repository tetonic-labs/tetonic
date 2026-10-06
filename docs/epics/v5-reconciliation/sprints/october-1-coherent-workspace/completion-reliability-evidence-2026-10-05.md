# Completion reliability — October 5, 2026

Follow-up to the [human handoff trials](human-handoff-evidence-2026-10-05.md).
This slice improves the existing local inference adapter and finite plan
dispatcher. It does not add another runtime, scheduler or budget ledger.

## Failure analysis and changes

The last handoff trial retained both completed worker contributions, but its
coordinator failed with `requested model residency unavailable`. The original
record did not distinguish admission from the post-response placement check,
so the exact historical timing cannot be established from that error alone.

Static tracing found a reproducible race: governed team context uses secret
retention, which sends Ollama `keep_alive: "0"`. A model can finish and unload
before Tetonic consumes its first buffered response chunk. Checking `/api/ps`
at that point previously rejected a completed response and lost its reported
usage. Deterministic HTTP fixtures reproduce this lifecycle.

The adapter still admits the exact allocation before sending the prompt and
holds its existing runtime admission lock through generation. Only an explicitly
zero-retention request may tolerate a subsequently absent runner. In that case,
output is buffered until the terminal completion receipt arrives. Missing
placement before dispatch, observed CPU spill, malformed placement, provider
errors and incomplete streams still fail. Retention is not extended; no prompt
is replayed. Completed content, tool calls and reported usage are preserved.
The broker retains its previous fallback classification for genuine absence.

The dispatcher also previously returned only the requested worker's result,
even when another previously waiting worker had completed in the meantime.
The coordinator could try to finish against a stale outstanding-work list,
then spend further calls fetching an already completed result. Dispatch replies
now include newly completed contributions from the same managed run, their
recorded direction amendments, and the remaining assignment keys. Only delivered
contributions clear the existing completion guard. No work is rerun and private
exploration is not included. Coordinator instructions explain this receipt.

Genuine local model-availability failures now have an actionable, curated UI
message that preserves completed contributions without exposing provider bodies.

## Checks

| Check | Result |
|---|---|
| Inference unit/HTTP tests | 139 passed, two opt-in tests ignored |
| Application unit/integration tests | 206 passed, two opt-in tests ignored |
| Broker unit tests | 34 passed |
| CLI build | Passed |

New regressions cover a terminal-only and a buffered multi-chunk response after
expected unload, content/tool/usage preservation, truncated output, unexpected
unload, CPU spill, pre-dispatch absence, and preservation of provider error
reasons. Application fixtures verify completed-sibling receipt reconciliation,
no repeated extra contribution, retained outstanding work and private-history
exclusion. Existing handoff, cancellation, deadline, grant and usage tests pass.

Provider changes and their tests are committed as `90564ca`. The application
integration remains in the existing larger uncommitted workspace changes; those
were not swept into the provider commit.

## Live evidence

The fresh local test uses `.lokai/reliable-completion-live-2026-10-05`, the
installed `qwen3.5:latest`, and the current team UI on isolated ports 3003/5176.
The explicit demonstration plan matches the previous trial: two 3500-token
worker allowances and 4096 tokens for coordination, totaling 11096; worker
deadlines remain 120 seconds and the parent deadline is six minutes. The browser
starts the plan, amends the unstarted review and answers the actual worker
question. The brief/plan are seeded inputs; all questions, contributions and
coordination decisions after start are real model output.

**Outcome: completed.** The owner answered **Beginners**. The comparison worker
used that audience, the reviewer supplied the amended risk/mitigation bullets,
and the coordinator returned both in its final result. The second dispatch reply
included the comparison in `also_completed` and an empty outstanding list;
there was no premature finish rejection or repeat result-fetch call. The UI
displayed **Your team's result / Result ready / 2 of 2 contributions ready**.

| Participant | Model calls | Reported tokens | Own allowance |
|---|---:|---:|---:|
| Coordinator | 3 | 3101 | 4096 |
| Comparison worker | 2 | 1923 | 3500 |
| Reviewer | 1 | 1083 | 3500 |
| Total | 6 | 6107 | 11096 |

The recorded run interval was about 3 minutes 39 seconds. All six calls reported
usage; no pending/unknown calls or held tokens remain. These are provider token
counts, not a financial billing guarantee. The earlier failed trial attempted
five coordinator calls; this is a measured comparison of these two runs, not a
general performance guarantee.

Artifacts in the trial directory: `result.json`, `snapshot.json`,
`usage-evidence.json`, `workspace.db`, and `completed-team.png`. Source work:
`6722219c-b1a1-403a-9637-36aad8aeb20f`; coordinator work:
`66c3b831-0590-473b-b2c7-946625e0358b`. The synthetic plan and limited reasoning-only
contributions are not evidence of supplied-document usefulness, broad tool
collaboration, fresh-user comprehension or sustained reliability. Full sprint
acceptance stays open.

## Remaining scope

Durable parking/resumption, the wider tool and artifact envelope, resource
conflicts, broader queue fairness, and release-wide reliability remain open.
A successful short local trial is not evidence of long-term or distributed
reliability. No deadline, budget, GPU-placement threshold, grant or privacy
boundary was expanded to obtain this proof.
