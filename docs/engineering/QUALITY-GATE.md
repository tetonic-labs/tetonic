# Engineering quality gate

Current verification entry points, October 8, 2026. The gate is
[`tetonic-arch-gate`](../../engine/tooling/tetonic-arch-gate), not a runtime or
security proof. [Contributing](../../CONTRIBUTING.md) explains change routing;
[quality debt](QUALITY-DEBT.md) records exceptions. Earlier V3 authority audits
remain historical evidence and are not a description of today's product.

## Four complementary controls

| Control | Question |
|---|---|
| Behavior tests | Does the changed behavior, including failure handling, work? |
| Architecture checks (`ARCH-*`, `WEB-*`) | Do the specific checked boundaries remain intact? |
| Quality checks (`QG-*`) | Does the implementation meet the checked engineering standards? |
| Review | Are responsibility, permissions, user experience and recovery correctly designed? |

No one control substitutes for the others. A source pattern check is a narrow
tripwire, not proof that a whole subsystem is correct or safe.

## Verification tiers

Run these from `engine/`:

```sh
cargo run -p tetonic-arch-gate                    # architecture only
cargo run -p tetonic-arch-gate -- quality         # static quality only
cargo run -p tetonic-arch-gate -- verify fast
cargo run -p tetonic-arch-gate -- verify fast --crate tetonic-app
cargo run -p tetonic-arch-gate -- verify package
cargo run -p tetonic-arch-gate -- verify full
```

| Tier | Checks |
|---|---|
| Fast | Formatting, architecture, static quality; optional named-package all-target `cargo check` |
| Package | Formatting, workspace/all-target Clippy with warnings denied, architecture and static quality |
| Full | Package checks plus `cargo test --workspace` |

Package verification does not execute behavior tests. Run affected suites as
well. Full verification does not run ignored or live-environment tests unless
requested separately. Debug skip flags are not evidence that the omitted check
passed. The retired eval binary and daemon are not current verification commands.

## Automated quality checks

### QG-FMT-001

`cargo fmt --all -- --check` checks workspace Rust formatting.

### QG-CLIPPY-001

`cargo clippy --workspace --all-targets -- -D warnings` denies compiler and
Clippy warnings. It does not enable every optional lint or prove the absence of
runtime panics. Keep exceptions local and document their removal condition.

### QG-CHECK-001

`verify fast --crate <name>` additionally runs `cargo check -p <name> --all-targets`.
This checks the named package, not workspace warning cleanliness.

### QG-TEST-001

Full verification runs workspace tests. Passing tests establish the scenarios
and environments exercised, not correct behavior for all models or platforms.

### QG-ANYHOW-001

The static checker examines direct dependency declarations in the six engine
package groups. New library `anyhow` dependencies are rejected outside the
explicit source allowlist. Existing debt includes `tetonic-core`, `tetonic-app`,
`tetonic-artifact` and `tetonic-node`; do not copy it. Retained legacy names in
the allowlist do not make retired binaries supported. This does not inspect the
transitive dependency graph or establish that every call uses typed errors.

### QG-MODEL-001

A narrow quoted-model-identifier pattern checks core, runtime, orchestrator and
app production sources, excluding test fixtures. It is not a complete catalog
of model names. Keep model selection in configuration/request/provider data;
passing this check alone does not establish provider-neutral product logic.

### QG-DOCS-001

The repository must not have a root `sprints/` directory. The rest of the epic
layout is a contribution convention: `docs/epics/<epic>/sprints/<sprint>/`.

## Architecture checks

The authoritative list is [`ids.rs`](../../engine/tooling/tetonic-arch-gate/src/ids.rs)
and the checks called by [`run_all`](../../engine/tooling/tetonic-arch-gate/src/lib.rs).
Current checks cover selected dependency directions, direct network/process
construction, effect mutation patterns, run-journal ownership, context/compute
wiring, async-store patterns and file-size debt. Do not infer a strict Earth-layer
stack from those rules.

### ARCH-OWNER-001

[`product_boundaries.rs`](../../engine/tooling/tetonic-arch-gate/src/product_boundaries.rs)
requires current transport, application, managed execution and storage owners to
exist and be readable. Deleting or moving one requires updating the owner map,
callers and checks together. This prevents an absent scan target from silently
turning a current boundary check green.

### ARCH-OWNER-002

Current CLI transports, work/workspace/resource use cases and the team-work
controller cannot directly construct lifecycle services, issue the enumerated
attempt-lifecycle commands or construct agents. The existing registered assembly
owner may construct agents; it may not issue those lifecycle commands. Inline
`cfg(test)` and test files are excluded. Syntax patterns are not complete Rust
call-graph or macro analysis; indirect bypasses still require review.

### ARCH-FS-001

The workspace mutation check scans the current `tetonic-tools/src` tree (with
legacy path fallback). Known direct write/remove/create patterns are allowed
only in the existing workspace/mutation owners. Inline test fixtures are excluded.
This does not prove all effects are transactional, nor scan every filesystem API.

### ARCH-RUN-001

Run-journal mutation patterns belong to the execution storage owner. Grouping
storage into control/context/usage/artifacts must not enable competing journal
writers. This structural check complements transactional and competing-attempt
behavior tests.

### ARCH-SIZE-001

Rust implementation files exceeding 900 lines need the existing documented
exception process. A small file is not proof of good design. Split responsibility
where it clarifies an owner, not solely to beat a counter.

### Retained historical rules

Some checks and allowlists still target retired daemon/RPC/eval paths. A rule
that finds no files at such a path provides no evidence about the current local
API. Keep its historical intent distinct from current guarantees. New current
owners have explicit missing-path tests; broader modernization of every old rule
is not claimed. Follow [retirement](../epics/v5-reconciliation/retirement.md) and
[current ownership](../architecture/ownership.md), not old command examples.

## Frontend checks

From `web/`, `npm test` runs boundary regression tests and Vitest; `npm run build`
runs TypeScript and Vite with source-graph and bundle checks.
[`architecture.mjs`](../../web/architecture.mjs) checks required owners, literal
imports/reexports, production fixture exclusion, wire declarations, known network
primitives and pure projection dependency closures. See
[frontend boundaries](../architecture/frontend-boundaries.md) for exact scope.

A pure helper check cannot establish that a status, result, animation or approval
is truthful. Test the corresponding engine-backed behavior and inspect changed
interaction flows. Do not duplicate server authority in browser storage.

## Review obligations and exceptions

Review must still cover actionable errors, runtime panic risks, contention,
blocking work on async workers, scope and authorization, cancellation propagation,
transaction boundaries, new network stacks and misleading UI projections.
A passing `reqwest` or process-constructor pattern check cannot cover every way
to open a network connection or spawn a process. Model-specific behavior and
security guarantees require evidence from their actual execution path.

For each exception record the rule ID, location, reason, owner and expiry or
removal condition in [quality debt](QUALITY-DEBT.md). Keep source allowlists and
that record aligned. Add negative tests that demonstrate a new guard actually
catches a violation, and legitimate cases that it must allow.

## CI integration

[Engine CI](../../.github/workflows/engine-ci.yml) has a package-gate job,
a selected core/application package test job, cross-platform release builds and
a web boundary/test/build job. It does **not** run every engine workspace package's
tests. Local `verify full` has broader test scope than that selected CI test job.
Do not report a configured CI job as a completed remote run without its result.
