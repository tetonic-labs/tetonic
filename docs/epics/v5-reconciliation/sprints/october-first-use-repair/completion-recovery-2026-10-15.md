# Repair before completion

## Product problem

An agent could call `finish` after a failing staged check. Finalization refused
to publish its files, correctly, but the model loop had already ended. The agent
could not use that rejection to repair the output in the same attempt. A team
coordinator's outstanding contributions were classified as invalid tool arguments
rather than unfinished work.

## Change

The loop now asks its tool host for completion readiness before accepting a
finish call, a conversational answer, or the existing effort-cap answer fallback.
Known blockers become tool results (or conversation feedback for prose answers)
and appear through the existing step/event channel. The agent can repair and
check again within the original attempt. No extra attempt, allowance, approval,
or model call outside the existing loop is created.

Repeated premature claims count toward the existing no-progress cutoff. Step,
time and token limits still apply. Cancellation and execution authorization are
checked before and after the host readiness call. A host failure fails closed.
Mixed completion/action batches are rejected before any effect, with a response
for every call, so a finish cannot silently skip the rest of its batch.

## Ownership

- `tetonic-domain::ToolHost` defines the neutral, read-only readiness contract.
- `tetonic-core` owns feedback, attempt limits and cancellation; it knows no file
  check commands or team planning rules.
- `tetonic-transaction` owns verification state and shares the same condition
  between readiness and approval binding. A later edit invalidates a prior check.
- `tetonic-tools` exposes that state through the contract.
- The existing registered host reports pending team contributions; MCP and skill
  hosts forward readiness. No new orchestration service is introduced.

Readiness never approves or publishes work. Managed finalization still validates
and commits it, and publication can still fail if the workspace changes. Existing
draft retention remains the fallback when the attempt cannot deliver.

## Scope and evidence limits

This repairs a known completion boundary; it does not judge arbitrary output
quality or require code checks for every kind of task. A successful command is
only evidence about that command. An agent that never requested a staged check
does not gain a new universal verification requirement from this change.

The existing map, navigation and agent setup are unchanged. Real-model large
project qualification remains paused. Scripted provider decisions prove the
engine mechanics, not whether a model will choose a good plan or useful tests.

## Validation

- `cargo test --locked -p tetonic-core -p tetonic-domain -p tetonic-tools
  -p tetonic-transaction --lib --tests --quiet`: 258 passed, one previously
  ignored. Includes completion recovery, bounded repeated rejection, prose and
  effort-cap bypass prevention, atomic batch rejection, cancellation/revocation,
  host failure, staged-check invalidation and publication-conflict regressions.
- `CARGO_INCREMENTAL=0 cargo test --locked -p tetonic-app --lib
  --test sub01_loop --quiet`: 245 passed, five previously ignored. This includes
  the real created-agent repair path through all three hosted-provider adapters
  with scripted responses, actual temporary-file edits, explicit owner approvals,
  failed/passing commands and publication. Six new premature-finish recovery cases
  cover individual grants and the Build & verify pack. Repeated false completion
  retains the failed draft without publishing it or granting more steps. A
  composite MCP/skill-host test preserves the pending-contribution guard.
- `CARGO_INCREMENTAL=0 cargo run --locked -p tetonic-arch-gate -- verify package`:
  passed formatting, workspace/all-target Clippy with warnings denied,
  architecture and static quality checks.
- `git diff --check`: passed. Across the selected test suites, 503 tests passed
  and six existing tests remained ignored.

The first final regression/Clippy run exhausted local disk space. One long-running
application test could not persist its fixture state, and Clippy could not write
incremental caches. Application artifacts were cleaned and the application suite
rerun successfully with incremental compilation disabled. No test or lint was
disabled to address this resource failure.

The existing native multi-observation test now delivers and then finishes in a
separate response; it still checks all six observations and their original byte
allowance, and also retains the delivery receipt in the model's context.

No paid inference, large-project reruns, production engine restarts or writes to
the user's working files were performed. The running engine needs a rebuild and
restart to load the change. The separately pending first-use tutorial edits are
outside this engine change.
