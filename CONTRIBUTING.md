# Contributing to Tetonic

Start with the [current architecture](docs/architecture/README.md),
[ownership map](docs/architecture/ownership.md) and
[domain terminology](docs/architecture/terminology.md). They identify the existing
owner, entry points, durable records and boundary tests for a change.

---

## 1. Architectural Guardrails

Tetonic currently has 28 engine workspace packages. Preserve these ownership
rules when extending or extracting code:

| Boundary | Contribution rule |
|---|---|
| Authority | Use verified resource/context services; configuration and caller-supplied IDs are not grants. |
| Execution | Submit through managed execution; do not add an independent supervisor, attempt owner or completion journal. |
| Effects | Preserve runtime capability checks, controlled egress, process isolation and the relevant transactional tool paths. Guarantees depend on the effect and platform. |
| State | Keep durable integrity in store transactions and expose authorized projections to the UI. |
| Dependencies | Follow behavioral ownership and actual Cargo dependencies, not an assumed strictly descending Earth-layer stack. |

`tetonic-arch-gate` checks specific static invariants with explicit exceptions;
it does not mechanically prove every rule above. Some checks still refer to
retired paths. Select behavioral tests from the ownership map as well as running
the gate. Do not replace such tests with assertions that only pin filenames.

---

## 2. Engineering Standards

- **Formatting**: All Rust code must be formatted using standard `cargo fmt`.
- **Clippy**: All code must compile cleanly with `-D warnings`. No new warning debt is permitted.
- **Error Handling**: Prefer strongly typed library errors via `thiserror`. Binary entry points such as `tetonic-cli` and `tetonic-arch-gate` may use `anyhow`.
- **No Panics in Dispatch**: Production agent loops, tool handlers, and RPC paths must never use unhandled `.unwrap()` or `.expect()` calls.
- **Async Concurrency**: Never perform blocking filesystem operations inside Tokio worker threads. Never re-introduce monolithic mutex locks over persistent stores.

---

## 3. Verification Workflow

Before submitting a pull request, run the following checks from the `engine/` directory:

```bash
# 1. Run the engineering gate (formatting, clippy, quality linters, architecture invariants)
cargo run -p tetonic-arch-gate -- verify package

# 2. Run unit tests for packages you modified
cargo test -p <package_name>

# 3. (Optional) Run the full test suite
cargo test --workspace
```

---

## 4. Documentation Conventions

- When changing responsibilities, durable records or public entry points, update the [ownership map](docs/architecture/ownership.md) and the relevant package documentation together.
- When modifying local HTTP or cluster protocols, update the corresponding contracts in `docs/implementation/contracts/`.
- All documentation must reflect actual production behavior, not aspirational designs.
- Keep sprint tickets under `docs/epics/<epic>/sprints/<sprint>/`; do not create a top-level `sprints/` folder.
