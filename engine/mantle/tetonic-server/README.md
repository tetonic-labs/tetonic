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
