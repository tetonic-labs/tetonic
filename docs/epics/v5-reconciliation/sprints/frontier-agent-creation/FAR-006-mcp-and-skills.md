# FAR-006 — Real MCP connections, tool attachments and skills

Status: **in progress — local HTTP read-tool profile implemented**. Size: L. Parent: [frontier agent sprint](plan.md).

## Work

Implement approved stdio/HTTP MCP connection lifecycle, auth, discovery, manifest pinning and invocation through the common tool gateway. Distinguish consuming remote MCPs from exposing Tetonic tools to a harness. Add versioned skill create/import/load and disclose requirements without granting them. Add capabilities in the existing Tools and agent editor surfaces.

## Acceptance

One neutral real test MCP with a read and reversible mutation works through Tetonic and the first vendor harness. Expired auth, changed schemas, missing tools and cancellation are visible. New tools are not automatically granted. Imported skills preserve provenance and cannot escape policy.

The same granted MCP tools must work through every supported tool-capable model adapter (OpenAI, Anthropic, Google and local), using FAR-004's common disclosure/authority binding. An MCP connection is owned by the execution environment and scoped to its users/agents, not attached to a model-provider brand. The current Ollama-only profile below is an interim delivery restriction, not the product's target architecture.

## Delivered slice — October 6, 2026

An operator supplies `tetonic ui --mcp-config <file>` with named local HTTP servers and exact, vetted read-tool names. The existing Tools & MCPs and agent editor surfaces discover individual tools, show real connection failures, preserve selections across provider changes, and register only explicitly selected tools. No server is installed or launched. This is reusable domain-neutral tooling, exercised with calendar availability; it contains no calendar-specific runtime or workflow.

Reuse: `LocalWorkspace` → immutable ResourceService agent definition → existing job capability bindings and grants → registered managed executor → composite `ToolHost` → existing ActionBroker / one-use capability consumer → `EgressGuard` → MCP server. Results enter the normal model tool-result messages and work history. Managed usage, deadlines, cancellation and durable assignment receipts stay authoritative. No second scheduler, agent registry, credential store or permission system.

The granted tool ID pins the configured endpoint, connection ID and complete advertised manifest. Each invocation opens a separate MCP session and re-lists tools before calling the pinned version. Changed/removed tools fail before invocation; refreshing discovery does not update an existing agent's definition or expand its grants. A current agent must be recreated with the newly reviewed tool version until FAR-007 adds revision editing. Restarts require rediscovery; tools are not persistently declared ready.

The server must advertise `readOnlyHint: true` **and** the operator must name the tool in `read_tools`. The hint is a compatibility check, not a sandbox or proof of behavior. Operator vetting remains necessary. Local argument checks enforce object type and size; the complete pinned schema is advertised to the model, but domain schema validation remains the MCP server's responsibility.

## Supported profile and open work

| Area | Current support | Still open |
|---|---|---|
| Transport | Numeric loopback HTTP; bounded JSON and SSE; initialize/list/call/session cleanup | Remote HTTPS, OAuth/API-key connections, stdio process lifecycle, server deployment |
| Agent | User-created Ollama/general agents, selected per-tool grants; file tools can coexist | Hosted MCP result-disclosure contracts and frontier/vendor harness attachment |
| Tool behavior | Operator-vetted reads, text/structured results, exact manifest pin | Mutation previews/approval/effect receipts, full local JSON Schema validation, media/resources/prompts |
| Teamwork | Independently assigned agents reuse the registered execution path | Agreed-plan child tool grants; existing delegated children remain prompt-only plus human escalation |
| Stop | Stops waiting/inference, requests MCP cancellation, no automatic tool retry | Server termination acknowledgement; cancellation cannot undo or guarantee termination of server work |
| State | Cached discovery inventory; failures clear availability; durable selected IDs | Session resumption, SSE reconnect, background tasks, server sampling/elicitation |
| Skills | Unchanged | Versioned create/import/load and provenance |

Only the local UI host exposes this configuration in this slice. Do not imply daemon-wide or distributed connector deployment. See the [operator setup and protocol contract](../../../../implementation/contracts/local-ui-v1.md#local-mcp-connections).

## Evidence

- Real loopback HTTP fixture supports session negotiation, JSON/SSE discovery, tool calls and cancellation. Tests cover manifest replacement rejection before call, sanitized tool errors, malformed response IDs, non-granted mutation exclusion and cancellation without retry.
- A created agent runs through real ResourceService grants, managed activation, the action capability broker and MCP transport against fake Ollama. The selected calendar tool's result reaches the next inference request and durable work history. An unselected tool is not advertised and never reaches the MCP server. The same scenario passes on a file-enabled host, proving network resources do not become filesystem-version paths. A managed stop cancels an in-flight call and prevents the next inference. Reusing the work ID does not duplicate tool calls or inference.
- UI fixtures exercise discovery → individual selection → exact creation payload, incompatible provider changes without silent deselection, unavailable selection removal and discovery failures.
- Egress fixtures cover fragmented UTF-8/SSE framing, LF/CRLF/CR, JSON-RPC correlation, rejection of server requests, redirects, oversized responses, unsupported content types, invalid sessions and incomplete streams. Numeric loopback MCP uses a direct proxy-free client inside EgressGuard.

Final regression results are recorded in [FAR-008](FAR-008-conformance-and-product-proof.md). No external software was installed, no private external data was transmitted, no provider charges or OAuth grants were incurred, and no non-fixture MCP vendor compatibility or rebuilt-browser demonstration is claimed. Preserve the [audit's conformance boundaries](../october-1-coherent-workspace/agent-creation-frontier-audit-2026-10-06.md#10-required-conformance-and-release-evidence).
