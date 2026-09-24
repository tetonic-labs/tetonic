# Tetonic server: local standing-agent milestone

`tetonic-server --config <path>` starts one configured agent using the real
`tetonic-core::Agent::run_in_world` loop, a prompt-configured brain backed by
`SingleModelBrain` / `OllamaProvider`, and the acknowledged TWP WebSocket adapter.

The current server supports **standalone, localhost operation only**. Unsupported
node modes and non-loopback URLs are rejected. The world URL, action vocabulary,
instructions, agent identity, charter, model and inference limits come from TOML;
no Village-specific behavior is compiled into the binary. The Village checkout
contains `tetonic.local.toml` and its `LOCAL-EXPERIMENT.md` runbook.

The server uses no OS/file tools. Decisions are bounded to the configured world
manifest and the authoritative world remains responsible for validating actions.
The inference boundary performs outbound secret scanning. WebSocket receipts are
correlated by action ID. A connection/E-Stop epoch prevents an old inference result
from becoming an action after disconnect/reconnect or stop/resume.

Health is available on the configured loopback bind (default in the example:
`http://127.0.0.1:4000/health`). Logs show inference starts, decisions and world
receipts. Set `RUST_LOG=info` to show them. Ctrl+C stops the process and closes its
world connection. A disconnect never requests scripted replacement behavior.

The four most recent decisions are retained in memory. This milestone does not
yet implement persisted agent memory, cluster roles, a fleet REST API, or durable
world state. Those remain separate work from the local experiment.

Checks:

```powershell
cargo test -p tetonic-runtime -p tetonic-server --lib --bins
cargo build -p tetonic-server
```
## Local observation memory

World adapters may supply `state.data.observations`, an array of locally observed facts with stable string `id` fields, and `state.data.memory_scope`, an opaque world-instance identifier. Each configured brain keeps up to 32 last-seen facts independently. Facts retain their observation sequence and become unverified when absent from the current observation. A changed scope clears those memories.

Current observations take precedence over remembered facts and prior decision intents. Only out-of-view memories are repeated in the decision context. This memory is bounded and in-process; it does not persist across server restarts. Worlds remain responsible for observation visibility and action validation. World geometry, sensing rules, game actions, and scenario configuration belong in the world repository.


## Decision budget and health

Optional inference settings `completion_tokens` (default 384) and `context_margin` (default 512) reserve space within `context_tokens`. Prompt assembly estimates UTF-8 bytes / 3 plus 64 chat-overhead tokens, trims optional history, and rejects essential overflow. This is disclosed heuristic accounting, not exact model tokenization. The trace includes `context_budget`, requested `max_tokens`, and runtime-reported finish reasons. Length-terminated decisions emit no action.

The loopback health response includes a payload-free `decision` snapshot even when raw observability is disabled. Runtime state is separate from the world's connection and physical activity. `waiting` means no inference/action currently in progress; it does not establish that the model voluntarily chose idle. Failure metadata remains available after the next decision begins.
