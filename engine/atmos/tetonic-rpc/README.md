# lokai-rpc

Editor ↔ daemon boundary: LSP-style stdio framing and JSON-RPC v1 protocol types. Engine-free so schemas can generate TypeScript clients.

## Role in the stack

`tetonicd` is the only consumer at runtime. `engine/clients/ts/protocol.ts` is generated from these types. Keeps the fork/CLI and daemon decoupled from agent internals.

## Modules

| Module | Purpose |
|--------|---------|
| `framing.rs` | Content-Length headers over stdin/stdout (32 MiB cap, SEC-005) |
| `protocol.rs` | Methods, params, events, error codes |
| `outbound.rs` | Bounded outbound queue — token coalesce, progress replace, lossy log drop (AC2-10) |
| `server.rs` | `Notifier`, `writer_task`, `channel_pair` (tests) |

## Key API

- `PROTOCOL_VERSION`, `schema_bundle()`
- `OutboundQueue`, `Notifier`, `writer_task`, `DEFAULT_OUTBOUND_CAPACITY`
- Methods: `initialize`, `session/start`, `session/end`, `chat/send`,
  `session/cancel`, `approval/respond`, `agent/spawn`, `model/list`,
  `egress/policy.get`, `egress/policy.set`, `fabric/status`,
  `fabric/worker.trust.get|set`, `policy/get|set`, `estate/status`, capacity
  RPCs, `project/consolidate`, `run/snapshot`, `run/resume`, `shutdown`
- Events: `event/token`, `event/tool_call`, `event/run_status`, `event/approval_request` (typed `missing_controls` + `user_approval_required`), `event/egress`, `event/log`, `event/context`, `event/capacity/progress`
- Every event includes `session_id`, `seq`, **`agent_id`**

## Dependencies

None (foundation crate).

## Product plan

| ID | Feature |
|----|---------|
| B1 | RPC contract |
| D1/D3 | Policy and estate RPC |
| A13 | `agent/spawn` |
| D11/D12 | Orchestration params on `session/start` |
| AC2-10 | Bounded outbound queue |
| M5-3 | Worker trust get/set and audit result types |

## Tests

`cargo test -p lokai-rpc` — framing, protocol serde, outbound policy, notifier, schema bundle completeness.

Regenerate TS client:

```bash
cargo run -q -p tetonicd -- --print-schema | python scripts/gen_ts_protocol.py
```

## Related docs

- [agent-rpc-v1](../../../docs/implementation/contracts/agent-rpc-v1.md)
- `engine/scripts/gen_ts_protocol.py` — TS codegen
