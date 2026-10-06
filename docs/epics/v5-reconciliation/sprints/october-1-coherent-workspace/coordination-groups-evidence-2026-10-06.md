# Grouped coordination and inference timing

Date: October 6, 2026. Source baseline: `ce88f19`; final implementation: `270c3a4`.
Continues the [failed document trials](document-team-evidence-2026-10-05.md).
This is partial OCT-202/COORD-A evidence, not a complete sprint exit.

## Change and integration

The coordinator can request several agreed assignments in one
`dispatch_assignment` call using `assignment_keys`. It selects their order;
the existing host dispatcher runs each key sequentially and supplies declared
dependency results. This avoids an inference round solely to relay a result to
the next worker. Single-key dispatch remains available for intermediate judgment.

The new group helper feeds the same `DispatchCall` channel. It does not create
another scheduler, runtime, tool authority, store or budget ledger. Every key
still passes the existing pinned-definition lookup, current direction lookup,
dependency check, scoped context construction, grant derivation, live parent
binding and managed child admission. No total, child allocation, deadline,
permission or finalization guard was increased or disabled.

Selections must contain 1–12 unique agreed keys. Unknown keys, duplicate keys,
empty groups, mixed singular/plural fields and extra parameters are rejected
before the first dispatch. The model-facing schema advertises one array shape,
including for a single assignment; the prior singular field remains accepted
for compatibility. A group stops on a blocked assignment or human wait,
retaining earlier results and exposing outstanding keys. It does not implicitly
start prerequisites that were not selected. The caller supplies dependency order;
an incorrect order is rejected by the existing readiness check. Cancellation
drops the group wait and prevents later dispatches. Retrying completed keys reads
their retained results without starting new workers. Only receipts returned by
the existing reconciliation path advance the whole-plan completion guard.

The local inference adapter also emits a payload-free
`inference_first_reasoning` timing event, correlated to the managed attempt.
Together with existing first-chunk, first-content, stream and backend timings,
this helps distinguish model activity from loading or transport delays when a
deadline interrupts a request before usage is reported. It does not log reasoning
text, alter thinking mode, or turn absent usage into zero consumption. The opt-in
document helper enables only the existing performance tracing target.

## Verification scope

New regressions cover a complete grouped dependency chain, wrong ordering,
premature completion, idempotent group retries, parent stop before the next key,
human wait and partial-result preservation on failure. Fixture inference proves
runtime behavior, not the quality of model-generated work.

The unchanged source excerpts and reviewed plan from
[scenario.json](document-proof/scenario.json) are the actual-model input. Worker
allowances remain 3,500 each, coordinator allowance 4,096, total 11,096, with the
same local model, agent definitions and deadlines. The coordinator's general
dispatch instructions and tool schema change with the implementation. No desired
answer is supplied. Unit success alone is not a claim of successful real-model
coordination.

## Actual-model trials

Trials D/E/F retain the exact scenario used for B/C. All use installed
`qwen3.5:latest` on loopback Ollama. These are reviewed-plan execution checks,
not natural-language plan generation, connected file/MCP access or a fresh-user
journey. Model responses are not scripted. Raw local directories are
`.lokai/document-team-live-2026-10-06-{d,e,f}`; compact results and accepted answers
are retained in [group-trials.json](document-proof/group-trials.json).

| Trial | Interface | Coordinator reported / own allowance | Team outcome | Solo outcome |
|---|---|---|---|---|
| D | Group support with singular/array alternatives | 4,263 / 4,096 | Failed after both contributions; 205.47 s | Timed out; 120.19 s |
| E | Explicit dependent-group instruction, same two advertised forms | 4,330 / 4,096 | Failed before either worker started; 107.65 s | Completed; 109.03 s |
| F | One advertised array form | 2,754 / 4,096 | First worker timed out; unanswered recovery question expired with plan; 359.30 s | Timed out; 120.16 s |

D still used two one-key groups, so the coordinator made three inference calls
and did not save a relay round. Its workers reported 2,436 and 1,492 tokens;
total reported team usage was 8,191. This was not a completion fix.

E attempted `finish` before dispatch; the existing completion guard rejected it.
It then supplied both `assignment_key` and `assignment_keys` in one call. The
selection validator rejected the conflicting forms without dispatching a worker.
Three inference calls consumed the coordinator allowance. This concrete failure
motivated the final single advertised array schema, rather than another increase
in instruction length or budget. The singular parser remains for compatibility;
mixed arguments remain invalid.

F uses the final implementation, whose first recorded model action was
`dispatch_assignment({"assignment_keys":["assess","review"]})`. The first worker
timed out before delivering a contribution. The group returned that failure and
did not start the dependent reviewer. The coordinator then called `ask_human`,
asking whether to retry, assess the evidence itself, or request new evidence.
The unattended helper did not answer that question; the plan failed at its
original deadline. The coordinator made two inference calls and stayed within
its allowance, but no contribution completed. The analyst's usage is unknown
with 3,500 tokens held; the reviewer made no calls. This is real grouped dispatch
and bounded failure handling, not
successful two-worker completion or proof of reduced live coordination cost.

The team and solo paths have different total allocations. Hardware loading and
inference conditions were not controlled; these times are observations, not a
team-speed benchmark. Package checks ran during F. None of these changes makes
the provider's reported input/output token total an exact pre-call spend cap.

## Timing diagnosis and output quality

D's solo request first reported reasoning at 26.63 seconds from admission start,
after about 22.61 seconds before the HTTP request and 4.03 seconds to response
headers. It did not deliver a completed usable response before its 120-second
deadline. This rules out "no model activity at all" for that attempt; the timing
event does not establish what the model was doing throughout the remaining time.
The terminal ledger retains unknown usage and a 4,096-token hold.

E's solo request first reported reasoning at 26.94 seconds, first visible content
at 105.27 seconds, and inference completion at 108.23 seconds. The solo work
completed at 109.03 seconds. The local profile's load/admission cost and model
generation both matter; these observations do not justify silently changing
thinking settings, retention or deadlines. No reasoning payload was logged.
That solo call reported 626 input and 3,147 output tokens, despite its concise
accepted answer. Visible answer length alone is not an adequate effort estimate.
F's solo run again timed out, retaining unknown usage and a 4,096-token hold.
All three team proof invocations failed; only E's solo component completed.

D's reviewer largely repeats the analyst. Its recommendation combines a full
test run, restart checks and user validation, rather than one bounded next step.
It treats S3's statement about this run as evidence those checks were not
performed generally. E's accepted solo answer is concise and cites the source
IDs, but says a full-workspace run with restart would validate usability. That
does not establish fresh-user usability. An accepted answer is now available
for comparison; it is not verified research or evidence of team advantage.

## Engineering verification

- `cargo test -j 2 -p tetonic-app -p tetonic-inference --lib -- --test-threads=2`:
  application **210 passed, 3 ignored**; inference **139 passed, 2 ignored**.
  The ignored document proof was run separately; its live failures are not
  superseded by library success.
- After the final single-shape advertisement change,
  `cargo test -j 2 -p tetonic-app --lib plan_ -- --test-threads=2`:
  **14 passed, 3 opt-in tests ignored**. This includes an assertion on the actual
  inference request's advertised schema, along with group dispatch and existing
  singular dispatch, human handoff, cancellation and completion regressions.
- `CARGO_BUILD_JOBS=2 cargo run -p tetonic-arch-gate -- verify package`:
  **passed** after the final source changes: formatting, workspace Clippy,
  architecture and static quality checks.
- The inference response regression includes a private reasoning canary; the
  delivered content, tools and usage remain unchanged and zero-retention policy
  remains enforced. The new timing event contains no reasoning payload.
- `git diff --check`: passed. D/E/F scenario bytes match the committed scenario.

A focused-test relink attempted while E's executable was still running failed
with Windows `LNK1104`. It was rerun successfully after E exited; no user server
was stopped. Logs are `.lokai/group-app-inference-tests.log`,
`.lokai/group-dispatch-final-tests.log`, `.lokai/group-package.log` and
`.lokai/document-team-live-{d,e,f}.log`. No UI code or running user engine was
changed, and this slice does not claim a full-workspace test run.

## Remaining work and reproduction

Coordination should not require a model turn merely to carry a dependency result
between already agreed assignments. Grouping addresses that overhead; it does
not solve prompt-growth admission or reserve reliable synthesis headroom. The
ledger still detects overruns from reported usage, holds unknown consumption,
and does not lend unused child allowances to a coordinator. Plan admission needs
an honest headroom estimate and a way to surface infeasible work before spending
worker effort, without presenting a heuristic as exact token enforcement.

Review usefulness still needs separate validation against supplied sources,
including corrections, uncertainty and one actionable recommendation. A reviewer
label and a completed run are insufficient. Repeatable solo completion within
the supported local profile is also still open. Broader delegated tools, restart
reconciliation, fresh installation and fresh-user usability remain sprint work.

F also exposes a recovery affordance gap: a model can offer "retry" or doing the
work itself after a terminal child failure, while the current plan reuses that
failed child's receipt and still requires all agreed contributions. A human
answer does not grant retry authority or bypass the completion guard. Recovery
choices need to reflect supported engine actions, rather than sounding actionable
only in conversation. This is separate from the group transport contract.

Use the [document proof reproduction steps](document-team-evidence-2026-10-05.md#reproduce)
with a new directory and the unchanged committed scenario. The helper saves both
team and solo outcomes before asserting completion. Review accepted answers
against the supplied excerpts; retain failed trials and do not publish the
database or local connection credentials.
