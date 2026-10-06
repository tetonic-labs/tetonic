# FAR-007 — Mixed-runtime teams and versioned agent edits

Status: **in progress — versioned local agent editing delivered; mixed-runtime dispatch remains open**. Size: L. Parent: [frontier agent sprint](plan.md).

## Work

Replace local/prompt-only dispatch assumptions with resolved execution bindings for guide, coordinator and workers. Reuse existing child work, grants, payer reservations and context handoffs. Add agent edits and deliberate default-revision selection; keep active work pinned and revoke live authority immediately.

## Acceptance

Two supported agents using different runtime profiles collaborate with real tools and scoped handoffs under one allowance and parent stop. Editing an agent preserves identity and affects later work only unless explicitly rebound. Planner readiness and launch validation agree. No independent-root fallback.

A contributor retains its selected model/provider/harness and approved ordinary/MCP tools when it receives team work. Reuse FAR-004's resolved execution/disclosure binding and derive narrower child authority; never substitute a local prompt-only agent or create an independent root to work around an unsupported binding. Include mixed-provider MCP work in the managed team conformance proof. Team membership does not disclose the agent owner's private conversations or provider continuation state.

## Evidence

October 6: Agents → select a teammate → Edit agent now uses the existing creation form and common validation. Name, purpose, provider/model, supported harness, limits, local tools (including Terminal), and individually discovered MCP tools can be edited. Existing exact tool selections are retained; changing hosted disclosure scope requires renewed consent. MCP servers must already be configured by the engine operator. The engine-managed Guide and plan coordinator are not editable through this general-agent form.

`ResourceService::edit_agent` authorizes organization management and atomically publishes an immutable definition/identity revision, selects it as the agent's default, and records an idempotent edit receipt. Schema 62 adds the selected-revision head and edit receipts to the existing registry; no duplicate agent or parallel registry is created. Stale saves are rejected. Original registration retries and startup do not undo edits. Started work remains pinned; edits are not emergency revocation of an active run's grants.

Verification: all 170 memory tests, 53 focused web tests, the application test `edits_keep_identity_preserve_old_execution_and_run_new_tools_after_reopen`, web production build, CLI build, and package engineering gate passed. Tests cover atomic rollback, authorization, old/new revisions, stale saves, idempotent retries, restart, preservation of partial grants, adding shell/MCP tools, removal of unavailable MCP selections, and hosted consent changes. The application test uses a fake inference endpoint and verifies the actual old/new request manifests. The live local UI successfully saved and reopened an existing agent without changing its permissions.

Still open: mixed-provider team dispatch, vendor-harness attachment, live frontier-provider conformance, and MCP server registration through the UI. Preserve the [audit's conformance boundaries](../october-1-coherent-workspace/agent-creation-frontier-audit-2026-10-06.md#10-required-conformance-and-release-evidence).
