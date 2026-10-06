# Frontier agent creation and execution

Started October 6, 2026. Status: **in progress**. This dedicated sprint is authorized by the user's request following the [source audit](../october-1-coherent-workspace/agent-creation-frontier-audit-2026-10-06.md). It is the immediate implementation focus under the existing v5 reconciliation epic.

## Outcome

A person creates an agent, chooses an available model and supported harness, attaches tools, and gives it work. Tools survive model/harness changes; actual compatibility and authority determine whether work can start. Streaming, tool effects, usage, human flags and cancellation remain visible in the current team workspace. The engine owns policy and coordination regardless of which lab supplies inference or the agent loop.

## Product requirement — provider-independent capabilities

User clarification, October 6: an agent's granted tools and MCP connections must work across supported OpenAI, Anthropic, Google and local-model routes. Provider-specific tool subsets are temporary implementation gaps, not the intended product contract. Adding another supported provider must require an inference adapter and conformance evidence, not another tool executor or permission system.

- The agent definition retains stable tool selections. Effective execution authority comes from current organization/team/agent/run grants, deployment scope and policy. A provider change neither expands grants nor silently removes selections.
- Each inference adapter translates the same tool manifests, calls, call IDs, results, streaming completion and required private continuation into its native protocol. A tool-capable model and a working adapter are prerequisites; an unsupported model must be identified explicitly rather than silently downgraded to prompt-only behavior.
- Tetonic governs tool execution through the existing tool hosts, action/process brokers and EgressGuard. MCP credentials stay at the connection/execution boundary; model providers receive only policy-permitted manifests, arguments and results. An MCP server may execute remotely from the inference provider.
- Tool/action permission and permission to disclose context/results to a model destination are separate checks. Changing model destination preserves the tool grant but may require additional authorization under the organization's data policy. Apply that rule to all destinations, including remote local-model endpoints; do not use provider brand as an authorization policy.
- A vendor harness must bridge the same selected tools through the same authority checks. Delegated team assignments inherit explicit scopes, budgets and stop lineage; choosing a different model or harness cannot change these rules.
- The UI presents one capability selection experience. Readiness explains specific missing connections, unsupported protocol capabilities or denied data flows while preserving the requested configuration.

Acceptance: run the same neutral MCP and ordinary tool scenarios through OpenAI, Anthropic, Google and local adapters. Exercise success, unselected-tool denial, mismatched/changed manifests, argument/result correlation, errors, partial streams, cancellation, revocation and denied disclosure. Repeat the selected-tool path through managed team dispatch and the first enabled vendor harness. Do not claim parity from a model dropdown, standalone probe or fixture-free assertion.

## Local execution scope — October 6 clarification

Tool parity includes shell commands and installed local command-line tools, as well as files and configured MCP tools. Inference location does not select the execution machine: the agent's host executes its granted local tools. Terminal is an explicit permission, with the exact proposed command reviewed in the existing Needs you area. No implicit terminal grant is added to existing agents.

## Immediate implementation order

This clarification supersedes the earlier vendor-first ordering below. Prioritize FAR-004's common capability/disclosure binding with FAR-006 MCP use; qualify the existing OpenAI and Anthropic adapters plus the new Google route under FAR-002/003/008. Carry the resolved binding through FAR-007 team dispatch. FAR-005 must then connect its harness to that same path. Versioned editing and lifecycle controls continue to reuse existing identity/revision primitives. Do not remove the current guards before their common replacement and tests exist. This is a target and priority change, not a claim that the current restrictions have been removed.

## Relationship to the October plan

This is a dedicated implementation sprint, not an extra calendar week or an independent architecture. It expands and specializes OCT-102/103/105 and COORD-A/B plus connected capability work. Existing October release gates stay open. October 25 remains the target; the first supported profile and remaining vendor coverage must be re-estimated from evidence. Do not pretend all rows below fit the remaining time. The prior statement that no fourth sprint is added is superseded for this focused workstream; the three calendar windows remain the planning baseline.

## Work

| Ticket | Status | Size | Delivery |
|---|---|---|---|
| [FAR-001](FAR-001-creation-and-readiness.md) | Verified for current profiles | M | Creation contracts and honest readiness |
| [FAR-002](FAR-002-model-discovery-and-connections.md) | In progress | L | Account-aware model discovery and connections |
| [FAR-003](FAR-003-modern-inference-and-streaming.md) | In progress | L | Modern inference protocols, streaming and tool continuity |
| [FAR-004](FAR-004-governed-tools.md) | In progress | L | Selected tools across frontier models |
| [FAR-005](FAR-005-vendor-harnesses.md) | In progress | L | Frontier harness adapters inside managed execution |
| [FAR-006](FAR-006-mcp-and-skills.md) | In progress | L | Local MCP read-tool profile delivered; remote/auth/writes, harness attachment and skills open |
| [FAR-007](FAR-007-team-dispatch-and-edits.md) | In progress | L | Versioned agent editing delivered; mixed-runtime team dispatch open |
| [FAR-008](FAR-008-conformance-and-product-proof.md) | In progress | L | Conformance, installation and end-to-end product evidence |

Sizes indicate breadth and uncertainty, not days. First close creation correctness, then prove a modern direct model/tool round trip, then the same assignment through a vendor harness. MCP and mixed-team execution use those same contracts. Run the vendor restriction/egress feasibility spike before committing to its full adapter. API auth and ChatGPT-plan auth are distinct supported profiles and require distinct evidence.

## Reuse and boundaries

Reuse ResourceService, durable identity/revisions, grants, managed attempts, action/process brokers, EgressGuard, usage reservations and existing UI readers. Extend the existing executor contract. Do not add a parallel fleet registry, scheduler, approval authority, history or budget ledger. Never enable an unsupported capability just by removing a guard. New provider state stays scoped to the attempt/session and never silently joins shared team memory.

## Verification and completion

Each ticket records implementation, exact checks, limitations and evidence. Unit/protocol fixtures do not prove account entitlement or live provider compatibility. Keep real inference trials bounded and separately metered. Commit reviewable increments; do not push or publish as part of this sprint unless requested. Sprint completion requires the declared supported model and vendor-harness profiles to perform real selected-tool work through the existing product, plus truthful failures, inherited stop and usage evidence.

## Current evidence

Latest October 6 increment: the common disclosure binding is implemented, and direct Ollama/OpenAI/Anthropic/Google general agents use the same granted file/MCP execution path. Google discovery/native function calling and Anthropic native-ID/private-continuation fixes are integrated. Hosted file writes reuse staging/finalization; MCP remains local HTTP reads. Shared fixtures cover real effects/results and denial/stop behavior. See FAR-004 and FAR-008 for the current evidence; earlier entries below describe historical slices. No live-provider or installed vendor-harness completion is claimed.

The next integration boundary is FAR-007: resolve each delegated worker's provider, exact tools and authorized input/result disclosure, preserving derived grants, context privacy, budgets and parent stop. FAR-005 must then bridge vendor harnesses into that same execution contract. These remain substantive work, not configuration toggles.

- Source audit: implementation tracing plus nine UI tests and one mocked backend test at baseline. No vendor harness or real MCP integration was proven.
- [FAR-001 evidence](FAR-001-creation-and-readiness.md#evidence): compatibility and preserved selections, invalid-profile rejection, hosted execution on file-enabled hosts; 25 Rust and nine UI tests passed, plus TypeScript. Mocked provider evidence only.
- [FAR-002 partial evidence](FAR-002-model-discovery-and-connections.md#evidence): account catalog discovery and current-editor refresh/error paths; four application, six egress and twelve UI tests passed; TypeScript and CLI compilation passed. OAuth and full capability qualification remain open.
- [FAR-003 partial evidence](FAR-003-modern-inference-and-streaming.md#evidence): OpenAI Responses streaming, complete-call validation and private in-memory continuation. Durable resumption and other vendor protocols remain open.
- [FAR-004 partial evidence](FAR-004-governed-tools.md#evidence): scoped OpenAI file reads through managed execution and existing tools; selected/unselected/path-escape/secret cases exercised with a fake provider and real files. 28 workspace, 47 core and 13 editor tests passed. Writes and vendor-harness execution are not enabled.
- [FAR-005 feasibility evidence](FAR-005-vendor-harnesses.md#evidence): actual Codex 0.160.0 app-server against an offline provider; dynamic-tool success, unselected custom/native tool rejection, pending-tool interruption and cumulative usage passed. Integration into managed execution and a guarded inference gateway remains required before enabling the harness in the product.
- [FAR-001 creation-to-use follow-up](FAR-001-creation-and-readiness.md#creation-to-first-assignment-follow-up): exact partial tool selections, setup refresh and known-problem status, same-agent credential repair, direct first assignment and pinned recipient retries. Hosted file approval now binds the folder actually displayed. Full web suite: 138 tests; production build, three provider application tests and package engineering gate passed. This does not close vendor-harness integration or live provider proof.
- [FAR-006 first MCP profile](FAR-006-mcp-and-skills.md#delivered-slice--october-6-2026): operator-configured local HTTP read tools, discovery and individual selection in the current editor, pinned manifests, actual managed execution and stop behavior. Domain-neutral fixture proof; hosted/harness MCP, mutations, delegated child tools and skills remain open.
