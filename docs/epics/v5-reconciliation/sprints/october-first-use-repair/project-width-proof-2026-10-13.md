# One project across several agents — October 13, 2026

**Acceptance: NOT PASSED.** The product goal is one substantial problem decomposed
by the Guide across 5–10 saved agents and 10–30 meaningful assignments, with actual
dependencies, concurrent execution and useful delivered artifacts. Several running
conversations or a scripted scheduler test do not establish that result. The live
tests demonstrate model-authored decomposition and six workers running together,
but zero completed project contributions. Only a separate single-worker harness
diagnostic delivered a file. No launch-readiness claim follows from this work.

## Product scenario

Create an offline operator kit for the October 25 single-user, single-machine
preview: source-grounded setup, capabilities, coordination and budget/recovery
guides; practical acceptance exercises; an honest JSON readiness checklist; a
self-contained accessible HTML entry point; independent review and validation.
The team must distinguish recorded source evidence from checks it actually runs.

Six persistent specialists were provisioned through the authenticated local API:
setup, capabilities, coordination, budget/recovery, experience building, and
independent review. All have scoped file tools; only the reviewer has the command
tool, with its existing approval requirements. Source snapshots include a path
and SHA-256 manifest. No plan, work assignment or successful output was inserted
by the provisioning helper. The Guide is asked through the actual product UI to
design the decomposition and save a proposal for review before dispatch.

This verifies configured-agent usage, not first-user agent creation through the
editor. It uses the previously authorized saved OpenAI connection and discovered
`gpt-6.1-sol` model. An isolated database and temporary working folder keep the
owner's normal work untouched. The initial project ceiling was 320,000 reported
tokens including coordination, raised explicitly to 384,000 for the recovery
attempt. The Guide's planning allowance is separately 32,768 per attempt. These
are engine usage allowances, not a provider billing guarantee or a total cap
across retries. Failures and unconfirmed usage remain recorded.

The existing admission policy was explicitly set to seven team slots (one
coordinator plus six workers), using the administrative CLI. Default team
capacity remains four. No capacity guard or uncertain-usage stop was disabled.

## Scale blockers repaired

1. **A plan could not exceed 12 assignments.** One bounded maximum of 32 now
   flows through plan validation, planning schema, dispatch schema, dispatch
   validation, durable acceptance/completion receipts and the review editor.
   The engine publishes the limit; older engines retain the editor's 12-task
   fallback. The 48,000-byte plan limit, cycle/reference checks, budget rules and
   execution capacity are unchanged.
2. **Hosted streams were killed after two minutes even while progressing.** The
   first live proposal attempt failed at that transport boundary. Streams now
   have separate two-minute header/idle and 15-minute total limits. Nonempty data
   renews only the idle deadline; the agent deadline and cancellation still apply.
   There are no transport retries, and unconfirmed usage is retained. The UI
   identifies this timeout instead of suggesting a generic run-limit failure.
3. **Context trimming could orphan native tool results.** Calls, their provider
   continuation metadata and results are now retained/dropped as whole exchanges.
   The latest user intent and protected prefix survive trimming; compaction cuts
   only between complete exchanges. Core regressions exercise native Responses
   request construction after trimming, not just message counts.
4. **Workspace context was hardcoded at 8192.** The host now has an explicit
   `workspace_execution.context_tokens` setting. The default stays 8192 for local
   compatibility; the isolated hosted trial uses 32768. This does not increase
   saved agent usage allowances or provide automatic memory/compaction.
5. **A growing workspace could prevent new Guide conversations.** The automatic
   overview failed at 16000 bytes after the two 21-assignment attempts. It now
   drops optional detail rows within that bound, marks the overview partial and
   records omitted counts. Authoritative scope, aggregate usage and work counts
   remain intact; the Guide can request details through its existing tools.

## Deterministic regression evidence

`plan_width_tests.rs` runs the existing managed runtime with six saved agents and
20 dependent assignments. A scripted inference provider holds the initial calls
until all six arrive, checks real dependency contributions in downstream input,
and holds one slow branch until another branch starts its successor. All 20
receipts reach the coordinator, the final synthesis occurs, and a repeated start
does not create duplicate tasks. Saved agent definitions remain unchanged and
usage reservations are released. A separate three-worker-capacity variant checks
that queued assignments make progress.

This provider is deterministic test scaffolding. It is **not** evidence that a
frontier model creates a good plan or finishes useful work.

Validation:

- Application suite after the task-bound change: 209 passed, five existing ignored.
- Plan/huddle storage tests: seven passed.
- Plan editor tests: 30 passed; frontend production build passed.
- Egress suite after the timeout change: 33 passed, one existing ignored; includes
  progressing/silent streams, total lifetime, empty chunks, size/decode errors
  and cancellation using paused time.
- Failure-message tests: two passed.
- Core suite after protocol-safe context trimming: 49 passed.
- Host context setting regression: passed; defaults, overrides and bounds checked.
- Guide/director tests after overview bounding: 19 passed. Includes a real
  workspace with 64 execution records, private-conversation exclusion and pure
  tests retaining whole IDs, totals and scope in oversized Unicode overviews.
- Rebuilt CLI and package gate passed (formatting, workspace Clippy, architecture,
  static quality checks).

## Live run evidence

The first planning run obtained real resource inventory, then its second model
call hit the former 120-second stream deadline. It reported 3,781 tokens from the
first call; the interrupted call retained 28,987 tokens of unconfirmed allowance.
No proposal, worker assignment or output file was created by that failed run.
The attempt remains recorded; it was not erased to make the workspace look clean.

After the transport fix, the same conversation was continued through the Guide
UI. It generated a real 21-assignment proposal, with six independent initial
assignments and downstream guides, exercises, readiness, interface, review,
repair and validation dependencies. The owner review flow started this proposal
without editing its work decomposition.

- **First execution:** all six workers reached running concurrently and performed
  real file reads. Each failed when context trimming left a tool result without
  its native provider call. No deliverables were committed. The coordinator
  correctly asked for human direction; the plan was explicitly stopped.
- **Recovery after the protocol fix:** the built-in unfinished-work flow retained
  all 21 outstanding assignments. Initial reading allowances were increased to
  24,576 each and the accepted total became 382,224. Six workers again ran at the
  same time. Five stopped after repeated reads, consistent with source evidence
  dropping out of the fixed 8192-token context; one had a transport failure.
  Again no project deliverable was committed. The plan was stopped through its
  normal control. This attempt reported 107,933 tokens including coordination.
- **Focused context diagnostic:** with the new host context set to 32,768, one
  worker read five sources once, listed the root and staged an evidence file.
  At a 24,576 cumulative allowance it stopped before finalization. Repeating
  this isolated diagnostic with a 32,768 saved allowance completed and committed
  `diagnostics/context-retention.md`; four provider calls reported 28,660 tokens.
  The file was independently read from disk. This is a harness diagnostic,
  **not a completed assignment in the project**. An intervening diagnostic was
  canceled after its allowance update was rejected for a noncanonical path; it
  remains in history rather than being hidden.
- **Replanning:** a new request for a lean 12–16 assignment version initially
  failed before model admission because the automatic workspace overview exceeded
  16,000 bytes. After bounding that overview, the exact saved message was retried
  from the UI and admitted successfully. It saved a 16-assignment proposal, then
  exceeded its planning allowance before a closing response (34,118 reported).
  The proposal remained reviewable. Its original total was 214,768; the operator
  used the existing editor to raise only allocations to 382,912 including 32,768
  coordination, without changing assignments, ownership, dependencies or grants.
- **Lean execution:** six initial workers ran together. They did use bounded
  source line ranges; the tool log's byte summary describes the original file,
  not necessarily the returned excerpt. One worker staged an evidence write.
  Capabilities then reported 24,769 against its 24,576 assignment allowance
  (193 over). The existing tree-wide overrun rule closed further inference and
  canceled its five peers. No project file was committed; ten downstream
  assignments remained unstarted. This plan reported 81,405 tokens and retained
  68,456 tokens of unconfirmed allowance. More paid project retries were stopped.

Execution roots: `e227c077-67a2-4600-9a3f-ff0e6572201c` (first attempt),
`e39a0203-76b8-43cf-86b7-3417b92553da` (recovery). Successful diagnostic:
`42a91a0a-7050-414a-baf6-4797d0e2eb5a`. Lean proposal conversation:
`65d58127-3975-4238-94df-953f804b3976`.
Lean execution root: `e1c136c5-2f60-4251-b1c2-6cc29437eae3`.

The isolated workspace's final recorded usage is **343,645 reported tokens plus
169,676 held/unconfirmed tokens**, across planning, failed projects and diagnostics.
Held amounts are not known consumption or a currency bill. Final snapshots show
no running/waiting/canceling tasks and no pending tool approvals. Engine processes
remain available on ports 18739 and 18740; the read-only polling helper was stopped.

## Remaining product findings

Follow-up: [budget-aware execution and scoped exhaustion](budget-aware-execution-2026-10-13.md)
implements and deterministically qualifies the budget fixes identified below.
The live-run evidence and failed acceptance recorded here remain unchanged.

- Context retention and cumulative usage are different resources. The Guide's
  original decomposition underbudgeted repeated source input. The later prompt
  explicitly requested focused source ranges and concise intermediates; this is
  operator intervention, not evidence that allocation is already automatic.
- **Highest next priority: budget-aware execution and scoped exhaustion.**
  `WorkUsageProvider` caps output by remaining cumulative allowance without first
  reserving for the next call's input. It checks reported input-plus-output after
  the call. Workers are not given a changing per-step remaining-usage instruction.
  `require_bounded_work_reports` intentionally blocks the entire delegated tree
  after any allocated branch overruns, even when the root has unused allowance.
  Simply raising limits or disabling this guard would not resolve the product
  failure. Add conservative pre-call headroom and clear budget feedback; define
  a bounded finish/park path and an explicit policy for individual exhaustion
  versus a true root overrun. Prove preserved independent progress and safe
  resumption with deterministic tests before another live project retry.
- General registered workers still trim older history rather than automatically
  compacting it. A larger configured context postpones that boundary.
- Some pre-HTTP validation failures are counted as unknown provider usage by the
  common metering wrapper. Holds were not manually cleared. Typed pre-dispatch
  failure accounting needs a separate correction.
- The map can show earlier assigned work as “Unassigned” when an agent avatar is
  attached to its later assignment. The detailed assignment retains the agent.
  Wider-project visual clarity remains a product issue; the design was not
  changed during this execution qualification.

Ignored evidence profile: `.lokai/project-width-87372f80/`. Its `session.json`
locates the temporary working folder. It retains input hashes, saved agent/team
configuration, usage, task/approval snapshots and sampled execution states.
Connection credentials and raw provider keys are not published in this record.

## Code checkpoints

- `235efbec` — bounded 32-assignment plans and scheduler/editor regressions.
- `bf513bb8` — progressing hosted streams survive the former two-minute cutoff.
- `ad7c58f8` — protocol-safe context trimming and compaction boundaries.
- `d48d05f7` — explicit host context size independent of cumulative usage.
- `25076c48` — bounded partial Guide overviews as work inventory grows.

These checkpoints were committed locally on `codex/working-setup`; this run did
not push or merge. Production UI styling and the map layout were not changed.
