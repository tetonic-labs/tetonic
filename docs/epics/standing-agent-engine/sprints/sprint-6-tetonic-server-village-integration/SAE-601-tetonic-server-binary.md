# SAE-601 — `tetonic-server` Binary Scaffold

| Field        | Value                                              |
|--------------|----------------------------------------------------|
| **Ticket**   | SAE-601                                            |
| **Sprint**   | Sprint 6 — Tetonic Server + Village Integration    |
| **Epic**     | Standing Agent Engine                              |
| **Type**     | Feature                                            |
| **Priority** | P0 — Critical Path                                 |
| **Estimate** | 3 pts                                              |
| **Depends**  | SAE-401 (EngineConfig), SAE-402 (NodeRole)         |

---

## Objective

Create the `tetonic-server` binary — the generic, intent-agnostic Tetonic engine daemon. This is **not** `lokaid` (which is a coding-agent interface). `tetonic-server` is the runtime that hosts agents, connects them to worlds, and routes their inference.

## Acceptance Criteria

- [ ] New crate at `engine/mantle/tetonic-server/` with `Cargo.toml` and `src/main.rs`.
- [ ] Added to workspace members in `engine/Cargo.toml` (already covered by `mantle/*` glob).
- [ ] CLI flags via `clap`:
  - `--mode <standalone|coordinator|runner>` (default: `standalone`)
  - `--world <ws_url>` — WebSocket URL of the world to connect to (e.g. `ws://127.0.0.1:3001/world/gateway`)
  - `--model <ollama_model>` — Model tag for inference (e.g. `qwen3.5:latest`)
  - `--ollama-url <url>` — Ollama API base URL (default: `http://127.0.0.1:11434`)
  - `--agent-id <id>` — Agent identity string (e.g. `barnaby`)
  - `--agent-charter <text>` — One-line intent charter for the agent
  - `--bind <addr>` — Listen address (default: `127.0.0.1:4000`, for future operator API)
- [ ] Boots in `Standalone` mode using `NodeRole::Standalone` from `tetonic-node`.
- [ ] Initializes `tracing-subscriber` with `RUST_LOG` env filter support.
- [ ] Prints startup banner: `tetonic-server v{VERSION} | mode={mode} | agent={agent_id}`.
- [ ] Compiles and runs (`cargo run -p tetonic-server -- --help`).

## Implementation Notes

```
engine/mantle/tetonic-server/
├── Cargo.toml
└── src/
    └── main.rs
```

### Dependencies (from workspace)
- `clap` (derive)
- `tokio` (full)
- `tracing`, `tracing-subscriber`
- `serde`, `serde_json`
- `anyhow`
- `tetonic-domain` (path = `../../core/tetonic-domain`)
- `tetonic-node` (path = `../tetonic-node`)

### Key Design Decision

This binary is **not** wired to stdio JSON-RPC like `lokaid`. It is a standalone daemon that:
1. Reads CLI args / config
2. Initializes the engine in the requested mode
3. Connects agent(s) to world(s)
4. Runs until `SIGINT` / `Ctrl+C`

The binary should be structured so that future tickets can add:
- TOML config file loading (`tetonic.toml`)
- HTTP operator API (health, metrics, E-Stop)
- Multi-agent fleet management

## Out of Scope

- WebSocket world transport (SAE-602)
- Inference provider wiring (SAE-603)
- Agent lifecycle / brain wiring (SAE-604)
- Operator API endpoints (future sprint)
