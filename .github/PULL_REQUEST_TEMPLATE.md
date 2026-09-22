## Description

Brief summary of the changes introduced in this pull request and the problem they solve.

## Related Issues

Fixes #(issue number)

## Architecture and Quality Checklist

Before submitting this pull request, verify the following standards:

- [ ] Architecture invariants maintained:
  - [ ] Egress isolation: HTTP network calls via `reqwest` are restricted to `lokai-egress` (`ARCH-NET-001`).
  - [ ] Sandboxed execution: child processes use `lokai-sandbox` executors (`ARCH-PROC-001`).
  - [ ] Transactional staging: file writes use `lokai-transaction` (`ARCH-FS-001`).
  - [ ] Single engine kernel: runs execute through `lokai-core` (`ARCH-APP-001`).
- [ ] Engineering verification passes locally from `engine/`:
  - `cargo run -p tetonic-arch-gate -- verify package`
  - `cargo test -p <modified_package>`
- [ ] No Clippy warnings introduced (`-D warnings`).
- [ ] No unhandled panics (`.unwrap()` or `.expect()`) in production dispatch paths.
- [ ] Documentation updated under `docs/` or package `README.md` if public APIs or contracts changed.

## Testing Strategy

Explain how you tested these changes (unit tests, integration tests, or manual reproduction steps).
