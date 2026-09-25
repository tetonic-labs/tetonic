# lokai-core

An idle, already-created agent can change its inference dependency through
`Agent::replace_inference` with an `AgentInferenceBinding`. The owning runtime
supplies an admitted provider, model, context budget, and tokenizer. Identity,
tools, policy, hooks, and history stay intact; token accounting is invalidated.
See [session inference selection](../../product/lokai-app/INFERENCE-SELECTION.md)
for the application operation used by the TUI and daemon.

Single-agent tool loop: advertise tools → call model → execute tools → feed results back until `finish`, cap, or cancel.

## Role in the stack

Sits between **inference** (`lokai-inference`) and composition (`lokai-runtime`). Both `tetonic-cli` and `tetonicd` assemble an `Agent` from this crate. The orchestrator (`lokai-orchestrator`) wraps the same loop with routing and specialist overlays — it does not replace this crate. Capability implementations (`lokai-tools`, `lokai-transaction`) are wired by composition, not by this crate.

## Public API

| Type / fn | Purpose |
|-----------|---------|
| `Agent` | Main loop: `run`, `turn`, hooks for audit and approval |
| `Conversation` | Multi-turn state, cooperative cancel |
| `AgentConfig` | Model/budgets plus durable run identity, workspace root, data class, briefing, and specialist overlay |
| `Step` | Streaming events: tokens, tool calls, context budget, stopped |
| `Tokenizer`, `HeuristicTokenizer`, `ExactTokenizer` | Context token counting |
| `ApprovalHook`, `AuditSink` | Shell gating and persistence hooks |
| `TurnState`, `TurnOpsHook` | Operational turn FSM + persistence callback (AC2-6) |

## Dependencies

- `lokai-inference` — model calls
- `lokai-domain` — host contract, invocation, outcome
- `lokai-policy` — policy engine attachment

## Product plan

| ID | Feature |
|----|---------|
| A1 | Agent loop |
| D1 | Session data class on config |
| D4 | `project_context` injection |
| D5 | Session briefing on first turn |
| D7 | Tool-arg validate (capability host) + one repair pass on bad args |
| D11 | `specialist_role`, `system_overlay` |
| M5-3 | Full-message classification metadata, durable run/trace identity, and workspace binding |

## Tests

`cargo test -p lokai-core` — action-task tool requirement heuristic.

## Related docs

- [coding-tools-v1](../../../docs/implementation/contracts/coding-tools-v1.md)
- [agent-rpc-v1](../../../docs/implementation/contracts/agent-rpc-v1.md)
