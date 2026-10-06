# Supplied-document team trial

Date: October 5, 2026. Source baseline: `53911d0` on `main`, with the
implementation and test changes accompanying this evidence. Continues the
[package gate work](resource-api-evidence-2026-10-05.md). This is partial
COORD-A/C and OCT-202 product evidence; the useful-output gate remains open.

## What actually ran

An evidence analyst and a dependent reviewer worked on three explicitly supplied
repository-document excerpts. A coordinator dispatched them through existing
registered activation, derived grants, managed children and shared accounting.
The task was to distinguish demonstrated behavior from unproven release claims
and recommend one next validation step with source citations.

The excerpts contain 149 words. Their original paths, line ranges, full-file
digests and exact text are in [sources.json](document-proof/sources.json).
[Scenario A](document-proof/scenario-a.json) requested a final answer under 130
words and contributions under 100 words. [Scenario B/C](document-proof/scenario.json)
requested an 80-word final answer and 50-word contributions, with the reviewer
asked for corrections rather than a repeated assessment. No answer or preferred
conclusion was supplied.

The test saves the supplied brief and a reviewed two-assignment plan through the
existing ResourceService, agrees to it, and calls the normal `start_plan` path.
It does **not** test natural-language plan generation or the first-user setup
journey. Agents receive the explicit shared brief and permitted dependencies;
private exploration text is checked for absence in their recorded messages.
This profile still has no delegated file, MCP, hosted-inference or workspace
access. The documents were explicitly pasted into the saved brief, not read by
a model using a file tool.

All trials used installed `qwen3.5:latest` through loopback Ollama on this machine.
The plan retained its 11,096-token allowance: 3,500 for each contribution and
4,096 for coordination, with a 360-second plan deadline. Worker definitions
retained four steps, 120 seconds and 4,096 tokens; delegated execution uses its
smaller allocation. Trials B/C additionally submitted the same brief to the
analyst as a separate solo task with its original limits and no team history.
The two paths have different total allowances, so this is not an equal-budget
benchmark. Local loading and inference conditions were not controlled.

## Changes made from the trial

- The existing result inspector now exposes the exact pinned brief and revision
  under **What the team was given**. It does not substitute later brief edits or
  fabricate missing content in older responses.
- Participant usage now includes each participant's own allowance. Coordination
  overruns are explained even when the displayed plan total has unused capacity.
  Unused child allowances are not automatically transferred to the coordinator.
- The existing coordinator tool advertisements are shorter. Its `finish`
  description asks for a concise synthesis with citations instead of the generic
  request for a detailed answer. Schemas, dispatch idempotency, completion guards,
  budgets, permissions and cancellation remain unchanged. Trial C uses the exact
  same input as B to exercise this change.
- An ignored, opt-in application test retains actual execution views, usage,
  timing and a solo comparison in a fresh local directory. It fails on incomplete
  execution; there is no recorded-response substitute or automatic success label.

## Results and source review

All three team trials **failed**, with both contributions completed but no
accepted coordinator synthesis. The reviewed outputs and measured counts are
retained in [trials.json](document-proof/trials.json); the local raw directories
are `.lokai/document-team-live-2026-10-05-{a,b,c}`.

| Trial | Coordinator reported / own allowance | Team reported / total allowance | Team outcome | Solo outcome |
|---|---|---|---|---|
| A, original request | 4,681 / 4,096 | 8,260 / 11,096 | Token allowance failure | Not run |
| B, shorter requested output | 4,180 / 4,096 | 7,348 / 11,096 | Token allowance failure, 202.70 s | Timed out, 120.17 s |
| C, same B input, compact advertisements | 4,110 / 4,096 | 7,193 / 11,096 | Token allowance failure, 191.43 s | Timed out, 120.05 s |

In each team trial the coordinator made three inference calls and each worker
made one. Trial C's coordinator reported 3,485 input and 625 output tokens. Its
input history consumed most of its allowance despite the small source excerpts.
The overrun was detected from reported usage; it is not evidence of an exact
pre-inference token cap. Known unused worker reservations were released within
their own allocations. No team calls had unknown usage or outstanding holds in
the captured terminal view. Each timed-out solo call has **unknown usage and a
4,096-token hold**, not zero consumption or a refund. Its cause needs separate
provider/runtime timing investigation.

Trial A's first version of the helper asserted team completion before submitting
the solo comparison, so there is no solo or separate timing record for A. B/C
capture both outcomes before asserting. Scenario B/C hashes match. Model output
varied across trials, so the observed token reduction is not an isolated causal
measurement of the advertisement change. Shorter text did not fix completion.

Manual source review also failed the usefulness goal:

- The contributions generally distinguish an engineering gate from release
  readiness and include source IDs. That is useful partial behavior.
- A's reviewer upgrades S2's mention of risk/mitigation *bullets* into demonstrated
  "proper risk mitigation," which the excerpt does not establish. It claims
  software tests and installation/recovery checks validate model accuracy. B's
  reviewer similarly says a workspace test run validates model evidence.
- Across trials the reviewer mostly repeats the assessment. C avoids those
  particular unsupported claims but returns "verify actual-model team evidence
  ... or complete remaining gates," not one concrete validation action. It also
  exceeds the requested 50-word limit. A role named reviewer is not sufficient
  evidence of independent scrutiny.
- S3 states what that particular engineering run did not verify. It does not
  establish that those checks have never occurred anywhere. Several outputs
  flatten that distinction. The sources cannot establish complete release status.

Neither a useful combined result nor a team advantage was demonstrated. There
was no accepted solo answer for comparison. Do not mark OCT-202 or Sprint 1 done.

## Verification and next work

- `cargo run -p tetonic-arch-gate -- verify package` with `CARGO_BUILD_JOBS=2`:
  **passed**, including formatting, workspace Clippy, architecture and static
  quality checks after the advertisement change.
- `cargo test -j 2 -p tetonic-app --lib -- --test-threads=2`:
  **206 passed, zero failed, three opt-in live tests ignored**. The ignored
  document test was run explicitly above and failed; these unit results do not
  supersede the live failures.
- Focused plan execution regressions: **9 passed, three opt-in tests ignored**.
  Existing dispatch, completion, privacy, human handoff and cancellation checks
  remain in the path; no policy or gate was loosened.
- `node node_modules/vitest/vitest.mjs run tests/team-plan.test.tsx`:
  **10 passed**. Includes pinned versus edited brief, older missing brief, and
  coordinator exhaustion below the total plan allowance.
- `npm run build`: **passed**, retaining the existing large-chunk warning.
- Real browser inspection of trial A confirmed both retained contributions,
  the failed overall state, original source disclosure, exact per-participant
  allowances and coordinator explanation. The current map and styling remain.
  Screenshot: `.lokai/document-team-live-2026-10-05-a/budget-explanation.png`.
- `git diff --check`: passed. Logs are retained under `.lokai/document-plan-*`
  and `.lokai/document-team-live-{a,b,c}.log`.

The next implementation priority is coordination headroom in plan admission and
context management: account for repeated prompt history and synthesis work,
surface a plan that cannot fit before spending worker effort, and retain atomic
scope and allocation enforcement. Do not simply give coordinators unlimited
tokens or silently borrow delegated allowances. Separately diagnose the solo
deadline failures, including model load and response completion timing.

The next product validation must judge review quality against source claims,
contradictions, uncertainty and a concrete recommendation, with an accepted solo
baseline. Improve the general review/context contract without baking in answers
to this scenario. Broader capabilities, restart reconciliation, fresh installation
and fresh-user usability remain open as recorded in the sprint plan.

## Reproduce

From the repository root, choose a **new** proof directory and copy the scenario:

```powershell
$trial = Join-Path (Get-Location) ('.lokai/document-proof-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $trial | Out-Null
Copy-Item docs/epics/v5-reconciliation/sprints/october-1-coherent-workspace/document-proof/scenario.json $trial
$env:TETONIC_DOCUMENT_PROOF_DIR = $trial
Set-Location engine
cargo test -j 2 -p tetonic-app --lib local_model_document_plan_journey -- --ignored --nocapture
```

Requires the installed model and local Ollama on port 11434. The helper refuses
an existing database. It calls the real model and may fail or return different
answers; a passing test asserts completion, not accuracy or usefulness. Review
`result.json`, `solo-result.json`, `usage.json`, `snapshot.json` and `timing.json`
against the supplied sources. Keep failed trial artifacts. Do not publish the
database or local UI connection credentials.
