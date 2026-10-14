# Why the reviewed project failed, and what to correct

Date: October 14, 2026. Investigation against `4776e99`.
Evidence: [the reviewed twelve-assignment run](reviewed-project-qualification-2026-10-14.md),
its retained messages, tool arguments/results, reported usage and attempt events.
**No new agent/model attempts were started. No engine settings or implementation
were changed during this investigation.**

Implementation follow-up on October 14: the investigation below is historical.
The subsequent repair adds budget-aware working-context maintenance, useful
observation pages, narrower assignment contracts, structured incomplete receipts,
stopped-draft retention and cancellable commit contention handling. Current
ownership is documented in [harness assembly](../../../../architecture/ownership.md#harness-assembly-and-action-enforcement).
No hosted or local-model qualification attempt was restarted; development uses
scripted loopback providers and isolated temporary stores/files only.

The new offline delivery regression charges growing request inputs to the real
ledger. Two registered workers each read ten source files, write their own
output, inspect staged bytes and finalize within 80,000 tokens per worker.
Compaction calls are included in the ledger totals. A negative variant exhausts
allowance after writing and checks retained draft bytes, no published output,
coordinator failure details and known-usage settlement. The initial offline
regression also exposed a previously unreached bug: simultaneous independent
commits failed on ordinary writer-lock contention. Managed finalization now
waits briefly under cancellation and rechecks conflicts under the writer lock;
actual conflicts still fail rather than overwriting another writer.

Validation of this repair:

- `cargo test --locked -p tetonic-app --lib`: 224 passed, 5 ignored. The
  actual-model qualification fixtures remained ignored. Both new delivery and
  budget-stop regressions passed; the latter also checks that retained drafts
  identify the authorizing task and attempt, before finalization is reached.
- `cargo test --locked -p tetonic-core -p tetonic-transaction -p tetonic-inference
  -p tetonic-tools --lib --tests`: passed the selected unit/integration suites.
  This preceded the final mutation-provenance fix, which the full app suite
  exercises through the registered execution path.
- `cargo test --locked -p tetonic-run --test managed_service_tests`: 48 passed,
  covering managed cancellation, finalization and execution ownership.
- `cargo run --locked -p tetonic-arch-gate -- verify package`: passed formatting,
  all-target workspace Clippy, architecture and static quality checks.
- No production engine restart, workspace modification or budget increase was
  performed. These checks do not qualify the original project or model quality.

Remaining limits: compression is model-generated and can lose nuance; the
cost horizon is a heuristic rather than a reserved completion budget. A budget
that cannot fit even the minimum useful context still stops. Draft restoration
is manual review, not durable resumption of arbitrary effects. The separate
ordinary-shell/staged-verification issue described below remains open. The
original twelve-task project has not been rerun or requalified.

## Finding

Parallel dispatch worked. The immediate failure was the execution harness's
inability to bring useful work to completion within a cumulative token allowance.
Context assembly, observation sizing, request admission and draft finalization
each enforce a local rule, but do not jointly manage the cost of finishing work.

The repeated pattern is:

```text
Read small source excerpts
    -> ask the model again with the accumulated history
    -> request more excerpts or produce/rewrite a draft
    -> ask again with a still larger history
    -> next request fails local budget admission
    -> normal failure handling aborts staged output
    -> coordinator receives a generic failure
```

These six failures were not provider authentication, rate limiting, local GPU
capacity, sequential scheduling, timeouts, step limits or inaccessible tools.
Each terminal attempt event records insufficient request headroom. That does
not establish model quality or rule out other defects outside this run.

## 1. The loop manages context-window size, not the cost of completing a task

`tetonic-core/src/agent.rs::build_context` sizes a request against
`num_ctx - context_reserve`. It keeps the current assignment and complete recent
tool exchanges. These are useful invariants, but the target does not depend on
the remaining cumulative allowance. Repeated requests can consume the allowance
long before the model's context window is full.

Separately, `tetonic-app/src/resources/work_usage.rs::WorkUsageProvider::chat`
atomically checks the next request estimate plus 256 output tokens against the
ledger. `work_usage_request.rs::apply` caps that request's output and adds a
natural-language budget notice. It does **not** reserve a later inspection and
finalization request. The core's `reported_token_ceiling` checks stop after the
ceiling is reached; they do not adapt the working context before admission fails.

The notice is advice to the model, not a resource-management mechanism. Its
"further calls" count also assumes roughly unchanged input and excludes those
future calls' output; it cannot guarantee room to finish a growing transcript.

Concrete setup-worker progression:

| Model call | Reported input | Reported output | Following activity |
|---|---:|---:|---|
| 1 | 2,523 | 67 | Manifest and output-directory check |
| 2 | 3,906 | 141 | Search and two source reads |
| 3 | 5,307 | 198 | Search, continuation and search |
| 4 | 6,561 | 172 | Three source reads |
| 5 | 8,190 | 172 | Three source reads |
| 6 | 9,525 | 186 | One source read |
| 7 | 9,817 | 653 | Write draft |
| 8 | 10,859 | 730 | Replace draft |
| 9 | 12,008 | 36 | Inspect draft |
| 10 | Not sent | Not sent | Admission needed 21,261; only 8,949 remained |

The model reached inspection, but the system had no affordable route to its
next decision. More budget delayed the failure without fixing that mechanism.

**Correction:** connect the existing loop to a narrow host-supplied budget view
and a costed working-context policy. Keep the authoritative atomic check in the
existing ledger. Plan for another useful action, inspection and termination;
adapt or stop with recoverable progress before those become unaffordable. This
must not force a model to claim completion merely because the budget is low.

## 2. Small observations increase the number of expensive decisions

`agent/observations.rs` gives pageable calls a nominal 6,144-byte batch allowance,
subtracts framing, and divides it evenly among calls. Three reads get 1,536 bytes
each; four get 1,024; larger batches can reach a 512-byte floor. The local source
tool separately defaults to 2,048 bytes and caps a page at 4,096.

Original model tool-call arguments versus executed arguments show **58 read/search
requests narrowed by the host** in this run. For example, the HTML worker asked
for three 4,096-byte inspection excerpts and received pages around 1,536 bytes.
The source auditor's sixteen narrowed file reads included eight 512-byte pages.
These limits control individual response volume, not total tokens to finish.

Across the six workers there were 46 explicitly paginated file observations:
56,019 source characters and 21,918 framing characters. Framing was about 28% of
their displayed text before that text was resent in later requests. There was
no overlapping byte range among these explicit pages of the same file. This
excludes unpaginated reads and search snippets, which can overlap those pages.

**Important correction to the earlier shorthand:** the evidence does not justify
calling this predominantly pointless file rereading. A substantial part was
legitimate continuation of truncated observations, plus repeated transmission of
already-retained history. The latter happens even when the agent never repeats a
tool call.

**Correction:** size observations for total task cost and useful source units,
not a fixed tiny equal split. Keep explicit bounds, version checks and omission
markers. Account for actual returned bytes so an empty search does not waste its
share. Permit larger useful excerpts when the working budget supports them;
retain scoped retrieval for oversized documents. Do not blindly shrink pages as
tokens run low—the prior trials already showed that creates more continuation
calls. Also avoid the opposite extreme of unbounded whole-file dumps.

## 3. General workers have no active working-memory compaction

The registered general harness in `resources/registered/harness.rs::prepare`
uses the default empty compaction discipline. Its assembly explicitly attaches
neither a global briefing nor the coding context compiler. Consequently the
existing `maybe_compact` implementation is inactive for these workers.

This distinction matters: durable audit history and scoped recall already exist.
They do not automatically turn a worker's active transcript into a compact,
useful working set. A knowledge graph is not required to fix this defect.

The existing summarizer cannot simply be switched on:

- It triggers on a fraction of the context window, not task affordability. The
  recorded portable message estimates at failure were only approximately
  11,382–16,502 tokens excluding schemas, while the host context window was
  32,768. It can be too late even before considering native provider state.
- `Agent::summarize` omits run/task/attempt IDs in its `FabricCallMeta`.
  `WorkUsageProvider::chat` requires all three before admission. Enabling this
  path unchanged would fail the managed budget boundary.
- It has no explicit summary output cap. Its fold region can include the original
  user assignment; preserving only the protected system prefix is insufficient
  for a durable task contract.

**Correction:** keep the assignment, permissions and acceptance obligations
stable; maintain a compact, scoped working record of evidence, file versions,
decisions, pending checks and draft state. Preserve complete native tool
exchanges. Any summarization call must have managed attribution, explicit budget
and output bounds. Older evidence can remain in authorized audit/artifact storage
with retrievable references; it must not silently disappear or become a new
source of authority. Do not transplant the coding harness wholesale or add a
second memory database.

## 4. Request sizing has two different models and insufficient diagnostics

Core context sizing counts text/tool arguments at roughly 3.7 characters per token
with small envelope overhead. `tetonic-inference/src/request_budget.rs` uses
serialized UTF-8 bytes divided by three plus framing, schemas and private vendor
continuation. The core count does not include that private continuation.

| Worker | Remaining tokens | Next admission estimate including minimum output |
|---|---:|---:|
| Setup | 8,949 | 21,261 |
| Permissions | 16,523 | 20,133 |
| Coordination | 9,982 | 21,343 |
| Budgets | 14,935 | 19,695 |
| HTML | 20,538 | 30,498 |
| Source audit | 17,663 | 28,597 |

Admission estimates substantially exceeded the last *successful* calls' actual
input, but that is not an apples-to-apples comparison: the rejected requests had
new results and continuation. Exact next-request usage is unknown because they
were never sent. Private continuation is intentionally absent from portable
audit messages, so the exact rejected wire requests cannot be reconstructed
from this evidence. We cannot claim all six would have finished with a more
accurate estimator.

The estimator already avoids adding both portable and matching native assistant
representations. `provider_state` is skipped during ordinary serialization. This
is **not** a demonstrated double-counting bug. Serialized encrypted continuation
size remains an uncertainty, not proof of billed input tokens.

**Correction:** use a consistent request-cost breakdown for context selection
and admission, with provider-aware handling and conservative fallback. Retain
numeric estimated/actual counts, selected/dropped context sizes and estimate
components per request, including rejected requests, without recording private
payloads or secrets. Validate estimators against adapter fixtures and recorded
usage. Do not weaken the ledger or equate bytes, model tokens and dollars.

## 5. A last-turn rejection destroys the usable draft state

`Agent::turn` aborts staged mutations on `Failed`, `Canceled` and `Limited`.
`ToolsFinalizationDriver` commits only through managed completion. Thus a
successful `write_file` is a staged effect, not a delivered file. Four workers
wrote drafts, but none finalized. This explains zero delivered files despite
visible successful write calls; it is not evidence that the writes never worked.

The existing durable wait contract deliberately refuses staged mutations and
retained subprocess state (`Tools::checkpoint_ready`). A budget notice saying
"ask_human" does not supply a durable checkpoint for such work, and admission
may reject the next model request before it can ask anyway.

**Correction:** distinguish budget pressure from permanent execution failure and
preserve inspectable, scoped draft artifacts before terminating execution. Use
the existing transaction, artifact and managed checkpoint owners. Draft recovery
must be explicit and version-fenced; unverified drafts must remain distinct from
committed deliverables. Never auto-commit on failure or replay external effects.
The lifecycle/state change needs tests for cancellation, stale files, revoked
grants and restart, not merely a new UI label.

## 6. Coordination cannot act on the actual failure

`work/plan_execution.rs::contribution_text` rejects non-completed work with only
the name/state and a generic retained-output sentence. The grouped dispatch
result therefore contains `result: null` instead of the retained budget reason.
The coordinator escalated for details that the engine itself already knew.

Workers also receive every team's output contract in their protected current
request. The shared contract section was 2,806 characters in each of the six
worker inputs, including unrelated downstream tasks. This is a contributor to
repeat cost, not the entire cause: removing it alone does not fix the loop.

**Correction:** expose bounded, authorized failure category, budget situation,
draft/committed output state and retry constraints through the existing dispatch
receipt. Keep dependency failures specific and preserve the finish guard. Supply
only relevant dependency/interface contracts inline; make other permitted team
metadata available on demand. Do not disclose private teammate conversations or
silently borrow another assignment's budget.

## A separate blocker the failed run did not reach

Ordinary `run_shell` uses the committed workspace, while staged file reads can
see private changes. Registered finalization currently sets `verify_cmd: None`.
The qualification moved validator creation to an earlier task to work around
this mismatch. That did not cause these six failures, but would affect normal
write-then-test behavior. Reuse the existing verification overlay and governed
process path so agents can test their own staged changes without prematurely
publishing them or seeing other agents' private drafts.

## Implementation order and acceptance

These are repairs to the existing execution path, not a new orchestration system
or a broader MVP. Preserve the current UI and approved-work semantics.

| Order | Concrete slice | Existing owners | Evidence required before a hosted rerun |
|---|---|---|---|
| 1 | Add numeric request-cost diagnostics and an offline regression using these failure shapes; connect working-context selection to remaining allowance | `tetonic-core`, inference sizing, app usage adapter, existing memory ledger | Recorded-size histories cannot consume the inspection/finalization allocation unnoticed; rejection is typed; ledger remains atomic under parallel requests |
| 2 | Make source inspection efficient and keep a compact evidence-backed working set; reduce unrelated assignment context | Core observation/context code, tools source paging, registered harness, plan input assembly | Useful excerpts survive batching; active context remains bounded while source/version references remain retrievable; no orphaned tool results or lost assignment/authority |
| 3 | Preserve recoverable draft progress and return actionable failure receipts | Existing transactions/artifacts, managed checkpoints/finalization, work projection and team controller | Budget stop after a write leaves an inspectable draft but no false committed result; restart/revocation/conflict rules hold; coordinator receives the actual scoped reason |
| 4 | Connect write-then-verify through the staged workspace view | Existing tools verification overlay, process broker, managed finalization | Local deterministic test writes a script, runs it against staged changes, records actual verification, then commits or safely retains/aborts; failures cannot masquerade as success |

Slices 1 and 2 should be developed together at their boundary: adding a budget
hook without changing the working set merely creates an earlier stop. Reserve
inspection capacity proportionally to the selected request, not a magic constant
or a hardcoded domain workflow. Stop with a meaningful incomplete result if the
remaining authorized budget cannot support honest completion.

Use deterministic, local provider/adapter fixtures and actual temporary-file
effects for development. Existing observation tests use scripted successful
responses and tiny reported usage; they prove tool pairing and bounds, not this
full budgeted delivery path. New tests must include growing request histories,
realistic recorded usage, post-write denial, compaction attribution and failure
propagation. No hosted call is necessary to expose or fix those boundaries.

These checks will not prove a real model reliably solves the project. That
requires a separately authorized future qualification after the repairs. The
owner explicitly prohibited further attempts during this investigation.

## Investigation limits and local evidence

No prompt rewrite, model substitution, new grant, budget increase, engine
restart or paid rerun was performed. Analysis read the retained SQLite audit in
read-only mode and the current source. Scalar calculations are retained locally
at `.lokai/project-width-87372f80/offline-context-diagnosis.json`; original run
evidence remains under `reviewed-project-*` in that ignored profile.

Counts above describe this one failed run. The paginated-range calculation does
not measure semantic duplication across different source files, searches or
unpaginated reads. Provider billing discounts and exact rejected-request token
counts are unavailable. Current evidence supports an execution-integration
diagnosis; it does not justify blaming weak local hardware or guaranteeing that
one isolated context-size change will make the project pass.
