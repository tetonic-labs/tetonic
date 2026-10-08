# tetonic-cli

Host and operator commands for the local team workspace. Package name is
**`tetonic-cli`**; the binary on PATH is **`tetonic`**.

`tetonic ui --database <dedicated-db> --model <installed-model>` starts the
loopback-only adapter for the live web workspace. Open its owner-only connection
link. Configuration, authentication and current limits are in the
[local UI contract](../../../docs/implementation/contracts/local-ui-v1.md).

The former terminal coding chat (one-shot tasks, interactive TUI, code index,
time travel and project memory flags) was removed in October 2026. Agent work
runs through the governed team workspace.

## Commands

| Command | Purpose |
|---------|---------|
| `tetonic ui` | Serve the local team workspace API to the web UI |
| `tetonic job` | Launch a registered job with operator-supplied host settings |
| `tetonic control` | Offline operator control of agents, contexts, work and execution limits |
| `tetonic estate …` | Worker enrollment, worker trust and capacity |
| `tetonic estate worker trust get <worker>` | Show persisted trust and audit history |
| `tetonic estate worker trust set <worker> <tier>` | Persist a globally versioned coordinator-owned trust assignment |

`tetonic` with no command, `help` or `--help` prints this list.

## Modules

| Module | Purpose |
|--------|---------|
| `main.rs` | Command dispatch |
| `local_ui.rs` | Loopback HTTP adapter for `LocalWorkspace` |
| `job.rs` / `job_view.rs` | Registered job launch and its result view |
| `control.rs`, `control/` | Offline operator control over resource services |
| `estate.rs` / `capacity.rs` | Estate subcommands; status/doctor via `tetonic-app` services |

The standalone trust command writes the shared `lokai.db`. The coordinator
re-reads that assignment and global policy epoch immediately before each remote
attempt, so a running coordinator cannot dispatch using the old tier.

## Tests

`cargo test -p tetonic-cli` — operator control CLI integration tests.
