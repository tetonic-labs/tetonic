# Guide: live workspace inspection and visible operations

Date: 2026-10-09. Implemented and fixture-tested; the existing local engine process has not loaded this build.

## Product behavior

The Guide remains a conversation on the map. Short questions and brainstorming do not create execution work. Instructions now distinguish requests to accomplish an outcome from discussion: inspect the saved resources, identify relevant constraints or missing access, and save a delegation proposal instead of substituting a chat deliverable. One worker is valid for small work; useful independent assignments can run in parallel.

The Guide can inspect current agents, teams, skills, MCP inventory, usage and configured limits, list ongoing work, or read a specific work record and its bounded result. It can still inspect and revise the current conversation's proposal. The existing inline **Start this plan** action remains the transition to execution. These changes do not add autonomous launch, permission grants, agent creation, or editing of running assignments to the Guide.

Guide messages have a subtle hover/focus highlight. A compact activity line reports the actual managed operation requested by the model, with animated dots while pending and a recorded result afterward. Prior actions are available through an inline disclosure, without another modal. Failed and unconfirmed actions are not displayed as successful checks. Disconnection and terminal turns stop pending animation; reduced-motion settings are respected.

## Integration and authority

- Extend the existing internal `work_plan` capability with `resources` and `work`, alongside `inspect` and `propose`. No parallel registry, scheduler, budget ledger or external control endpoint is introduced.
- Existing `DirectorBinding` binds every operation to the active Guide turn, attempt, conversation root and observed revisions. The managed host only advertises/admit this capability when that Guide binding exists. Ordinary workers do not gain it through this change.
- Reads use `WorkService`, authorized snapshots, MCP/skill registries, approval views, execution configuration and scoped usage storage. The selected team limits the eligible roster. Installed capabilities are distinguished from grants to individual agents.
- Listings are bounded and report partial results. Oversized serialized results fail explicitly. Work detail rejects Guide discussion records; neither inventory nor work listings include other private discussion bodies. Connector secrets and endpoints are omitted from inventory results.
- Execution ceilings are token, step and time allowances. Host configuration and active assignment counts are not measured CPU/GPU capacity. Reported token usage is not monetary spend; lifetime workspace usage is not a remaining shared allowance. Connector inventory is not a live health check.
- Proposals continue through existing brief/plan validation, conflict detection, readiness checks and recorded receipts. Starting work still uses the existing pinned agent definitions, policy, budget reservations and parallel coordinator.

## Observable activity

The scoped transcript reader now exposes structured audit metadata under the same context/session authorization as text history. The existing text-only API keeps its previous return shape. The query joins recorded tool status within the same audit session and preserves rollback exclusions.

Guide activity is projected from assistant tool requests and durable tool result records. Only operation names and receipt states are added to the UI contract. Tool arguments and private reasoning are not added to the activity display. A request alone means pending, never completed. Missing receipts remain unconfirmed or interrupted. Unknown calls retain their place in serial result ordering, and a new assistant batch cannot accidentally complete a skipped call from the previous batch.

This extends the capability described in [Guide planning tools](../october-1-coherent-workspace/guide-planning-tools-2026-10-07.md). The current outer tool schema requires `operation`, `direction`, `plan` and `work_id`, with null placeholders for unused fields. Compatible older calls without the unused work ID remain accepted.

## Verification and limits

- Eight focused director tests passed, covering real proposal persistence/revision, natural answers without plans, scoped live reads, private discussion exclusion, argument/authority rejection, and receipt projection including interrupted/malformed batches.
- The hosted Guide fixture passed for OpenAI, Anthropic and Google: resource read, work listing, plan inspection, saved proposal and conversational completion. All four operation receipts were recovered as completed. No workers were launched and no real provider was called.
- Existing shaping test passed: managed exploration retains its restricted tools and versioned brief behavior.
- Scoped-memory authorization test passed for both text and structured transcript reads.
- Twenty-one focused web tests passed, including activity rendering, shaping flows and the engine client. TypeScript, production web build and frontend architecture check passed. The existing bundle-size advisory remains.
- The Tetonic CLI build passed. No local engine restart or live-model planning trial was performed for this slice. Provider fixtures verify integration, not model judgment; following the delegation instructions remains model-dependent.

The backend update requires the running engine to load the new binary. The earlier automatic approval review rejected stopping/replacing/restarting the local engine with only a generic policy-block reason; this slice did not retry that action. Existing workspace data was preserved.
