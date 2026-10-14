# lokai-tools

Workspace-scoped coding tools: read, grep, glob, edit, write, shell, finish, plus optional index, memory, LSP, and orchestration tools. No direct network access.

## Role in the stack

Every agent session gets a `Tools` instance bound to a workspace root (main tree or session worktree). `lokai-core` reads `Tools::defs()` for the model catalogue and calls `Tools::execute` / `Tools::execute_authorized`.

Production wiring (`lokai-runtime::build_tools`) and `Tools::new` both use `EnforcementLevel::Sandboxed` on `ProcessExecutor` (R6-2). Constrained remains available for unit tests via `with_enforcement_level`.

## Modules

| Module | Purpose |
|--------|---------|
| `lib.rs` | `Tools` dispatch, index/memory caches |
| `types.rs` | `ToolError`, `ToolOutcome`, argument schemas |
| `workspace.rs` | Path sandbox, TOCTOU-safe edits, `O_NOFOLLOW` writes |
| `catalog.rs` | Tool definitions and operator card |
| `mutation.rs` | `RepositoryMutationService` + `MutationSink` (AC2-4) |
| `process_executor.rs` | `ProcessExecutor` + `ProcessSink` (AC2-3); R4-3 stdout/stderr `ScannerEngine` redaction; R09 sandboxed `run_git` |
| `exec.rs` | Bounded shell execution (timeouts, env modes) |
| `verify.rs` | Auto-detect and run verify-before-finish commands |
| `worktree.rs` | Opt-in git worktree per session (`LOKAI_USE_WORKTREE=1`) |
| `lsp.rs` | LSP-backed tool wrappers |
| `orchestration.rs` | `spawn_agent` tool schema (host executes) |
| `sink.rs` | `ExecutionOutcome` → `ToolOutcome` mapping |

## Key API

Registered agents use `run_shell({"command":"python check.py","view":"staged"})`
to test pending `write_file`/`edit_file` changes in a disposable copy. The same
shell grant, exact-action approval, capability consumer, process executor and
cancellation apply. `ToolHost::prepare_parameters` binds the transaction, patch
digest and copy directory **before** approval; the issued action is never
rewritten. Failed checks block publication; repairs require a fresh passing
check. Output names the checked patch and states that generated files are not
published. Default `view: "workspace"` still runs against committed files.

The copy excludes build/dependency caches and protected directories and is
bounded to 20,000 entries/256 MiB. Commands must use relative workspace paths.
This enables verification; it does not guarantee that the chosen tests cover
the user's requirements. See `staged_shell.rs`, transaction `staged_check.rs`
and app `workspace/providers/tests/staged_checks.rs` for the managed path.

- `Tools::new`, `with_index`, `with_lsp`, `with_allowed_tools`, `with_enforcement_level`
- `Tools::execute_authorized` — routes mutations/shell through sinks when `AuthorizedAction` is present
- `Tools::run_verify_sink` / `run_command` — verify-before-finish via `ProcessSink`; with an open stage, cwd is the staged verify overlay (R10). Overlay cwd is bound at issue, not rewritten after consume.
- `Tools::apply_verify_overlay` — bind `AuthorizedAction.working_directory` to the overlay **before** capability issue (post-issue rewrite is ScopeMismatch)
- `Tools::validate_tool_args` — D7 schema validation (strict JSON coercion)
- `ProcessExecutor` / `RepositoryMutationService` — mandatory execution-contract sinks. Sandbox audit lines are one-line (`sandbox partial windows missing=…`); full capability JSON stays in tracing, not tool summaries.
- `Tools::commit_staged_if_any` / `abort_staged_if_any` — M2-4 / R6-3 success vs cancel-fail paths
- `detect_verify_command`, `resolve_verify_cmd`
- `ensure_session_worktree`, `remove_session_worktree`

## Enforcement tiers (`ProcessExecutor`)

| Tier | Behavior |
|------|----------|
| **Advisory** | Full inherited env; timeouts/output caps still apply |
| **Constrained** | Minimal env allowlist, argv-only verify, kill on timeout — **test/legacy only** |
| **Sandboxed** | OS sandbox via `lokai-sandbox`; **production default** (R6-2) |

## Dependencies

- `lokai-index` — structural/semantic retrieval tools
- `lokai-memory` — episodic `recall`
- `lokai-lsp` — diagnostics, goto, references
- `lokai-domain` — execution-contract sink traits

## Product plan

| ID | Feature |
|----|---------|
| T1–T5 | Core file/shell/finish tools |
| AC2-3 | `ProcessExecutor` / `ProcessSink` |
| AC2-4 | `RepositoryMutationService` / `MutationSink` |
| D2 | Worktree + exec timeouts |
| D7 | `validate_tool_args` |
| D8 | Verify auto-discovery |
| D9 | LSP tools |
| D11 | Specialist tool filtering |

## Tests

```bash
cargo test -p lokai-tools
cargo test -p lokai-arch-gate
```

Coverage includes symlink escape, shell metachar rejection, capability deny, minimal env, sink round-trips, and TOCTOU edit guards.

## Related docs

- [coding-tools-v1](../../../docs/implementation/contracts/coding-tools-v1.md)
- [execution-contract-v1](../../../docs/implementation/contracts/execution-contract-v1.md)
