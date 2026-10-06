# Storage request API cleanup — October 5, 2026

Follow-up to the [engineering gate cleanup](engineering-gate-evidence-2026-10-05.md). This completes the previously identified storage signature refactor. It does not close the overall engineering gate or implement another product capability.

## Change and integration

Twenty store methods now take named inputs defined in `engine/strata/tetonic-memory/src/control_requests.rs`. Seventeen request types cover publication, delegated admission, approvals, effort, work creation/delegation, briefs, inference accounting, plans, human handoff, and workstation claims. Work-creation and inference variants share their inputs instead of duplicating types. The three convenience wrappers retain their previous defaults.

All 111 existing callers in storage tests, application services, the local workspace, and the managed runtime were updated. No second persistence or execution path was introduced. HTTP payloads, CLI commands, schema migrations, and stored records are unchanged. The Rust store method signatures change; downstream Rust callers must adopt the named inputs.

The request objects are untrusted inputs, not permission grants. Store-side authorization, transaction boundaries, idempotency checks, version checks, lease checks, allocation rules, and unknown-usage handling remain authoritative. Device secrets are still passed directly to the existing verification path; the new request types do not derive serialization or debug output.

Effect approval handling now lives in `human_controls/approvals.rs`, beside the stop/effort control implementation. This gives approval proposal, resolution, lookup, and dispatch validation a focused home without expanding the size allowlist. The inference tier match and checkpoint sort were simplified with equivalent behavior.

## Review evidence

- All 111 rewritten call sites were compared with their previous positional arguments: their argument-to-field mappings match.
- The 17 substantive storage operation bodies were compared with the previous commit after removing input destructuring and whitespace: their logic matches. The three forwarding wrappers were reviewed separately and retain their defaults.
- `cargo check --workspace --all-targets` passed. It reports an existing unused `ContextBudget::assemble` method in the server binary.
- No lint suppressions, gate changes, or allowlist additions were introduced.

## Test results

- `cargo clippy -j 2 -p tetonic-memory -p tetonic-inference --all-targets -- -D warnings`: **passed**.
- `cargo test -j 2 -p tetonic-memory -p tetonic-app -p tetonic-run -p tetonic-inference -p tetonic-core -p tetonic-arch-gate --lib --tests --no-fail-fast`: **1,140 passed, 19 failed, 4 ignored**. The overall command fails; no test was disabled or rewritten to hide a failure.
- All six library suites passed: application 206, architecture gate 89, core 47, inference 139, memory 168, managed runtime 10 (**659 total**). The 36 managed-service integration tests also passed, including cancellation, owned process termination, delegated authority, shared execution ownership, and deadline/finalization checks. The complete managed-runtime crate suite passes.
- `cargo fmt --all -- --check` and `git diff --check`: **passed**.

The broad run continued after failures instead of stopping before storage and managed-runtime regressions. Its 19 failures are all in application integration targets:

| Failure category | Count | Evidence / remaining work |
|---|---:|---|
| Removed daemon source paths | 14 | `code03_pins`, `comp01_pins`, `gate01_pins`, `portal01_pins`, and `work03_door` still read files under the absent `litho/tetonicd` tree. Replace old source pins with checks against the current supported composition; do not restore retired systems just to satisfy the pins. |
| Old source-pattern assertions | 2 | `code03_llm_route_stamps_parent_fabric` expects `FabricCallMeta`; `comp01_cli_no_supervisor_clone` expects `self.install_compute_plane(`. Reconcile the intended invariant with its current owner. |
| Competing-attempt fault fixtures | 2 | `fin03_competing_finalizers_fail_closed` and `v4_audit_lease_loss_during_verify_e2e` fail while creating a second attempt with `max simultaneous attempts reached`. Review fixture setup against current admission rules; do not relax production limits to make the fixtures pass. |
| Runtime dependency boundary | 1 | `cap01_runtime_crate_clean_of_repository_heuristics` rejects the existing direct `tetonic-secrets` dependency. Both the dependency declaration and failing test were verified unchanged from the previous commit. The runtime currently uses the dependency for redaction. Preserve redaction while reconciling this boundary. |

These findings remain unresolved. This run does not establish that every broad integration failure has been reproduced on an untouched checkout, and it does not establish a green release baseline.

## Remaining gate findings

The unmodified package gate now passes the storage/inference checks that blocked the previous slice, plus formatting, architecture, and quality-static checks. It still fails Clippy on later crates:

| Area | Current finding | Next treatment |
|---|---|---|
| Fleet supervisor | Two registry read guards span an asynchronous channel send in `inject_steering` | Release registry locks before waiting on receivers; add a backpressure regression that proves registry operations remain available |
| Managed runtime | Two large enum variants; one needless reference | Review binding ownership when introducing indirection; preserve existing cancellation/delivery regressions |
| Runtime test helpers | Complex callback type; discarded future handle in the soak test | Name the callback contract and make handle disposal explicit |
| Server | Two boolean expressions and two event acknowledgement callback types | Simplify equivalent validation and name the callback contract |

Clippy stops failing crates early; these are the currently observed findings, not a complete inventory of all downstream debt. The fleet lock issue is a behavioral risk and should lead the next reliability slice. The running UI/server process was not restarted.
