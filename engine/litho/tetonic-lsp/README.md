# lokai-lsp

Local LSP client: spawn language servers as stdio subprocesses (rust-analyzer, pyright, typescript-language-server / vtsls). No network.

## Role in the stack

When a language server is on PATH and `LOKAI_LSP` is not disabled, `lokai-tools` exposes `lsp_goto_definition`, `lsp_find_references`, `lsp_diagnostics`. Set `LOKAI_LSP=0` (or `false`/`off`) to disable. With an approval hook attached (daemon), each `lsp_*` tool call may require user approval (`lokai-core`).

## Modules

| Module | Purpose |
|--------|---------|
| `detect.rs` | Find language servers on PATH (Rust, Python, TS/JS) |
| `framing.rs` | Content-Length JSON-RPC over stdio (32 MiB cap) |
| `client.rs` | Sync request/response, `didOpen`/`didChange`, health |
| `pool.rs` | Reuse server processes per workspace + language; respawn on crash |
| `state.rs` | LSP subprocess lifecycle FSM (AC2-10) |
| `util.rs` | Content hashing, UTF-16 position helpers |

| `launcher.rs` | Pluggable `LspProcessLauncher`; production installs sandbox launcher (R29) — no default `Command::new` |

## Key API

- `detect_server`, `available_languages`, `lsp_enabled_by_env`
- `set_process_launcher` / `clear_process_launcher` — thread-local spawn backend
- `LspPool`, `LspSession`, `LspLifecycle`
- `Lang`, `ServerSpec`, `DiagnosticHit`, `LocationHit`
- `utf16_col_at_byte`, `normalize_character`

## Dependencies

None. Consumed by `lokai-tools` (`SandboxLspLauncher` via `ProcessExecutor::configure_lsp_launcher`).

## Product plan

| ID | Feature | Status |
|----|---------|--------|
| D9 / T12 | LSP adapter | Partial (servers optional on PATH) |
| R29 | Long-lived spawn via sandbox broker path | **Done** |

## Tests

`cargo test -p lokai-lsp` — framing caps, language detection, lifecycle FSM, UTF-16 helpers, mock-server integration, pool respawn.

## Related docs

- [coding-tools-v1](../../../docs/implementation/contracts/coding-tools-v1.md)
