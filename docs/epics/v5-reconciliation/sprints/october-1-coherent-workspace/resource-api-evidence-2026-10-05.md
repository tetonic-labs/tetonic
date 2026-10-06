# Application resource API and package gate

Date: October 5, 2026. Continues the [managed runtime and server cleanup](managed-gate-evidence-2026-10-05.md).
Source baseline: `e5736fb` on `main`; verification below covers the modified tree
for this slice, including the new resource request module. No live model or
browser scenario is claimed by these checks.

## Implemented

The 14 application methods reported by the preceding package gate now use 12
named input types exported from `tetonic_app::resources`. The three work-creation
entry points share `CreateTeamWorkItem`; the existing default-input and default
work-purpose forwarding behavior is retained. The remaining inputs cover cursor
activation, delegation, brief updates, hierarchical stop, scoped publication,
approval proposal/resolution, measured effort, workstation enrollment/claims and
delegated execution grants.

All 37 affected callers in the existing CLI, workspace, activation service and
tests were migrated. This extends the earlier named storage inputs through the
existing application boundary. It does not add a service, database, execution
loop, permission authority or ledger. The service bodies retain their storage
operations, scope checks, idempotency keys, version checks, generation fences and
proposal binding.

Employee credentials stay outside the new inputs. Verified principals are still
derived by the service and supplied to storage internally. Worker claims retain
device-secret authentication; delegated execution retains its separate live
`DelegationParent` handle. Request fields cannot replace either authority.

This is a Rust source API change: external Rust callers of these methods must
construct the corresponding named input. CLI flags, HTTP payloads, durable schema
and serialized records are unchanged. The application README documents the seam.

Five additional production lint findings were resolved with equivalent expressions
and a private inference-snapshot type alias. Test cleanup relocates four intact
test modules below production items, uses boolean/containment assertions, and
ends three test mutex guards in explicit lexical scopes before awaited operations.
No test was removed, ignored or weakened, and no lint allowance or gate exemption
was added.

## Verification

- `cargo check -j 2 --workspace --all-targets`: passed after the API migration.
- Independent source comparison: all **37** migrated argument-to-field mappings
  match the old calls, and all **14** migrated method bodies match after unpacking
  the named inputs and expanding the two forwarding calls. The four relocated
  test modules are unchanged.
- `cargo run -p tetonic-arch-gate -- verify package` with `CARGO_BUILD_JOBS=2`:
  **passed**. This includes workspace-wide Clippy for all targets with
  `-D warnings`, formatting, architecture and static quality checks. Earlier
  application production and test lint findings are cleared.
- `cargo test -j 2 -p tetonic-app -p tetonic-cli --lib --tests --no-fail-fast`:
  **723 passed, zero failed, four existing ignored tests** across 32 suites.
  Application coverage is 599 passing tests; CLI coverage is 124, including all
  six executable control-CLI lifecycle tests. The ignored tests are two local-model
  proof helpers and two manual terminal frame exporters.
  Passing cases include real managed two-agent dispatch with fixture inference,
  live parent revocation, shared usage, human handoff/stop, private-history
  isolation and explicit publication, approvals, and workstation generation fences.
- `git diff --check`: passed.

Local ignored logs: `.lokai/resource-api-check.txt`,
`.lokai/resource-api-package-final.txt`, `.lokai/resource-api-tests.txt`, and
`.lokai/resource-api-mapping-audit.txt`. The argument mapping and module relocation
audits are local review aids, not replacement regression tests.

## Remaining release work

The package engineering gate is green; this does not close OCT-101 or Sprint 1.
This run does not claim a fresh full-workspace test run, installer validation,
engine-restart recovery, fresh-user usability results or new actual-model team
evidence. The running UI/server was not restarted.

The next product proof remains useful team work over supplied documents through
the supported local path, with inspectable source evidence and a combined result.
The sprint also retains its restart reconciliation and release-profile evidence
requirements. Reuse the current workspace, registered agents, managed children,
shared accounting and human controls for that proof.
