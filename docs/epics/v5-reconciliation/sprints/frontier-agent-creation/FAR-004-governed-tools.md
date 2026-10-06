# FAR-004 — Provider-independent tool execution and disclosure

Status: **in progress — direct-agent file/shell/MCP parity implemented for Ollama, OpenAI, Anthropic and Google; fixture verified**. Size: L. Parent: [frontier agent sprint](plan.md).

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

### Provider-independent direct agents — October 6 follow-up

The first three coupling rows above are reconciled. Version-1 `ToolDisclosure` pins the provider, inference endpoint, exact selected tools and optional jailed folder. It is data consent, not execution authority. MCP tool IDs retain their endpoint/manifest pin. All four runtime profiles advertise the same host-supported file and discovered MCP tools. Hosted MCP-only agents need no workspace. General agents still use ResourceService definitions, existing execution grants, managed attempts, ActionBroker, `RegisteredToolHost`/`McpToolHost`, EgressGuard, secret scanning and staged file finalization. No parallel executor or grant store was introduced.

Hosted approval now covers selected tool inputs/results. The editor retains tools across provider changes and resets consent when the model, folder or selection changes. The server independently binds/rechecks the scope. Conservative legacy support accepts old OpenAI read approvals only; it cannot add new provider, MCP or write permissions. Missing keys and changed destination/folder bindings block activation.

One shared fixture matrix exercises real file reads and staged writes, genuine result correlation, unselected-write denial, path escape, secret-result denial and legacy/destination checks against all three hosted protocols. Another exercises actual local HTTP MCP through created hosted agents, including unselected calls, changed manifests, key removal and managed cancellation. Existing Ollama tests exercise the same underlying runtime and MCP host. Results and commands are recorded in FAR-008. No paid inference or live vendor compatibility is claimed.

Remaining: inherited tools/data scope for agreed-plan children (fourth coupling row), interactive approval beyond shell commands, vendor harness attachment, durable frontier continuation, stronger capability qualification and live provider/product evidence. Provider independence is implemented for direct general-harness agents; this does not finish the whole ticket or sprint.

### Local terminal increment — October 6

Direct general agents can select `run_shell` with Ollama, OpenAI, Anthropic or Google. This uses the existing Tools implementation, ActionBroker policy/capability store, sandbox ProcessExecutor and managed cancellation. A per-agent broker facade connects the host approval callback after managed attempt assignment; it shares policy and capability storage, without a separate executor or permission registry. Commands receive the platform shell identity in their tool description, and can use installed CLI programs within existing policy and command restrictions. No background-service lifecycle is added.

Schema 61 adds an optional exact shell proposal to existing effect approvals. The initiating owner reviews command, shell, working folder and predicted OS confinement gaps in Needs you. Its digest binds the proposal, canonical command parameters, call and attempt; consumption is transactional and once only. Existing work/run/deadline/stop checks gate proposal, resolution and consumption. Requests from a person's participation context are visible/resolvable only by that person, not every team member. Missing work bindings and other interactive action kinds deny. The legacy generic approval endpoint cannot create shell payloads.

Terminal permissions require control storage outside the working folder. The existing protected-store, traversal, credential and inline-code checks now also run at the capability-backed shell sink. Windows process-tree containment is reused, but filesystem confinement is unavailable; the working folder must not be represented as an OS security boundary. Host-specific gaps are displayed before approval. Provider disclosure consent remains separate from permission to run the command.

The local catalog no longer advertises `outline`/`search_code` without a bound code index. This does not remove their existing implementations. File browsing/search, granted commands and discovered local MCP read tools remain available; LSP/index configuration and other native integrations need their own actual bindings.

Still open: delegated team-tool inheritance, vendor harnesses, remote/authenticated/write MCP, durable provider continuation and live-provider qualification. Shell approval waits use the existing execution deadline; they are not resumable approvals after restart. Existing recovery makes stale attempts non-executable. This increment does not provide a shell confined to its folder on Windows or imply unrestricted tool access.

### Historical first slice

The current connected editor allows selected OpenAI file reads (`read_file`, `list_dir`, `grep`, `glob`) where the host permits them. Creation requires explicit disclosure consent, recorded against the canonical configured folder. Submission rechecks that scope, narrows the tool host to the selected tools, and uses existing ResourceService grants, managed execution, action broker, usage reservations and the egress scanner. Changing the host folder invalidates that approval. Prompt-only agents retain no ambient workspace access.

The managed round-trip test uses a fake Responses transport with real temporary files and real tool execution. It proves file contents reach the next model call, original call IDs survive success/denial paths, an unselected write has no effect, parent-directory traversal returns a denial, and a fixture secret blocks further inference. This test found and fixed missing tool-result IDs in regular and failure branches of the existing core loop.

The creation follow-up closes a stale-form approval gap: hosted tool creation also supplies `expected_workspace_root`, the canonical catalog folder actually shown when consent was given. Missing or mismatched values are rejected before registration. This is an equality check against the host's own root, never a caller-selected path or authority. Refreshing the form onto a different folder clears effective file-disclosure approval. Existing prompt-only and local-agent creation requests are unaffected; hosted-tool API clients must supply the displayed root. All three provider application tests passed, including new missing/stale-folder rejection cases and the real-file fake-provider round trip.

Checks: 28 local-workspace tests passed; three existing live-model scenarios remain ignored. All 47 core library tests and 13 focused editor tests passed; TypeScript passed. The package engineering gate passed (workspace Clippy with warnings denied, formatting, architecture and static quality). Live paid inference, live browser execution and an installed-server restart were not performed.

Still open: the common capability/disclosure binding, reversible writes with integrated approval/finalization, Anthropic and Google qualification, hosted use of the now-implemented local MCP client, inherited model/tool scope for team dispatch, revocation during an active file request, and live provider proof. This is a tested read-only slice, not completion of the acceptance criteria. Preserve the [audit's conformance boundaries](../october-1-coherent-workspace/agent-creation-frontier-audit-2026-10-06.md#10-required-conformance-and-release-evidence).
