# Managed runtime and server gate cleanup

Date: October 5, 2026. Continues the [fleet and integration-contract slice](fleet-and-contract-evidence-2026-10-05.md).

## Implemented

`ManagedAdmission::Admitted` and `ManagedSubmission::Started` now carry their
large `ManagedBinding` in an owned `Box`. Admission still uses the existing
supervisor, registry, authorization, leases, and dispatch owner. The completion
receiver is unchanged. The convenience admission API still returns an owned
`ManagedBinding` by moving it out of the box. No shared ownership, new manager,
or new execution path was introduced.

The two binding construction sites and the test fixture extracting an owned
binding were updated. Existing submission code propagates/clones the boxed
metadata just as it propagated/cloned the inline metadata. Cancellation error
reporting drops a redundant reference while retaining the same redaction helper.
This is a Rust source-level change to those enum payloads; no HTTP shape, database
schema, serialized run record, admission policy, or cancellation ordering changes.
Commit: `5ba5549`.

The server's intention validation uses simpler equivalent conditions, and its
event acknowledgement callback has a named type. The unused `ContextBudget::assemble`
wrapper was removed; its tests call the production `assemble_with_state` method
with the same null working state the wrapper supplied. Context limits, retention
order, mandatory observations, and overflow rejection are unchanged.

## Verification

- `cargo test -j 2 -p tetonic-run --lib --tests --no-fail-fast`: **106 passed, zero
  failed, zero ignored**. This includes all 36 managed-service regressions:
  concurrent admission/replay, scoped delegation, cancellation during admission,
  authority revocation, worker/process termination, cancellation/finalization
  races, capacity retention until quiescence, and deadline handling.
- `cargo clippy -j 2 -p tetonic-run -p tetonic-server --all-targets -- -D warnings`:
  **passed**.
- `cargo test -j 2 -p tetonic-app -p tetonic-server --lib --tests --no-fail-fast`:
  **612 passed, zero failed, two ignored** (599 application, 13 server). The two
  existing ignored tests require a local model and fresh proof directories. This
  verifies application consumers of the boxed binding, including real managed
  team dispatch through fixture providers, privacy, budget settlement, approval,
  cancellation, and the server's perception and context-budget behavior.
- Combined runtime/application/server coverage: **718 passed, zero failed, two
  existing ignored live-proof tests**.
- `cargo run -p tetonic-arch-gate -- verify package`: formatting, architecture and
  static quality pass. The overall command **still fails Clippy**, now reporting
  19 findings in the application layer. No gate or allowlist was relaxed.
- `git diff --check`: **passed**.

Local ignored logs: `.lokai/managed-gate-tests.txt`,
`.lokai/managed-server-clippy.txt`, `.lokai/managed-gate-consumer-tests.txt`, and
`.lokai/managed-gate-package-1.txt`.

After verification completed, disk space was below 1 GB. Cargo's package-scoped
cleanup (`cargo clean --package tetonic-app` against this engine's target directory)
removed 35.7 GiB of generated application artifacts. Sources and verification
logs are retained; the next application build will rebuild those artifacts.

## Next gate work

The next cohesive slice is the application resource API, adapting its existing
methods and callers to named request inputs while preserving authorization and
storage operations. It should build on the already migrated storage requests;
it must not accept caller-selected authenticated principals or replace the
existing resource service.

| Area | Remaining positional APIs |
|---|---|
| Team work | `create_team_work_item`, `create_team_work_item_with_input`, `create_team_work_item_for_purpose`, `activate_from_cursor`, `create_work_delegation` |
| Briefs | `save_work_brief` |
| Stop control | `apply_control_stop` |
| Scoped publication | `publish_message` |
| Human controls and accounting | `propose_effect_approval`, `resolve_effect_approval`, `record_team_effort` |
| Workstation placement | `enroll_workstation`, `claim_worker_assignment` |
| Execution grants | `bind_delegated_execution_grant` |

These 14 findings need careful argument mapping and preservation of default
values, idempotency keys, scope checks, proposal digests, generation fences, and
delegation-parent validation. Keep employee credentials and device-secret
authority distinct: worker assignment claims currently authenticate the device,
not an employee team-management credential. Do not pass a caller-provided actor
through the borrowed storage request types.

The other five findings are a needless provider-URL borrow and a simplifiable
agent-key check in `local_workspace/agents.rs`, a complex inference-binding return
type in `product_submit.rs`, a redundant result match in `resources/work_usage.rs`,
and a nested condition in `resources/team_work_activation.rs`.

This inventory is what the current gate reached, not a guarantee that later
targets have no further findings. This slice does not close the sprint or the
release baseline, and the running UI/server was not restarted.
