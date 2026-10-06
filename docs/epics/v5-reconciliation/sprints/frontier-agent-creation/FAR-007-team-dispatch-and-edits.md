# FAR-007 — Mixed-runtime teams and versioned agent edits

Status: **in progress — persistent editing and parallel mixed-provider teams delivered for the general harness; vendor harnesses and live frontier proof remain open**. Size: L. Parent: [frontier agent sprint](plan.md).

## Work

Replace local/prompt-only dispatch assumptions with resolved execution bindings for guide, coordinator and workers. Reuse existing child work, grants, payer reservations and context handoffs. Add agent edits and deliberate default-revision selection; keep active work pinned and revoke live authority immediately.

## Acceptance

Two supported agents using different runtime profiles collaborate with real tools and scoped handoffs under one allowance and parent stop. Editing an agent preserves identity and affects later work only unless explicitly rebound. Planner readiness and launch validation agree. No independent-root fallback.

A contributor retains its selected model/provider/harness and approved ordinary/MCP tools when it receives team work. Reuse FAR-004's resolved execution/disclosure binding and derive narrower child authority; never substitute a local prompt-only agent or create an independent root to work around an unsupported binding. Include mixed-provider MCP work in the managed team conformance proof. Team membership does not disclose the agent owner's private conversations or provider continuation state.

## Evidence

October 6: Agents → select a teammate → Edit agent now uses the existing creation form and common validation. Name, purpose, provider/model, supported harness, limits, local tools (including Terminal), and individually discovered MCP tools can be edited. Existing exact tool selections are retained; changing hosted disclosure scope requires renewed consent. MCP servers must already be configured by the engine operator. The engine-managed Guide and plan coordinator are not editable through this general-agent form.

`ResourceService::edit_agent` authorizes organization management and atomically publishes an immutable definition/identity revision, selects it as the agent's default, and records an idempotent edit receipt. Schema 62 adds the selected-revision head and edit receipts to the existing registry; no duplicate agent or parallel registry is created. Stale saves are rejected. Original registration retries and startup do not undo edits. Started work remains pinned; edits are not emergency revocation of an active run's grants.

Verification: all 170 memory tests, 53 focused web tests, the application test `edits_keep_identity_preserve_old_execution_and_run_new_tools_after_reopen`, web production build, CLI build, and package engineering gate passed. Tests cover atomic rollback, authorization, old/new revisions, stale saves, idempotent retries, restart, preservation of partial grants, adding shell/MCP tools, removal of unavailable MCP selections, and hosted consent changes. The application test uses a fake inference endpoint and verifies the actual old/new request manifests. The live local UI successfully saved and reopened an existing agent without changing its permissions.

### Persistent agents and concurrent teams — October 6 follow-up

Product contract: creating an agent creates a durable identity with a saved definition. An assignment references that identity and pins a definition revision; it does not construct a replacement agent. Provider/model, supported harness, instructions, selected ordinary/MCP tools, disclosure approvals and per-run effort limits follow that agent. Calling user, work context and deployment policy can narrow effective authority; assignment cannot broaden it. Private history is not implicitly copied into a team.

Direct and team work now share `LocalWorkspace::agent_execution_settings`. Team workers resolve their pinned revision through that same path. Missing tools/connections or unavailable models produce an explicit failure rather than a silent provider/tool substitution. Coordinator revisions remain engine-managed; existing coordinator registrations accept the new orchestration instructions without replacing user agents.

An approved group schedules independent agents concurrently. Dependencies receive completed contributions, each agent's capacity is respected, occupied execution slots wait within the parent's deadline, and all children retain existing managed run, payer, allocation, lease and stop lineage. Unrelated ready work continues when another assignment fails. Repeated dispatch reuses the original work. There is no independent-root fallback. The coordinator still selects the agreed keys; deliberate one-key dispatch remains possible.

Ordinary delegated grants remain subsets of parent authority. An organization administrator can explicitly approve a child's exact execution environment, recorded on the existing immutable grant lineage. That binds model/provider, canonical workspace, selected capabilities, time/token/preparation limits and disclosure route. It permits the worker's saved tools without advertising those tools to the coordinator. Changed environments are rejected; parent revocation and loss of administrator authority invalidate the derived grant. This local-owner path is not yet a general employee self-service delegation policy.

Ollama generations with the same allocation can hold concurrent read leases. Loading, resizing, embedding and unloading retain exclusive leases. Incompatible allocations still wait, and actual model-server/hardware capacity may queue requests. This removes Tetonic's unconditional generation serialization; it does not promise GPU parallelism on every deployment.

Verification: 226 application tests passed (three existing live scenarios ignored), 171 storage tests passed, and 149 inference tests passed (two existing ignored). New tests require both workers' inference requests to arrive before either completes, cancel both active workers from the parent, accept unordered dependent selections, retain idempotent receipts and identity/configuration, and reject changed delegated environment settings. Mixed local/hosted team scenarios cover OpenAI, Anthropic and Google wire adapters with real file reads, MCP HTTP calls and owner-approved shell effects. Provider responses are controlled fixtures; no paid frontier calls were used.

The CLI build and package engineering gate passed. The local test server on port 3004 was restarted behind the existing 5177 UI after confirming no active work. Authenticated API checks confirmed all four agents' IDs, definition digests, models, harnesses, tools and per-run limits were unchanged, with all 13 existing tasks retained. No UI layout changes or publication were made.

Still open: cumulative personal spend/effort accounting across assignments (current saved limits are **per run**), installed vendor-harness attachment, durable recovery/resumption of the scheduler after process restart, live frontier-provider conformance, remote/authenticated/write MCP and MCP server registration through the UI. Preserve the [audit's conformance boundaries](../october-1-coherent-workspace/agent-creation-frontier-audit-2026-10-06.md#10-required-conformance-and-release-evidence).
