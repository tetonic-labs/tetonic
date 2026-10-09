# MCP action tools — October 9, 2026

Agents can now use reviewed MCP tools that change remote state. The connection library discovers foreground tools with read-only, mutating or absent annotations. The owner reviews exact manifests and assigns selected tools to an agent through the existing capability flow. Connecting or discovering a server does not grant tools.

## Behavior

- The tool picker distinguishes service-reported reads from actions that can change state. Missing read-only annotations mean an action; missing destructive annotations on an action are treated conservatively. Annotations describe behavior, never confer permission.
- The composite tool host now reports each selected tool's actual read-only classification. Actions pass through the same ActionBroker, argument-bound capability consumption, provider disclosure and managed execution path as existing tools. Read-only/explain turns can no longer accidentally execute an MCP action as a read.
- Full manifests remain pinned in tool IDs, including annotations. Changed or removed tools fail before dispatch. Credential rotation, removal of approval and disconnect continue to revoke access. Saved selections are not silently rewritten.
- `isError` tool results retain bounded service text and structured feedback, so an agent can respond to domain errors such as “not adjacent.” JSON-RPC error bodies and transport diagnostics remain private. Tool content is service data, not additional authority or instructions.
- Results support text, structured data, embedded text resources and resource links. Links are passed as data and never fetched automatically. Binary media is still unsupported. Result size bounds apply to failures as well as successes.
- An interrupted action after dispatch reports `mcp_outcome_unknown`: the operation may have completed, and the agent should inspect service state before deciding whether to retry. The client does not automatically retry actions, even when a server reports idempotency. Cancellation requests and session cleanup are not rollback guarantees.
- Task-only tools are omitted without hiding the server's usable foreground tools.

## Startup configuration

Existing `read_tools` configurations retain their original meaning: a listed tool must still advertise `readOnlyHint: true`. A server cannot turn a read grant into a mutation by changing an annotation. New `action_tools` explicitly permits the named tools to have effects or unknown annotations. Lists must be disjoint, with at most 32 names combined; either list can be omitted. The UI connection flow continues to store reviewed manifests instead of these startup name lists.

```json
{
  "connections": [
    {
      "id": "village",
      "name": "The Village",
      "endpoint": "http://127.0.0.1:3002/mcp",
      "action_tools": [
        "village_perceive", "village_available_actions", "village_inspect",
        "village_navigate", "village_step", "village_interact",
        "village_speak", "village_idle", "village_acknowledge_events",
        "village_disconnect"
      ]
    }
  ]
}
```

Alternatively, start Village MCP, use **Connect a service** with the endpoint above and **No authentication**, review its tools, then select them in the agent editor. Perception is an action because connecting claims a villager and can update discovery memory. An existing game runner must release its controller before this agent can connect.

## Validation

The targeted Rust suite exercises explicit mutation grants, absent annotations, read-grant compatibility, changed manifests, action classification, actionable error feedback, resource results, dropped responses and canceled actions. Managed execution tests cover selected and unselected mutations, stop behavior, persisted approval, restart, token rotation and revocation. UI tests cover separate tool review and agent selection.

Verified: 18 targeted Rust tests passed, the opt-in real Village gameplay test passed separately, and 11 MCP UI tests passed. Frontend type checking, production build and architecture checks passed. The production bundle retains its size advisory.

Strict Clippy was attempted but blocked by existing Rust minimum-version warnings in `tetonic-memory/src/control/blackboard.rs` (`is_none_or` requires Rust 1.82 while the lint reports 1.80). Those concurrent changes were left intact; this is not a clean whole-workspace lint claim.

`cargo test -p tetonic-app mcp --lib` runs the protocol and managed tests. In the adjacent `village-mcp` checkout, `npm run test:tetonic` starts isolated game and MCP servers on ephemeral ports and runs the opt-in Rust test. It exercises local perception, inspection, rejected remote interaction, navigation, arrival, timber pickup and speech through Tetonic's actual MCP client. Both Node projects must be built first; `VILLAGE_REPO` and `TETONIC_REPO` can override their adjacent checkout paths.

## Remaining boundaries

This change covers foreground tools over the existing local HTTP/public HTTPS transport. MCP stdio process ownership, background tasks, OAuth, sampling/elicitation, standalone resources/prompts, binary media, per-call preview UI, persistent remote-session affinity and automatic reconciliation of unknown remote effects remain separate work. Sessions still open and close per invocation. Stateful integrations such as Village MCP must keep their controller outside these short-lived protocol sessions.

Tool behavior follows the [MCP tool result and annotation contract](https://modelcontextprotocol.io/specification/2025-11-25/server/tools).
