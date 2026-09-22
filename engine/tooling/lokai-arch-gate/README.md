# lokai-arch-gate

Architecture invariant checker (`ARCH-*`) and engineering quality gate (`QG-*`). P0 failure-injection suite (AC2-9). Not a runtime.

Quality lives in this binary (modules `quality`, `verify`) rather than a second crate: one process, distinct IDs and subcommands.

## Role in the stack

`verify package` is the Linux CI engineering gate (`engine-ci.yml`). Architecture-only `cargo run -p lokai-arch-gate` remains for local ARCH debugging.

Policy: `docs/engineering/QUALITY-GATE.md`. Debt: `docs/engineering/QUALITY-DEBT.md`.

## Architecture checks (`ARCH-*`)

| ID | Short name | What it enforces |
|----|------------|------------------|
| ARCH-PROC-001 | subprocess_spawn | No `Command::new` outside ProcessExecutor allowlist (`lokaid` `--supervise` re-exec is listed) |
| ARCH-PROC-002 | git_via_process_broker | No `Command::new("git")` in lokai-tools; worktree uses `ProcessExecutor::run_git` (R09) |
| ARCH-PROC-003 | lsp_via_process_broker | No raw LSP `Command::new` allowlist escape; production uses `SandboxLspLauncher` (R29) |
| ARCH-PROC-004 | production_tools_sandboxed | Production tools/worktree/runtime use sandbox wiring |
| ARCH-PROC-005 | no_old_verify_flag | No verify dual-path env flag in engine sources (M0 freeze) |
| ARCH-POL-001 | check_tool_new_sites | No production `check_tool` sites (allowlist emptied M5 CONVERGE) |
| ARCH-OUT-002 | fail_open_redact_new | No copied fail-open `redact_text_sync` `Err(_)` / `unwrap_or` plaintext arms (`lib.rs` not exempt) |
| ARCH-READ-001 | core_raw_read_new | No raw FS reads in `lokai-core` (`tokio::fs` / `std::fs::read`) |
| ARCH-EGRESS-001 | egress_hygiene_new | No empty worker Infer/capacity `EgressGuard::new()`; no `LOKAI_FABRIC_LEGACY_CHAT_ONLY` production read |
| ARCH-APP-014 | app_door_new | No production `lokaid` `turn_execution::execute_turn`; no production token-prefix eprintln |
| ARCH-APP-015 | inspect_door_new | No `.supervisor.snapshot` / `.supervisor.resume_from_sequence` in `lokaid` `handlers/run.rs` or `lokai-eval` `recovery.rs` |
| ARCH-PROD-001 | product_boundary_new | No `lokai-index` / `lokai-lsp` in `lokai-core` / `lokai-runtime` src or Cargo.toml; no `SpecialistRole` orchestrator identity |
| ARCH-NET-001 | reqwest_boundary | No `reqwest` outside `lokai-egress` |
| ARCH-RT-001 | engine_runtime | Production bins must not construct `Agent` directly |
| ARCH-DEP-001 | dependency_direction | `lokai-policy` must not depend on `lokai-inference` |
| ARCH-RPC-001 | schema_methods | Every `schema_bundle().methods` entry is handled in `lokaid` dispatch |
| ARCH-SIZE-001 | file_size | Rust implementation files stay under 900 lines; allowlist is documented debt |
| ARCH-INFER-001 | unguarded_remote_dispatch | Production remote providers are constructed only by authorized compute wiring |
| ARCH-APP-001 | lokaid_session_authority | `lokaid` must not own `SessionState` / conversation HashMap |
| ARCH-APP-007 | no_gates_ok_turn_abort | CLI/daemon turn-admission files must not branch on `gates_ok` |
| ARCH-APP-008 | no_duplicate_resume_cap | `RESUME_MESSAGE_CAP` must live only in `lokai-app::resume` |
| ARCH-APP-009 | no_duplicate_enrollment_helpers | enrollment helper defs only in `lokai-app` (R26) |
| ARCH-OUT-001 | outbound_secret_scanner | `lokai-secrets` must not depend on `lokai-context`; scanner attached at broker |
| ARCH-ASYNC-001 | async_sync_calls | No `.read_sync` / `.write_sync` inside an `async fn` body |
| ARCH-ASYNC-002 | no_mutex_store | No `Mutex<Store>` reintroduction |

Additional APP/DEP/FS/RUN/CTX/FAB IDs: see `src/ids.rs` and `docs/engineering/QUALITY-GATE.md`.

Quality static (`QG-ANYHOW-001`, `QG-MODEL-001`, `QG-DOCS-001`) is `cargo run -p lokai-arch-gate -- quality`. Formatting and Clippy are `verify` tiers, not this table.

## P0 regression tests (`p0.rs`)

- Production vs `TestRuntime` assembly split
- Resume newest-N after `session/end`
- Stale worker result rejection
- Mutating capability deny leaves FS unchanged
- Subprocess adversarial shell metachar rejected

## Commands

```bash
cd engine
cargo run -p lokai-arch-gate                    # architecture static (default)
cargo run -p lokai-arch-gate -- quality         # QG-* static only
cargo run -p lokai-arch-gate -- verify fast     # fmt + arch + quality [+ check -p]
cargo run -p lokai-arch-gate -- verify package  # + clippy -D warnings (CI)
cargo run -p lokai-arch-gate -- verify full     # + cargo test --workspace
cargo test -p lokai-arch-gate                   # fixtures + P0 suite
```

Policy: `docs/engineering/QUALITY-GATE.md`. Debt: `docs/engineering/QUALITY-DEBT.md`.

Findings print a stable `ARCH-*` or `QG-*` id, path, why, and how to fix.

## Product plan

| ID | Feature | Status |
|----|---------|--------|
| AC2-9 | Architecture CI + P0 failure injection | **Done** |
