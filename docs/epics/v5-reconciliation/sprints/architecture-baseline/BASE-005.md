# BASE-005 — Execution, harness and inference boundaries

Date: October 8, 2026. Status: complete.
Scope: step 5 of the architecture tidy-up, based on `57f0c5d9`.

## Delivered

- Grouped registered revision preparation, authorization, admission bindings,
  harness assembly and reconstruction in `tetonic-app/resources/registered/`.
  These remain inside the existing resource authorization boundary; no service
  fields were made public to support the move. Existing public resource exports
  remain compatible.
- Created `tetonic-app/execution/` for the application run-service adapter,
  managed observation hooks, audit interface, workspace hooks and tool
  finalization effects. Moved redacted step projection into `events/agent_steps`.
  `turn_execution` and `turn_attestation` retain public compatibility reexports.
- Clarified the domain executor contract: attempt ID is correlation, not
  authority; the executor returns a candidate, not an accepted result. The
  manager still constructs the existing local executor and binds its lifetime.
- Renamed node `job_ingress` / `JobIngressManager` to `inference_ingress` /
  `InferenceIngress`, including internal fabric fields. Wire formats are unchanged.
  Expanded rejection coverage to every non-inference job kind.
- Documented retained coding strategies separately from current team-work
  coordination. Updated crate entry documentation and the current architecture
  map. Added the [execution trace and extension contract](../../../../architecture/execution-boundaries.md).
- Updated architecture checks to follow the moved implementation. Missing
  execution adapter files now fail the lifecycle ownership check. Added mutation
  tests proving that lifecycle primitives in those adapters, missing files and
  loss of sandbox selection are detected; compatibility stubs cannot satisfy
  the sandbox check.

## Runtime defect exposed by verification

The mixed-provider team journey failed intermittently with
`stale sequence: expected 16, actual 17`. A sibling's run activity advanced the
shared sequence between child-admission commands. The admission mutex serialized
admissions but could not prevent other attempts from making progress.

Manager-generated child commands now use the supervisor's current-state
transitions, fixed task/attempt identities and existing authority/lease checks,
without a run-wide optimistic version that also changes for unrelated activity.
This is limited to child AddTask/CreateAttempt/LeaseAttempt/StartAttempt commands.
Public versioned control commands, root admission, execution claims and finalization
guards are unchanged. No whole-job retry or effect replay was added.

Deterministic tests inject a parent heartbeat before each child transition and
verify that one child is admitted without claiming execution. Companion cases
inject run cancellation before each of the four transitions and require admission
to fail. The test supervisor wraps the real durable supervisor and a temporary
SQLite store; it does not replace its transition logic.

## Preserved ownership and limits

`WorkService` and `TeamWorkController` own current work coordination.
`ResourceService` owns resource authorization. `ManagedRunService` and
`DurableRunSupervisor` remain the sole attempt lifecycle/acceptance authority.
Runtime action policy, controlled egress, capability grants, scoped context and
usage accounting retain their existing paths.

Most changes relocate existing code and repair imports/source checks. There are
no web changes, schema changes, new Cargo dependencies or new execution backends.
The live engine/database were not restarted or opened. Tests use local fixtures,
temporary databases and no paid inference.

The executor trait is not a backend registry. Vendor-native harnesses, remote
agent execution, replicated control storage and Keeper remain future work.
Inference workers move model computation only. Compatibility modules preserve
old public APIs; they do not establish another authority. Historical audit paths
remain dated evidence rather than current navigation.

## Validation

| Check | Result |
|---|---|
| Full `tetonic-app` library suite | 162 passed, 4 existing cases ignored. Includes provider/tool parity, parallel teams, approvals, scope, reconstruction and process-loss recovery. |
| Affected application integration suites | 126 passed across `comp01_pins`, `gate01_pins`, `portal01_pins`, `sub02_host`, `work02_execution`, `work03_door`, `workfin01_finalization`, `workfin02_terminal`, `obs02_envelope`, `obs02_finalization`. Source pins supplement behavioral tests. |
| Managed execution and claims | 47 managed-service cases and 1 execution-claim case passed, including the two new deterministic interleaving cases. |
| Worker and executor | 52 `tetonic-node` library cases and 2 local executor cases passed. |
| Architecture gate library | 91 passed, including moved-boundary mutation tests. |
| `tetonic-arch-gate verify package` | Passed formatting, workspace/all-target Clippy with warnings denied, architecture and static quality checks. |
| Documentation/diff | Current architecture, crate entry and delivery-record local links resolve; `git diff --check` passes; no `web/` changes. |

Total: 481 passing tests in these suites. The four ignored application cases
retain their baseline meaning: three opt-in local-model/scenario fixtures and a
child-process helper exercised by its parent. This is not a full workspace test run.

The initial application pass and focused reproduction exposed the stale-sequence
race described above; the full suite passed after its fix. A moved test file was
renamed to `execution_tests.rs` to retain the repository's existing test-file
convention and classification; no quality exception or threshold was added.
Temporary diagnostic logging used for the fixture failure was removed.

## Next step

Step 6: clarify durable storage, budgets and public contracts. It is not part of
this delivery. Preserve the integrated work, registered-agent and managed-run
paths when reviewing those boundaries; do not introduce parallel state owners.
