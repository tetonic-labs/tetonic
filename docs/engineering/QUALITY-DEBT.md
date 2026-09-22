# Quality / architecture exemption debt

Visible engineering debt. Not a license to add silent ignore files.

Every row: Rule ID, location, reason, owner, expiry / deletion condition.

---

## QG-ANYHOW-001 — grandfathered direct `anyhow`

CONTRIBUTING forbids `anyhow` in libraries. These crates **already** declare it. The gate blocks **new** library crates from adding it.

| Package | Reason | Owner | Removal |
|---------|--------|-------|---------|
| `lokai-core` | Direct `anyhow` in Cargo.toml despite CONTRIBUTING | Library error cleanup | Convert to `thiserror`; delete this row |
| `lokai-app` | Application kernel uses anyhow for orchestration | binary-adjacent (M8 D-10, 2026-08-26) | Keep; not an M8 COMPLETE bar. Convert only if a later package reclassifies |
| `lokai-artifact` | Library anyhow | error-type cleanup | Convert to `thiserror` |
| `lokai-node` | Worker binary-adjacent crate | May join the bin allowlist if classified as a bin | Decide with node packaging |

**Allowed by policy (not debt):** `lokai-cli`, `lokaid`, `lokai-eval`, `lokai-bench`.

---

## ARCH-SIZE-001 — file-size allowlist

Implemented in `engine/bins/lokai-arch-gate/src/lib.rs` `FILE_SIZE_ALLOWLIST`. Each entry must keep owner + removal note in that array. Do not grow the list without a new row here.

Current members (see source for owner comments):

- `crates/lokai-memory/src/lib.rs`
- `crates/lokai-inference/src/lib.rs`
- `bins/lokai-cli/src/main.rs`
- `crates/lokai-core/src/agent.rs`
- `crates/lokai-inference/src/pooled.rs`
- `crates/lokai-rpc/src/protocol.rs`
- `crates/lokai-tools/src/process_executor.rs`
- `crates/lokai-inference/src/placement_engine.rs`
- `crates/lokai-node/src/fabric_chat.rs`
- `crates/lokai-fabric-client/src/legacy.rs`
- `bins/lokai-arch-gate/src/lib.rs`
- `crates/lokai-app/src/turn_execution.rs` — rustfmt expansion during M0 VERIFY; extract event/redact helpers
- `crates/lokai-context/src/tests.rs` — `src/tests.rs` not skipped by `_tests.rs`; move to `tests/`

---

## ARCH-PROC-001 — subprocess allowlist

`is_allowed_subprocess` in `lokai-arch-gate`. Allowlists `process_executor.rs` for Constrained / `shell_command` `Command::new` residual (M2 leftover). **DEL-001** (unsandboxed verify execute) is DELETED. The gate prevents *new* spawn sites; it does not prove INV-PROC-001 ESTABLISHED.

---

## ARCH-POL-001 — `check_tool` allowlist (removed M5 CONVERGE 2026-08-26)

M0 freeze allowlisted `check_tool` in `sinks.rs`, `engine.rs`, and one `execute_gated` call (DEL-004). M5 deleted the dual API; CONVERGE emptied `CHECK_TOOL_ALLOW`. Any production `\bcheck_tool\b` in `.rs` is now a violation.

Does **not** prove INV-AUTH-001 ESTABLISHED (spawn Secret residual; enrollment SQL default).

---

## ARCH-OUT-002 — fail-open redact allowlist (removed M4 CONVERGE 2026-08-26)

M0 freeze allowlisted `Err(_) => (text.to_string(), false)` in `crates/lokai-secrets/src/lib.rs` (DEL-019). M4 inverted the arm; CONVERGE removed the exception. Any copied plaintext `Err(_)` / `unwrap_or((text.to_string(), false))` / `unwrap_or_else(|_| text.to_string())` in production `.rs` is now a violation. `fn redact_text_sync` may still be defined only in that file.

Does **not** prove INV-OUT-001 ESTABLISHED (spanning-token secrets remain).

---

## ARCH-READ-001 — prefetch allowlist (removed M3 CONVERGE 2026-08-26)

M0 freeze allowlisted one `tokio::fs::read_to_string` in `crates/lokai-core/src/agent.rs` (DEL-005). M3 deleted the prefetch; CONVERGE removed the `CORE_AGENT` exception. Any raw `tokio::fs` / `std::fs::read` in `lokai-core` is now a violation.

---

## RV-ERR-002 — production unwrap

Not Clippy-enforced. Enabling workspace `clippy::unwrap_used` is a cleanup epic. Review + V3 VERIFY until then.

---

## Bootstrap — not grandfathered

M0 VERIFY `verify package` is green without `--skip-fmt` / `--skip-clippy` (run 13). M0 CONVERGE `verify full` is green (run 14: fmt + clippy + architecture + quality-static + `cargo test --workspace`). M0 REBASELINE introduced no new lint suppressions. New Clippy findings after this point are **MUST FIX**, not silent allowlists, except localized `#[allow]` rows below.

M0 VERIFY clippy hydra also added localized allows (not crate-level `unwrap_used`) in: `lokai-fabric-client`, `lokai-broker`, `lokai-orchestrator`, `lokai-node`, `lokai-app`, `lokaid`, `lokai-cli`. Owners remain those crates’ migration packages.

---

## Clippy `#[allow]`

Existing localized allows (not a crate-level `unwrap_used`):

| Location | Lint | Reason | Owner | Removal |
|----------|------|--------|-------|---------|
| `lokai-memory` `record_tool_call`, `record_egress`, `append_message_with`, `upsert_compute_reservation_row`, `record_result_disposition`, `upsert_scheduler_decision_row` | `too_many_arguments` | Store row APIs | store cleanup | Split params structs |
| `lokai-index` `index_one_file` | `too_many_arguments` | Indexer row insert | index cleanup | Same |
| `lokai-fabric-protocol` `legacy_infer_profile` / `typed_infer_profile` | `too_many_arguments` | Compatibility constructors; callers stay positional | fabric capability reshape | Params struct without changing advertisement semantics |
| `lokai-telemetry` `record_compute_stage` | `too_many_arguments` | Safe span field list | telemetry cleanup | Params struct |
| `lokai-inference` `FabricNodeProvider::chat_on_fabric`, `evaluate_typed_job_placement` | `too_many_arguments` | Trait + placement signature | inference cleanup | Params struct without changing dispatch |
| `lokai-orchestrator` `run_orchestrated_turn` | `too_many_arguments` | WORK-02 added `root_execute: impl RootExecute` (8th arg). PLAN requires the hook; leftover critic/revision/spawn stay `.turn`. | WORK-02 / CODE-03 | CODE-03 leftover-turn deletion or params struct |
| `lokai-app` `DefaultSessionService::new` | `too_many_arguments` | WORK-02 S2 added `runs: Arc<dyn RunService>` so cancel can `drop_active_for_run`. | WORK-02 / CODE-02 | Session-chat leftover split or params struct |
| `lokai-run` `persist` / sync dedup helpers | `dead_code` | Async twins are the production callers | run wiring | Call or delete sync twins |
| `lokai-tools` workspace revert/nofollow/apply_edit | `dead_code` | Helpers used by tests; revert path unwired | mutation owner | Wire or `cfg(test)` |
| `lokai-tools` `process_executor` tests | `items_after_test_module` | Tests sit above `run_py_compile` | tools cleanup | Move tests to file end |
| `lokai-context` `src/tests.rs` inner `mod tests` | `module_inception`, `field_reassign_with_default` | Nested test module + MockProvider setup | M0 VERIFY; SIZE debt already wants `tests/` | Move module to `tests/` and struct-update mocks |

New allows need a row here.
