# Contributing to Tetonic

Thank you for your interest in contributing to Tetonic and its products (Lokai, Mantle). This guide explains our architecture standards, engineering gates, and development workflows.

---

## 1. Architectural Guardrails

Lokai enforces strict mechanical boundaries across all 31 engine packages. These rules are verified automatically by `lokai-arch-gate`:

| Rule | Description | Enforced By |
|---|---|---|
| **Egress Isolation** | Network HTTP calls via `reqwest` are forbidden everywhere except inside `lokai-egress`. | `ARCH-NET-001` |
| **Sandboxed Execution** | Child processes must be spawned via `lokai-sandbox` executors, never directly through `std::process::Command`. | `ARCH-PROC-001` |
| **Transactional Staging** | Workspace file modifications must flow through `lokai-transaction` to ensure atomic staging and rollback. | `ARCH-FS-001` |
| **Single Engine Kernel** | `lokai-app`, `tetonic-cli`, and `tetonicd` must not instantiate independent execution loops; all runs flow through the kernel in `lokai-core`. | `ARCH-APP-001` |
| **Clean Layer Boundaries** | Dependencies must respect the five Earth layers: Litho, Mantle, Core, Strata, and Atmos. | `lokai-arch-gate` |

---

## 2. Engineering Standards

- **Formatting**: All Rust code must be formatted using standard `cargo fmt`.
- **Clippy**: All code must compile cleanly with `-D warnings`. No new warning debt is permitted.
- **Error Handling**: Library crates use strongly typed errors via `thiserror`. Application and binary entry points (`tetonic-cli`, `tetonicd`, `lokai-arch-gate`) may use `anyhow`.
- **No Panics in Dispatch**: Production agent loops, tool handlers, and RPC paths must never use unhandled `.unwrap()` or `.expect()` calls.
- **Async Concurrency**: Never perform blocking filesystem operations inside Tokio worker threads. Never re-introduce monolithic mutex locks over persistent stores.

---

## 3. Verification Workflow

Before submitting a pull request, run the following checks from the `engine/` directory:

```bash
# 1. Run the engineering gate (formatting, clippy, quality linters, architecture invariants)
cargo run -p lokai-arch-gate -- verify package

# 2. Run unit tests for packages you modified
cargo test -p <package_name>

# 3. (Optional) Run the full test suite
cargo test --workspace
```

---

## 4. Documentation Conventions

- When adding or changing public capabilities, update the corresponding package README under `engine/<layer>/<package>/README.md`.
- When modifying JSON-RPC or cluster protocols, update the contract documents in `docs/implementation/contracts/`.
- All documentation must reflect actual production behavior, not aspirational designs.
