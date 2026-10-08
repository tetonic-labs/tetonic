# BASE-001 — Integrate retirement and establish the working baseline

Date: October 8, 2026. Status: complete with the validation limitation below.
Scope: step 1 of the architecture tidy-up only.

## Integrated history

The working branch is `main`. It was fast-forwarded from `76fcd6f7` to
`7ff613ec` (the cleanup branch had already merged that feature commit).
The cleanup worktree remains on `retire/legacy-chat-and-world`; it is not a
second product branch to build on. The `web/` tree is byte-for-byte unchanged
from `76fcd6f7`. No live user database was migrated or cleared during this work.

The retirement commits are `f8ea9d5f`, `ec4c51e5`, and `cfed6b11`.
Their integration does not establish distributed execution, automatic crash
resumption, or completion of the October MVP gates.

## Deliberately retired

The owner explicitly approved retiring the legacy coding chat and the world
action server. The following are no longer supported product entrypoints:

- Terminal coding chat/TUI, old session start/resume/turn lifecycle, and the
  independent CLI/daemon assembly used by those paths.
- `tetonic-server`, direct world execution, world adapters and the world charter.
  Existing Village clients of that server need a future governed world harness;
  this baseline does not claim Village compatibility.
- Disconnected fleet/operator/squad supervisors, volatile ThoughtStreamHub, and
  the prototype KeeperRegistry/RunnerClient lifecycle.
- The `tetonic-eval` executable and its chat-driven evaluation path.

The supported `tetonic` commands remain `ui`, `job`, `control`, and `estate`.
Persisted agents, teams, scopes, work, budgets, managed execution, approvals,
egress, tools, MCP and skills remain on their existing production paths.
Coding definitions, coding_pack, orchestrator/index/LSP/bench libraries remain;
their existence is not a claim that every library is exposed as a product feature.

## Behavioral coverage reconciliation

Paths below are relative to `engine/`. This is a map of the contracts retained
or deliberately retired, not a claim that every deleted assertion has an exact
replacement. Tests that only pinned the old source layout are not substitutes
for the behavioral evidence listed here.

| Removed suite or behavior | Current evidence | Disposition |
|---|---|---|
| `recovery_contract_tests`: reopen history, approvals and in-flight journal | `litho/tetonic-cli/tests/control_cli.rs`; `litho/tetonic-app/src/resources/registered_executor/reconstruction_tests.rs`; new `local_workspace/providers/tests/process_recovery_tests.rs` | Current history and registered continuation tested; old live-session maps retired. Abrupt process loss now has a separate-process regression. |
| `iface02_approval`: concurrent sessions, wrong/stale/duplicate replies, real tool execution | `local_workspace/providers/tests/shell.rs`, `local_shell.rs`, new `parallel_approval_tests.rs` | Current owner approvals exercise real shell effects. Identical commands and call IDs from different agents cannot share approval; stop invalidates the remaining decision. |
| `session_authority_tests` / `work04_lifetime`: cancellation, late completion, child lineage, lease ownership | `litho/tetonic-app/src/execution_regression_tests.rs`; `mantle/tetonic-run/tests/managed_service_tests.rs` and `tests/support/managed_{cancellation,delegation,deadlines,activations}.rs` | Generic lifetime guarantees retained; old one-turn-per-chat/session-map behavior retired. |
| `turn_attestation_tests`: sealed output and artifact access | `execution_regression_tests::governed_final_output_uses_existing_private_artifact_access`; managed service attestation/finalization tests and `mantle/tetonic-run/tests/workfin01_claim.rs` | Managed artifact path retained; old chat completion composition retired. |
| `fin03_contracts` / finalization portions of `v4_audit_corrections` | `mantle/tetonic-run/tests/managed_service_tests.rs`, `execution_claim.rs`, `workfin01_claim.rs`; `litho/tetonic-app/tests/obs02_finalization.rs` | Managed claim, cancellation, finalization ordering and error propagation retained. No assertion that unmanaged fabric completion is now unified. |
| `h2_3_spawn_budget`: legacy chat-spawn reservations | `strata/tetonic-memory/src/work_budgets_tests.rs`; managed activation/delegation tests; current workspace usage and coordinated-plan tests | Persisted product allowances, concurrent allocation, child attribution and stop scope retained. Old chat child-construction path retired. |
| `cli_assembly_parity`, old daemon/session composition | `job_launch::tests`, `compute_plane::tests`, CLI control and local UI tests | Current assembly and operator paths tested. Legacy CLI/daemon parity no longer a product requirement. |
| `work06_session`, old routed-role conversation continuation | Scoped context/compiler tests; registered reconstruction/human-wait tests; current Guide and plan tests | Current context/continuation retained; the old router's session-bound role behavior retired. |
| `tetonic-eval` graders/corpus | No equivalent end-to-end coding evaluation runner verified in this baseline | Explicit gap. Do not describe retained library tests as replacement for FileBoundary, ProtectedFiles, MutationTest or coding-corpus outcome evaluation. Preserve the retired implementation in Git for a future governed coding harness. |

## New proof and narrow recovery correction

1. Two saved agents reach approval concurrently on the actual local workspace
   path. Both request the same shell command, arguments and provider call ID.
   Approving one executes exactly one file append; the other stays pending.
   Crossed digests, replay and approval after cancellation are rejected.
2. A child process opens a fresh workspace database, creates an agent, starts a
   registered task and reaches a real pending shell approval. The parent kills
   that process without cleanup, crosses the persisted execution deadline, and
   reopens the same database. Work identity, run receipt and journal sequence
   survive; the task requires recovery, the old approval cannot run, and repeated
   submission does not create a replacement executor or work item.
3. Startup previously considered lease expiry without the shorter execution
   deadline. `tetonic-run::detect_recovery_required` now also detects active
   leased/starting/running attempts whose task deadline has elapsed. It uses
   the existing durable recovery projection; it does not mint execution authority,
   reset a budget/deadline, or mark unknown effects as completed. A managed-runtime
   regression checks the exact deadline boundary and excludes canceled work.

The subprocess helper is marked ignored for normal standalone discovery and is
explicitly invoked by the parent test. It uses temporary state, a loopback model
fixture and no paid inference. The existing durable human-wait tests separately
verify exact supported checkpoint restoration and denial after changed settings,
missing audit, revocation or stop. Ordinary killed execution is not that checkpoint
contract.

Recovery limits remain explicit: this correction applies on startup. Before a
deadline or lease expires, a persisted running record alone cannot prove that a
worker has died. Continuous liveness reconciliation, clearer stale-status UI,
and general crash resumption remain later work; this baseline does not add them.

## Validation

All commands below completed against the integrated main tree and the new changes:

| Check | Result |
|---|---|
| `cargo run -p tetonic-arch-gate --offline --locked -- verify package` | Passed: formatting, workspace/all-target Clippy with warnings denied, architecture and static quality. Nested Cargo checks used `CARGO_NET_OFFLINE=true`. |
| `cargo test -p tetonic-app --lib --offline --locked -- --test-threads=1` | 152 passed, 4 ignored (three opt-in local-model/scenario fixtures plus the child-process helper actually invoked by its parent test). |
| `cargo test -p tetonic-app --offline --locked --test obs02_finalization --test workfin01_finalization --test workfin02_terminal --test work02_execution --test work03_door --test work05_binding -- --test-threads=1` | 89 passed across six integration/contract suites. Some are source-layout assertions; they are not counted as behavioral replacement evidence above. |
| `cargo test -p tetonic-run -p tetonic-cli -p tetonic-memory --offline --locked -- --test-threads=1` | 115 run tests, 14 CLI tests (including six separate-process operator journeys), and 184 memory tests passed. |
| `tetonic ui/job/control/estate --help` | All four surviving commands load successfully. |
| `npm test -- --run` in `web/` | 210 passed across 31 files. |
| `npm run build` in `web/` | Passed; existing JavaScript bundle-size warning remains. |
| `git diff --exit-code 76fcd6f7 HEAD -- web` | No UI changes from integration. |

This is 554 Rust tests and 210 web tests, not a claim that the entire Rust
workspace test suite or paid-provider end-to-end journeys were run. The optional
temporary-server HTTP smoke command was rejected twice by tool policy (the second
attempt omitted filesystem cleanup), with no more specific reason supplied.
That live HTTP check remains unverified; CLI routing, API authorization/payload
unit tests and application-level workspace execution passed independently.

Validation used the cleanup worktree's Cargo target cache. After all tests,
`cargo clean -p tetonic-app --target-dir <verified-cleanup-worktree>/engine/target`
removed 7.7 GiB of generated application artifacts. No source or stored work was
removed. The running manual-testing engine and its database were left untouched.

## Exit and next boundary

Step 1 establishes a reviewable integrated baseline with retained behavioral
coverage and explicit retirement consequences. It does not move modules,
introduce services, replace the UI, or start the ownership/dependency reorganization
in subsequent numbered steps. Those changes should build on this main branch.
