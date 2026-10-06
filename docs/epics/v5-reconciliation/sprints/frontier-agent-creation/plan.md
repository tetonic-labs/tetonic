# Frontier agent creation and execution

Started October 6, 2026. Status: **in progress**. This dedicated sprint is authorized by the user's request following the [source audit](../october-1-coherent-workspace/agent-creation-frontier-audit-2026-10-06.md). It is the immediate implementation focus under the existing v5 reconciliation epic.

## Outcome

A person creates an agent, chooses an available model and supported harness, attaches tools, and gives it work. Tools survive model/harness changes; actual compatibility and authority determine whether work can start. Streaming, tool effects, usage, human flags and cancellation remain visible in the current team workspace. The engine owns policy and coordination regardless of which lab supplies inference or the agent loop.

## Relationship to the October plan

This is a dedicated implementation sprint, not an extra calendar week or an independent architecture. It expands and specializes OCT-102/103/105 and COORD-A/B plus connected capability work. Existing October release gates stay open. October 25 remains the target; the first supported profile and remaining vendor coverage must be re-estimated from evidence. Do not pretend all rows below fit the remaining time. The prior statement that no fourth sprint is added is superseded for this focused workstream; the three calendar windows remain the planning baseline.

## Work

| Ticket | Status | Size | Delivery |
|---|---|---|---|
| [FAR-001](FAR-001-creation-and-readiness.md) | Verified for current profiles | M | Creation contracts and honest readiness |
| [FAR-002](FAR-002-model-discovery-and-connections.md) | In progress | L | Account-aware model discovery and connections |
| [FAR-003](FAR-003-modern-inference-and-streaming.md) | In progress | L | Modern inference protocols, streaming and tool continuity |
| [FAR-004](FAR-004-governed-tools.md) | Planned | L | Selected tools across frontier models |
| [FAR-005](FAR-005-vendor-harnesses.md) | Planned | L | Frontier harness adapters inside managed execution |
| [FAR-006](FAR-006-mcp-and-skills.md) | Planned | L | Real MCP connections, tool attachments and skills |
| [FAR-007](FAR-007-team-dispatch-and-edits.md) | Planned | L | Mixed-runtime teams and versioned agent edits |
| [FAR-008](FAR-008-conformance-and-product-proof.md) | Planned | L | Conformance, installation and end-to-end product evidence |

Sizes indicate breadth and uncertainty, not days. First close creation correctness, then prove a modern direct model/tool round trip, then the same assignment through a vendor harness. MCP and mixed-team execution use those same contracts. Run the vendor restriction/egress feasibility spike before committing to its full adapter. API auth and ChatGPT-plan auth are distinct supported profiles and require distinct evidence.

## Reuse and boundaries

Reuse ResourceService, durable identity/revisions, grants, managed attempts, action/process brokers, EgressGuard, usage reservations and existing UI readers. Extend the existing executor contract. Do not add a parallel fleet registry, scheduler, approval authority, history or budget ledger. Never enable an unsupported capability just by removing a guard. New provider state stays scoped to the attempt/session and never silently joins shared team memory.

## Verification and completion

Each ticket records implementation, exact checks, limitations and evidence. Unit/protocol fixtures do not prove account entitlement or live provider compatibility. Keep real inference trials bounded and separately metered. Commit reviewable increments; do not push or publish as part of this sprint unless requested. Sprint completion requires the declared supported model and vendor-harness profiles to perform real selected-tool work through the existing product, plus truthful failures, inherited stop and usage evidence.

## Current evidence

- Source audit: implementation tracing plus nine UI tests and one mocked backend test at baseline. No vendor harness or real MCP integration was proven.
- [FAR-001 evidence](FAR-001-creation-and-readiness.md#evidence): compatibility and preserved selections, invalid-profile rejection, hosted execution on file-enabled hosts; 25 Rust and nine UI tests passed, plus TypeScript. Mocked provider evidence only.
- [FAR-002 partial evidence](FAR-002-model-discovery-and-connections.md#evidence): account catalog discovery and current-editor refresh/error paths; four application, six egress and twelve UI tests passed; TypeScript and CLI compilation passed. OAuth and full capability qualification remain open.
