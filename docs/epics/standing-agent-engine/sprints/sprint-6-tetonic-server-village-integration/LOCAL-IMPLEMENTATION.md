# Local integration implementation — September 24, 2026

The local experiment now runs through `tetonic-server`, not the TypeScript runner.
The engine is domain-neutral: the adjacent Village checkout holds the agent charter,
world instructions, supported verbs and local endpoints in `tetonic.local.toml`.

Implemented:

- Standalone configurable Rust server using `Agent::run_in_world`, `SingleModelBrain`
  through a prompt-configured perceptive wrapper, and the real `OllamaProvider`.
- Required outbound scanning on continuous brain inference, plus optional Ollama
  thinking-mode selection. The local Qwen configuration disables thinking for
  short action latency; existing provider callers retain their prior defaults.
- WebSocket TWP transport, action IDs and authoritative receipts; stale decisions
  are fenced across connection and E-Stop epochs. Optional event payloads decode.
- Village boundary normalizes Rust action payloads, validates the initial action
  vocabulary, and supplies legal steps and landmarks in perception.
- Strict experiment mode: no seeded agents, canned history, browser fallback
  movement, server-scripted villagers or takeover after engine disconnect.
- Server-driven sprite creation, canonical agent identity, reconnection binding,
  visible connection state, authoritative receipts and working E-Stop controls.
- Village `scripts/start-local.ps1` and `scripts/stop-local.ps1`, with hidden local
  processes, logs, and PID/start-time/executable verification for shutdown.

Validation:

- Both Village TypeScript checks pass; 43 world tests and 13 browser tests pass.
- Rust inference/runtime/server tests: 156 passed, two existing ignored tests.
- Observed model-generated movement, matching world receipts and live Pixi state.
- Browser E-Stop held position and prevented further actions; resume restored the loop.
- Stopping Tetonic left Barnaby stationary and marked disconnected; stopping the
  world marked the browser stale and disabled controls without fallback behavior.
- The launcher and stop script were exercised against their own recorded processes.

This is a local, single-agent milestone. World state and recent decision memory are
in-memory. It does not claim distributed operation, durable agent recovery, a fleet
management API, production deployment or a 24-hour soak. See the Village checkout's
`LOCAL-EXPERIMENT.md` for operation and configuration.
