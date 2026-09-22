# Engineering quality gate

Purpose: make repository degradation **difficult** under agent-heavy V3 migration, without claiming properties the tooling cannot prove.

Human contract: [`CONTRIBUTING.md`](../../CONTRIBUTING.md).  
Exemptions / grandfathering: [`QUALITY-DEBT.md`](./QUALITY-DEBT.md).  
Architecture invariants (owner): `engine/bins/lokai-arch-gate`.

---

## Four controls (keep them separate)

| Control | Question it answers |
|---------|---------------------|
| **Tests** | Does behavior still work? |
| **Architecture gate** (`ARCH-*`) | Does the system still obey structural boundaries? |
| **Quality gate** (`QG-*`) | Does the implementation meet the engineering standard? |
| **Review** (`RV-*`) | What static tooling cannot honestly prove? |

The binary `lokai-arch-gate` hosts both architecture and quality checks so there is **one entry point**. IDs and CLI subcommands keep the concerns distinct. A second crate was not added.

```text
cargo run -p lokai-arch-gate                 # architecture only (compat)
cargo run -p lokai-arch-gate -- quality      # QG-* static only
cargo run -p lokai-arch-gate -- verify fast
cargo run -p lokai-arch-gate -- verify package
cargo run -p lokai-arch-gate -- verify full
```

`verify` always runs architecture + quality static checks. Tiers add rustfmt / Clippy / workspace tests.

---

## Rule taxonomy

- **HARD AUTOMATED** — a tool proves a specific property. Failure blocks VERIFY / CI.
- **STRUCTURAL / ARCHITECTURAL** — owned by `ARCH-*`. Quality `verify` invokes them; it does not reimplement them.
- **REVIEW-ENFORCED** — important, not mechanically honest as a simple static rule.

A rule is called automated **only** if the implementation actually proves it.

---

## CONTRIBUTING.md classification

| Rule | Class |
|------|--------|
| HTTP only via `lokai-egress` / tools must not open the network | ARCHITECTURE_GATE (`ARCH-NET-001`) + REVIEW_ONLY (`RV-NET-002` other stacks) |
| Process spawn allowlist | ARCHITECTURE_GATE (`ARCH-PROC-*`) |
| Transactional file writes | ARCHITECTURE_GATE (`ARCH-FS-001` partial) + REVIEW_ONLY (`RV-FS-002`) |
| No `Agent::new` in production bins | ARCHITECTURE_GATE (`ARCH-RT-001`) |
| App/CLI/daemon not a second kernel | ARCHITECTURE_GATE (`ARCH-APP-*`) |
| File ≤ 900 lines | ARCHITECTURE_GATE (`ARCH-SIZE-001`) — maintainability guardrail, not architecture proof |
| `cargo fmt` | QUALITY_GATE (`QG-FMT-001`) |
| Clippy `-D warnings` | QUALITY_GATE (`QG-CLIPPY-001`) |
| Library `thiserror` / no new direct `anyhow` | QUALITY_GATE (`QG-ANYHOW-001`) |
| No production unwrap/expect | REVIEW_ONLY (`RV-ERR-002`) |
| Actionable errors | REVIEW_ONLY (`RV-ERR-001`) |
| No sync store I/O in `async fn` | ARCHITECTURE_GATE (`ARCH-ASYNC-001`) |
| No `Mutex<Store>` | ARCHITECTURE_GATE (`ARCH-ASYNC-002`) |
| Hot-path `Arc<Mutex<T>>` | REVIEW_ONLY (`RV-CONC-001`) |
| Heavy CPU off Tokio workers | REVIEW_ONLY (`RV-ASYNC-001`) |
| FAST / PACKAGE / FULL commands | TEST + QUALITY_GATE + ARCHITECTURE_GATE (tiers) |
| No root `sprints/` | QUALITY_GATE (`QG-DOCS-001`) |
| Epic folder layout | PROCESS_DOCUMENTATION + `QG-DOCS-001` |
| Crate README / sprint docs | PROCESS_DOCUMENTATION |
| Hardcoded model ids in routing crates | QUALITY_GATE (`QG-MODEL-001` narrow) |
| V3 one-authority / no `check_tool` | `ARCH-POL-001` (production `check_tool` = 0 after M5 CONVERGE) |
| Eval subsets | TEST (`lokai-eval` CI job, not `verify full`) |

---

## Automated quality rules (`QG-*`)

### QG-FMT-001

- **Description:** Rust sources match rustfmt.
- **Category:** HARD AUTOMATED
- **Severity:** block
- **Mechanism:** `cargo fmt --all -- --check` (no repo `rustfmt.toml`; rustfmt defaults).
- **Scope:** `engine/` workspace
- **Exemptions:** none
- **Proves:** formatting convention, not design quality.

### QG-CLIPPY-001

- **Description:** Clippy is warning-clean (`-D warnings`).
- **Category:** HARD AUTOMATED
- **Severity:** block (PACKAGE / FULL / CI)
- **Mechanism:** `cargo clippy --workspace --all-targets -- -D warnings`
- **Scope:** engine workspace, all targets
- **Exemptions:** localized `#[allow(clippy::…)]` only, with a [`QUALITY-DEBT.md`](./QUALITY-DEBT.md) row. No crate-level `unwrap_used` allow.
- **Proves:** the current Clippy + rustc warning set is clean. Does **not** enable pedantic lints. Does **not** enable `clippy::unwrap_used` (see RV-ERR-002).

This is also the **compiler-warning** gate. Workspace-wide `RUSTFLAGS=-D warnings` is not a separate command; Clippy `-D warnings` is the existing CI policy and already covers generated/test targets that CI builds.

### QG-CHECK-001

- **Description:** An affected crate compiles.
- **Category:** HARD AUTOMATED
- **Severity:** block when requested
- **Mechanism:** `cargo check -p <crate> --all-targets` on `verify fast --crate <name>`
- **Scope:** named package only
- **Exemptions:** none
- **Proves:** that crate type-checks. Does not prove the workspace is warning-clean.

### QG-TEST-001

- **Description:** Workspace tests pass.
- **Category:** HARD AUTOMATED (FULL only)
- **Severity:** block CONVERGE
- **Mechanism:** `cargo test --workspace`
- **Scope:** engine workspace
- **Exemptions:** none
- **Proves:** unit/integration tests in the workspace. Does **not** run `lokai-eval` subsets (those stay the `eval-gate` CI job).

### QG-ANYHOW-001

- **Description:** No **new** library crate may take a **direct** `anyhow` dependency.
- **Category:** HARD AUTOMATED
- **Severity:** block
- **Mechanism:** parse `engine/crates/*/Cargo.toml` and `engine/bins/*/Cargo.toml` `[dependencies]` (not the transitive tree; not `[dev-dependencies]`).
- **Scope:** direct dependency declarations
- **Exemptions:** bins `lokai-cli`, `lokaid`, `lokai-eval`, `lokai-bench` (policy). Libraries `lokai-core`, `lokai-app`, `lokai-artifact`, `lokai-node` (grandfather — QUALITY-DEBT.md).
- **Proves:** no additional library Cargo.toml adds `anyhow`. Does not prove call sites use `thiserror`. Does not forbid transitive `anyhow`.

### QG-MODEL-001

- **Description:** Production sources in routing/runtime crates must not embed model product identifiers.
- **Category:** HARD AUTOMATED (narrow)
- **Severity:** block
- **Mechanism:** regex for quoted literals (`llama3` / `gpt-4` / `claude-N` / `mistral` / `qwenN` / `qwen:`) in `lokai-core`, `lokai-runtime`, `lokai-orchestrator`, `lokai-app` production text. `#[cfg(test)]` items are stripped. `tests/`, `*_tests.rs`, benches skipped.
- **Scope:** those four crates only
- **Exemptions:** tests, capacity recipes, eval corpus, inference/capacity catalogs (not scanned).
- **Proves:** those crates’ production files do not contain the listed **quoted** product strings. Does **not** prove model-agnostic routing globally. Does not scan comments-only or unquoted identifiers. Does not treat “Ollama” as a model id.

### QG-DOCS-001

- **Description:** No repository-root `sprints/` directory.
- **Category:** HARD AUTOMATED
- **Severity:** block
- **Mechanism:** directory existence at repo root
- **Scope:** repo root
- **Exemptions:** none
- **Proves:** that folder is absent. Sprint layout under `docs/epics/` is otherwise process.

---

## Automated architecture rules (`ARCH-*`)

Owned by `lokai-arch-gate` static checks. `verify` fails if any fire. Do not duplicate in a second analyzer.

| ID | Short name | What it actually proves |
|----|------------|-------------------------|
| ARCH-PROC-001 | subprocess_spawn | `Command::new` text outside the spawn allowlist |
| ARCH-PROC-002 | git_via_process_broker | no raw `Command::new("git")` in tools git path |
| ARCH-PROC-003 | lsp_via_process_broker | LSP launcher is not raw Command |
| ARCH-PROC-004 | production_tools_sandboxed | production tools/worktree wiring matches sandbox patterns |
| ARCH-PROC-005 | no_old_verify_flag | no `LOKAI_USE_OLD_VERIFY` in engine `.rs`/`.toml`/`.yml` (gate source skipped) |
| ARCH-NET-001 | reqwest_boundary | `reqwest::` outside `lokai-egress` |
| ARCH-RT-001 | engine_runtime | production CLI/daemon do not `Agent::new` (heuristic) |
| ARCH-DEP-001 | dependency_direction | `lokai-policy` must not depend on `lokai-inference` |
| ARCH-DEP-002 | app_layer_deps | app Cargo edges |
| ARCH-DEP-003 | inference_no_enroll | inference must not depend on enroll |
| ARCH-DEP-004 | fabric_protocol_isolation | fabric protocol crate isolation |
| ARCH-RPC-001 | schema_methods | RPC schema methods have dispatch arms |
| ARCH-SIZE-001 | file_size | `.rs` files ≤ 900 lines except documented allowlist |
| ARCH-INFER-001 | unguarded_remote_dispatch | remote providers constructed only via authorized wiring |
| ARCH-INFER-002 | compute_broker_wiring | compute broker assembly patterns |
| ARCH-APP-* | various | CLI/daemon do not grow a second kernel |
| ARCH-FS-001 | workspace_mutation_bypass | known mutation bypass patterns |
| ARCH-RUN-001 | run_state_mutation_bypass | run-state mutation bypass patterns |
| ARCH-OUT-001 | outbound_secret_scanner | scanner wiring / secrets-context deps |
| ARCH-OUT-002 | fail_open_redact_new | no copied fail-open `Err(_) =>` / `unwrap_or` plaintext arms (`lib.rs` not exempt; definition stays in `lokai-secrets`) |
| ARCH-POL-001 | check_tool_new_sites | no production `check_tool` sites (allowlist emptied M5 CONVERGE) |
| ARCH-READ-001 | core_raw_read_new | no raw `tokio::fs` / `std::fs::read(` in `lokai-core` |
| ARCH-EGRESS-001 | egress_hygiene_new | no empty worker Infer/capacity `EgressGuard::new()`; no `LOKAI_FABRIC_LEGACY_CHAT_ONLY` production read |
| ARCH-APP-014 | app_door_new | no `turn_execution::execute_turn` in production `lokaid`; no production `LOKAI_RPC_TOKEN=` eprintln |
| ARCH-APP-015 | inspect_door_new | no `.supervisor.snapshot` / `.supervisor.resume_from_sequence` in `lokaid` `handlers/run.rs` or `lokai-eval` `recovery.rs` (FSM / fabric_run_bridge out of tripwire) |
| ARCH-PROD-001 | product_boundary_new | no `lokai-index` / `lokai-lsp` in `lokai-core` / `lokai-runtime` src or Cargo.toml; no `SpecialistRole` in orchestrator production src |
| ARCH-CTX-001 | context_compiler_wired | compiler wired in assembly |
| ARCH-ASYNC-001 | async_sync_calls | no `.read_sync`/`.write_sync` inside `async fn` body |
| ARCH-ASYNC-002 | no_mutex_store | no `Mutex<Store>` reintroduction |
| ARCH-FAB-001 | fabric_client_protocol | fabric-client uses protocol types |

**ARCH-SIZE-001 caveat:** line count is a **maintainability guardrail**, not proof of good architecture. Do not split modules solely to beat the counter.

**ARCH-PROC-001 caveat:** allowlisted `process_executor.rs` still has Constrained / `shell_command` `Command::new` (named M2 residual; DEL-001 execute mutant is DELETED). The gate prevents *new* spawn sites; it does not prove INV-PROC-001 ESTABLISHED.

**ARCH-POL-001:** no production `check_tool` (M5 CONVERGE emptied the allowlist). Does **not** prove INV-AUTH-001 ESTABLISHED.

**ARCH-OUT-002:** no copied fail-open plaintext redact arms (M4 CONVERGE removed the `lib.rs` allowlist). Does **not** prove INV-OUT-001 ESTABLISHED.

**ARCH-READ-001:** no raw `tokio::fs` / `std::fs::read` in `lokai-core` (M3 CONVERGE removed the prefetch allowlist). Does **not** prove INV-READ-001 ESTABLISHED.

**ARCH-EGRESS-001:** no empty worker Infer/capacity `EgressGuard::new()` and no `LOKAI_FABRIC_LEGACY_CHAT_ONLY` (M7 CONVERGE). Coordinator `new()` then reload is out of the tripwire. Does **not** prove INV-EGRESS-001 ESTABLISHED.

**ARCH-APP-014:** no production `lokaid` `turn_execution::execute_turn`, no production token-prefix eprintln (M8 CONVERGE). Inner `lokai-app` `execute_turn` and the INSECURE test primitive are out of the tripwire. Does **not** prove INV-APP-001 ESTABLISHED.

**ARCH-APP-015:** no `.supervisor.snapshot` / `.supervisor.resume_from_sequence` in production `lokaid` `handlers/run.rs` or `lokai-eval` `recovery.rs` (M10 CONVERGE). `fabric_run_bridge` / broker / `RunService` internal snapshot are out of the tripwire. Does **not** prove INV-APP-001 ESTABLISHED.

**ARCH-PROD-001:** no `lokai_index::` / `lokai_lsp::` in `lokai-core` / `lokai-runtime` production src, no those crates in those two Cargo.toml files, no `SpecialistRole` in orchestrator production src (M9 CONVERGE). Bins may still `Index::open` (DEL-036 leftover). `lokai-core` → `lokai-tools` is a named remainder. Does **not** prove INV-PROD-001 ESTABLISHED.

**ARCH-DEP-*: ** uses Cargo.toml / source patterns already in the gate (not a second dependency walker). V3 `Product → Application → Runtime → Authorities → Infrastructure` is **not** fully encoded yet; do not claim it is. Extend ARCH-DEP-* in the architecture packages, do not add a parallel QG dependency analyzer.

---

## Review-enforced rules (`RV-*`)

These stay in CONTRIBUTING / CHALLENGE / code review. Implementing them as regex would be enforcement theater.

| ID | Rule | Why not automated |
|----|------|-------------------|
| RV-ERR-001 | Actionable user- and model-facing errors | Requires reading the message and recovery path |
| RV-ERR-002 | No casual `unwrap`/`expect` on runtime paths | `clippy::unwrap_used` would explode existing debt; tests and infallible init need localized exemptions. Enable only as a dedicated cleanup, not this gate. |
| RV-CONC-001 | Avoid unnecessary hot-path `Arc<Mutex<T>>` | Presence of the type is not proof of contention; a ban would be false. |
| RV-ASYNC-001 | Heavy CPU off Tokio worker threads | `block_in_place` is used on purpose in compute paths; a global ban is wrong. |
| RV-COMPLEX-001 | Function size, nesting, premature abstraction | No justified threshold; high noise. |
| RV-AUTH-001 | Delete allowlisted `check_tool` dual API | Done at M5 CONVERGE (`ARCH-POL-001` now zero sites). INV-AUTH-001 stays PARTIAL (spawn Secret residual). |
| RV-AUTH-002 | No new tool-loop `execute_gated` skips | Regex of `if name ==` would be theater (finish / write_file / approval). Review until M3. |
| RV-MODEL-002 | Model-agnostic product logic beyond QG-MODEL-001 | Capacity/inference catalogs are data. |
| RV-FS-002 | New `std::fs::write` outside transactions | ARCH-FS-001 is partial; grep is not a transaction proof. |
| RV-NET-002 | Tools must not open the network | ARCH-NET-001 covers `reqwest::`; other HTTP stacks need review. |
| RV-V3-001 | Trajectory test (do not specialize Worker==Infer, etc.) | Design review |

---

## Verification tiers

From `engine/`:

### FAST (IMPLEMENT)

```bash
cargo run -p lokai-arch-gate -- verify fast
cargo run -p lokai-arch-gate -- verify fast --crate <crate>
cargo test -p <touched-crates>
```

Runs: rustfmt check, architecture static, quality static, optional `cargo check -p`. Does **not** run Clippy or workspace tests.

### PACKAGE (VERIFY)

```bash
cargo run -p lokai-arch-gate -- verify package
cargo test -p <touched-crates>
```

Runs: FAST plus `clippy --workspace --all-targets -- -D warnings`. CI uses this, then `cargo test --workspace` as a **separate** job step so tests are not executed twice inside the gate.

### FULL (CONVERGE)

```bash
cargo run -p lokai-arch-gate -- verify full
# when the package changes process/verify/eval behavior:
cargo run -p lokai-eval -- run --subset quality --corpus corpus
cargo run -p lokai-eval -- run --subset security --corpus corpus
```

Runs: PACKAGE plus `cargo test --workspace`. Eval is **not** inside the binary.

`--skip-fmt`, `--skip-clippy`, `--skip-tests` exist for local debugging; do not use them to declare VERIFY success.

---

## CI integration

[`.github/workflows/engine-ci.yml`](../../.github/workflows/engine-ci.yml) `test` job (Linux):

1. **Engineering gate** — `cargo run -p lokai-arch-gate -- verify package` (formatting + Clippy/warnings + architecture + quality static)
2. **Tests** — `cargo test --workspace`
3. **Audit** — `cargo audit`
4. **Security fixtures** — selected crate tests
5. **Generated artifacts** — `python scripts/gen_fabric_protocol_doc.py --check`

Eval subsets remain the `eval-gate` job. Windows/macOS jobs stay sandbox/platform tests and do not duplicate Linux fmt/clippy.

CI does **not** run `verify full` (that would rerun workspace tests inside the gate).

Failures print `QG-*` / `ARCH-*` ids. The combined engineering-gate step is intentional: fmt and Clippy are not repeated later.

---

## V3 orchestration

See [`docs/architecture/v3/08-OPERATING-PROTOCOL.md`](../architecture/v3/08-OPERATING-PROTOCOL.md).

| Stage | Quality obligation |
|-------|--------------------|
| PLAN | Name the QG/ARCH/RV rules the package can affect; record FAST/PACKAGE/FULL commands |
| IMPLEMENT | Run FAST often enough to catch local regressions |
| VERIFY | PACKAGE must pass; production-path tests as specified by the package |
| CONVERGE | FULL (workspace tests + gates); eval if process/verify changed |
| REBASELINE | Record gate evidence in `11-RUN-LOG.md`; list new suppressions in QUALITY-DEBT.md |

Package template: [`09-PACKAGE-TEMPLATE.md`](../architecture/v3/09-PACKAGE-TEMPLATE.md).

---

## Exemption model

Every custom exemption needs:

```text
Rule ID
Location
Reason
Owner / package
Expiry or deletion condition
```

Migration exceptions also cite a Deletion Ledger ID. Allowlists live in source (`FILE_SIZE_ALLOWLIST`, `is_allowed_subprocess`, `GRANDFATHER_ANYHOW`) **and** in QUALITY-DEBT.md. No unexplained ignore files.

---

## Agent-facing failures

Findings print: rule ID, title, path, what failed, why it matters, how to correct, and a link to this file’s heading.

---

## Limitations (what this system does not prove)

- Tests still passing does not mean architecture or quality is intact — that is why the other gates exist.
- Architecture gate passing does not mean V3 authorities exist (most INV-* are still NOT_ESTABLISHED).
- Quality gate passing does not mean unwrap-free runtime, good async design, or model-agnostic product behavior beyond QG-MODEL-001.
- File-size passing does not mean the module is well factored.
- Spawn allowlist passing does not mean QA-001 is fixed.
- New `check_tool` sites are gated (`ARCH-POL-001`); the dual API was deleted at M5 CONVERGE. That is not proof of INV-AUTH-001.
- Exact fail-open `Err(_)` / `unwrap_or` plaintext redact copies are gated (`ARCH-OUT-002`, `lib.rs` not exempt); that is not proof of INV-OUT-001.
- Pedantic Clippy, cognitive complexity, and “no `Arc<Mutex<T>>`” are not claimed.
