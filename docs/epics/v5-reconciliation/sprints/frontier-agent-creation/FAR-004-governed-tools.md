# FAR-004 — Provider-independent tool execution and disclosure

Status: **in progress — scoped OpenAI file reads implemented**. Size: L. Parent: [frontier agent sprint](plan.md).

## Work

Use existing Tools, execution grants, action/process brokers and egress policy for an attempt-scoped tool manifest. Add scoped hosted disclosure authorization before allowing selected file tools; reading a file and sending it to a lab are distinct permissions. Pin schemas and record actual effects/results. Skills and tool packs never grant authority.

### October 6 clarification and implementation targets

Deliver the [provider-independent capability contract](plan.md#product-requirement--provider-independent-capabilities). A granted file or MCP tool must work with every supported tool-capable provider/model and harness profile. The engine owns the grant and executor; providers own protocol encoding and inference. Current restrictions remain until the common authority/disclosure path is implemented and proven.

The current source has concrete coupling to reconcile:

| Location | Current coupling | Required change |
|---|---|---|
| `local_workspace/agents/profiles.rs` | Ollama gets local/MCP tools; OpenAI gets a file-read subset; Anthropic gets none | Resolve compatibility from adapter capabilities plus separately evaluated grants/disclosure scope. Preserve one selected tool set. |
| `local_workspace/providers.rs` and `GeneralAgentPreferences.hosted_workspace` | Hosted tool disclosure is an OpenAI-only folder binding | Generalize the pinned approval to data sources, exact selected tool/connection versions and model destination, retaining existing folder protections. |
| `local_workspace.rs` and `resources/registered_executor.rs` | Hosted submission clears MCP; executor rejects hosted MCP and validates only file-read names | Assemble the same scoped tool host for supported inference routes after common disclosure and grant checks. Preserve activation fingerprints and idempotency. |
| `local_workspace/plan_execution.rs` | Team contributors must use local inference without external/file tools | Pass each worker's resolved model/tool/disclosure binding through existing derived grants, context handoffs, budget reservations and parent cancellation. |

Reuse `ToolAdvertisement`, native inference adapters, `RegisteredToolHost`/`McpToolHost`, the registered executor and existing authority stores. New connection/disclosure bindings extend those contracts; no parallel agent registry, tool authorization database or independent execution loop. Version new persisted fields and preserve existing definitions conservatively. A provider change must not silently reuse private protocol continuation or broaden approved data destinations.

## Acceptance

Selected read and reversible write work with a frontier model, while unselected tools, path escape, private-context disclosure and revoked access fail. Actual serialized outgoing data is inspected. Tools and receipts appear in the existing work/map inspector. No placeholder or unconnected service is represented as usable.

Run the same ordinary-tool and MCP cases against OpenAI, Anthropic, Google and local adapters with equivalent grants. Their wire payloads may differ; the authorized effect, result delivery, denial and stop semantics must agree. Unsupported adapters/models are explicit readiness failures. Changing provider retains requested tools and triggers only genuinely required destination-policy checks; no provider-specific permission recreation.

## Evidence

The current connected editor allows selected OpenAI file reads (`read_file`, `list_dir`, `grep`, `glob`) where the host permits them. Creation requires explicit disclosure consent, recorded against the canonical configured folder. Submission rechecks that scope, narrows the tool host to the selected tools, and uses existing ResourceService grants, managed execution, action broker, usage reservations and the egress scanner. Changing the host folder invalidates that approval. Prompt-only agents retain no ambient workspace access.

The managed round-trip test uses a fake Responses transport with real temporary files and real tool execution. It proves file contents reach the next model call, original call IDs survive success/denial paths, an unselected write has no effect, parent-directory traversal returns a denial, and a fixture secret blocks further inference. This test found and fixed missing tool-result IDs in regular and failure branches of the existing core loop.

The creation follow-up closes a stale-form approval gap: hosted tool creation also supplies `expected_workspace_root`, the canonical catalog folder actually shown when consent was given. Missing or mismatched values are rejected before registration. This is an equality check against the host's own root, never a caller-selected path or authority. Refreshing the form onto a different folder clears effective file-disclosure approval. Existing prompt-only and local-agent creation requests are unaffected; hosted-tool API clients must supply the displayed root. All three provider application tests passed, including new missing/stale-folder rejection cases and the real-file fake-provider round trip.

Checks: 28 local-workspace tests passed; three existing live-model scenarios remain ignored. All 47 core library tests and 13 focused editor tests passed; TypeScript passed. The package engineering gate passed (workspace Clippy with warnings denied, formatting, architecture and static quality). Live paid inference, live browser execution and an installed-server restart were not performed.

Still open: the common capability/disclosure binding, reversible writes with integrated approval/finalization, Anthropic and Google qualification, hosted use of the now-implemented local MCP client, inherited model/tool scope for team dispatch, revocation during an active file request, and live provider proof. This is a tested read-only slice, not completion of the acceptance criteria. Preserve the [audit's conformance boundaries](../october-1-coherent-workspace/agent-creation-frontier-audit-2026-10-06.md#10-required-conformance-and-release-evidence).
