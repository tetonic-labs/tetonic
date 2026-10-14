# Verify pending work before delivery

Date: 2026-10-14. Scope: the immediate MVP's write → check → repair → deliver
path. No UI redesign, engine restart, live inference, or large-project retry.

## Product failure addressed

File tools stage an agent's edits and can read those pending bytes. Ordinary
shell commands operate on committed workspace files. Previously an agent could
try to validate a new program against missing or old files. Registered general
agents had no governed way to request a check of their pending version.

## Implemented behavior

- Existing `run_shell` accepts `view: "staged"`. Its schema explains when to use
  that view, while omitted/`workspace` preserves ordinary command semantics.
- The host binds the transaction, patch digest and disposable directory before
  action canonicalization, approval and capability issuance. The sink validates
  that exact binding; changed edits require a new approved action.
- The existing process executor, capability consumer, platform sandbox,
  cancellation signal and limits run the command. No alternate executor or
  ungoverned model-driven finalization command was added.
- The transaction owns the verification copy and receipt. Failed checks block
  delivery. The agent can repair rejected edits and recheck; any edit after a
  successful check invalidates that evidence and requires a fresh passing check.
- Commands that modify/remove copied source cannot certify the original patch.
  Command-generated files are not imported into the patch. Copied inputs enter
  the transaction's existing conflict detection.
- The model receives an explicit pass/fail result, patch identity and a recovery
  instruction. Ordinary live-folder shell results warn when pending edits were
  not checked. The existing inspector/tool history carries these results.
- A model's `finish` claim after failed verification cannot publish the patch;
  managed finalization retains the draft through the existing stopped-work path.

## Ownership

`tetonic-domain::ToolHost` adds a default pre-authorization parameter-preparation
boundary. Core invokes it on its existing owned blocking worker. The registered,
MCP and skill wrappers forward it. `tetonic-tools::staged_shell` binds and checks
the shell view, `tetonic-transaction::staged_check` owns pending-version checking,
and the existing app approval adapter accepts only the host-derived staged
directory. No filesystem copy or tool-specific decision was put in the core loop.

## Verification and limits

The new provider-path regressions use created agents, actual local shell
processes, owner approvals and managed finalization. OpenAI, Anthropic and Google
responses are scripted: these tests prove the plumbing, not real-model judgment.
The passing repair scenario and the failed-check/premature-finish scenario both
passed. Transaction/tool tests cover stale/revoked/changed actions, cancellation,
evidence invalidation, altered source, changed dependencies, private concurrent
checks, tampered staging and bounded copies.

Final local validation:

- Core, tools and transaction package tests: 208 passed, one previously ignored
  syntax-revert test. Includes five new transaction and two new tool regressions.
- App library: 225 passed and five ignored on the full run; the remaining
  agent-editing test falsely matched `write_file` inside the shell description.
  Changed that test to inspect exact advertised tool names; its focused rerun
  passed. No production grant policy was relaxed to make it pass.
- `cargo run --locked -p tetonic-arch-gate -- verify package`: passed formatting,
  workspace/all-target Clippy, architecture and static quality checks.
- `git diff --check`: passed. Current UI was not changed.

The copy excludes build/dependency artifacts such as `node_modules` and `target`,
as well as protected directories and credential names. It has a 20,000-entry /
256 MiB limit and checks cancellation while traversing. Commands requiring
omitted dependencies can fail; there is no silent fallback to original files.
Check execution keeps the existing platform containment limits, including
Windows' reported lack of OS filesystem/network filtering. A disposable copy is
not a claim that arbitrary shell side effects can be rolled back.

Agents must still choose useful checks. This change does not require a test suite
for every non-code deliverable, invent acceptance criteria, or establish that the
six-agent/twelve-assignment live workload now succeeds. That experiment remains
unrun, respecting the user's instruction against further attempts.
