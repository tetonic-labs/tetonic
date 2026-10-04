# October sprint 3 Release confidence

Dates: October 18 to 24, 2026. Status: planned. Release decision: October 25. Depends on [sprint 1](../october-1-coherent-workspace/plan.md) and [sprint 2](../october-2-coordinated-work/plan.md) P0 exits. Follow the [shared scope and priority rules](../README.md).

User outcome: a person can install the supported profile, understand what to do, leave useful work running, and recover from ordinary failures without a developer operating the system for them. Feature freeze starts October 18; October 21 to 24 is reserved for release-candidate verification and fixes.

## Ticket overview

All tickets below are planned. Packaging and pilot preparation should begin earlier once sprint 1 freezes their contracts; this sprint owns their acceptance.

| Ticket | Work | Priority | Size | Depends on |
|---|---|---|---|---|
| OCT-301 | Package the supported installation and operator controls | P0 | L | OCT-101, OCT-102, OCT-103 |
| OCT-302 | Prove failure recovery privacy and control boundaries | P0 | L | Sprint 1 and 2 P0 exits |
| OCT-303 | Test comprehension and coordination burden with new users | P0 | M | OCT-104, OCT-202, OCT-205, OCT-301 |
| OCT-304 | Run cross-domain trials and a measured multi-day soak | P0 | M | OCT-201, OCT-203, OCT-204, OCT-301 |
| OCT-305 | Remove proven obsolete paths and update developer guidance | P1 | M | Replacement evidence from sprints 1 and 2 |
| OCT-306 | Assemble release evidence and make the release decision | P0 | S | OCT-301, OCT-302, OCT-303, OCT-304 |
| OCT-307 | Apply small usability refinements from the pilots | P2 | S | OCT-303 |

## OCT-301 Package the supported installation and operator controls

Work: deliver one installation package for the selected OS with preflight, versioned configuration and a reliable start/stop procedure. Serve the built product UI without requiring users to run Vite or a Rust development toolchain. Check provider readiness and explain missing prerequisites. Expose supported logging level/destination/rotation, telemetry endpoint/disable/redaction, storage/artifact location and backup configuration. Keep credentials in the existing secure store. Include migration, graceful shutdown, restore and diagnostic instructions.

Reuse: `tetonic` CLI, local UI service, existing EngineConfig validation/redaction, tracing/telemetry, memory migrations/backup and build scripts. Build on the existing daemon/host lifecycle appropriate to the selected profile; do not introduce a competing application server. Developer launch scripts remain useful but do not constitute the release installer.

Acceptance: install into a clean environment without this checkout or its dependencies; connect a provider and complete useful work. Logs, traces and storage honor configuration and omit secrets. A telemetry exporter outage does not halt execution or cause an unbounded buffer. Unsupported backends/listen profiles fail clearly. Upgrade a supported test database without deleting history; restore a backup into a separate profile and verify real work and artifact access. Document any required reauthorization and how restored work avoids replaying uncertain effects.

## OCT-302 Prove failure recovery privacy and control boundaries

Work: run focused deterministic integration checks through the shipping path, including UI-to-API boundaries, with actual supported tools. Inject faults at accepted-work, dispatch, tool-effect, approval and recovery boundaries. Fix violations before release rather than documenting them as ordinary limitations.

Reuse: existing resource/privacy tests, managed service cancellation/ownership tests, memory migrations and human-control tests, frontend integration tests and architecture gates. Extend these contracts instead of writing tests that only mirror component markup.

Acceptance evidence must cover:

- Lost create/approval replies and retries: one accepted operation, with no premature success or repeated effect.
- Browser disconnect and engine crash: accepted work retained, one execution owner after recovery, truthful interrupted/uncertain state.
- Parent pause/cancel/emergency stop: applicable descendants and cancelable tools stop; queued work cannot bypass the control; external irreversible effects remain disclosed.
- Approval expiry, digest mismatch and restart: authorization stays bound to the exact effect and current authority.
- Private/team retrieval, delegated context, filesystem scope/escape and credentials: unauthorized reads, writes and disclosures are denied.
- Concurrent child admission and proxy delegation: no budget reset, double reservation or privilege expansion.
- Storage full/unavailable and telemetry failure: no fabricated durable success, no silent temporary-store fallback, bounded failure behavior.
- Resume and restore: authority is rechecked and unknown-effect actions are not blindly repeated.

Record observed cancellation and recovery latency under declared conditions; do not promise immediate cancellation of an external action that cannot be interrupted. Security tests remain deterministic; a cooperative model is not evidence that a boundary holds.

## OCT-303 Test comprehension and coordination burden with new users

Work: conduct three to five fresh-user sessions using the release candidate. Ask users to begin useful work, add a different responsibility, return after progress, inspect evidence and resolve a consequential decision. Do not provide a tutorial before observing the first attempt. Compare comparable solo and team work where practical, recording task/model differences and uncertainty. The sample is formative, not a statistically powered market study.

Reuse: the existing map/composer/inspector journey and scenario fixtures containing real input documents/repositories. Use an observation sheet rather than adding a new analytics subsystem.

Acceptance: save actual observations, confusion points, assistance required, time to first useful outcome, human interventions and supervision time per accepted result. Evaluate the proposed 30-second next-action and 60-second return-comprehension targets. Users must be able to understand approval scope and successful versus uncertain outcomes. Fix blocking confusion before release; record unmet softer targets and a follow-up decision honestly. If pilot participants are unavailable, mark the evidence missing rather than substituting an author walkthrough.

## OCT-304 Run cross domain trials and a measured multi day soak

Work: exercise research over supplied material, a bounded repository investigation/change, and recurring document review through the same product path using actual model calls. Run at least 48 hours of bounded operation, starting no later than October 21, including browser absence, scheduled occurrences, provider interruption and a controlled engine restart. Ensure credentials, costs and selected limits are set before the run; this ticket does not authorize unbounded background spending.

Reuse: existing logging, telemetry, work inspection, journals and activation cursors. Collect results from the runtime rather than synthetic UI counters.

Acceptance: publish hardware, OS, model/provider, tool capabilities, actual concurrency, queue behavior, task outcomes, retries, intervention counts, resource usage, cancellation and recovery observations. Begin with the proposed envelope of three work items and two cooperating agents; adjust the supported envelope to measurements. Verify no duplicate accepted recurring occurrences, orphaned children, forgotten approvals, uncontrolled catch-up or growing idle-model loop. Actual workload outputs and limitations must be reviewable. A failed gate followed by a fix requires rerunning the affected scenario and extending the soak when the failure could recur over time.

## OCT-305 Remove proven obsolete paths and update developer guidance

Work: trace callers for implementations replaced during these sprints. Remove duplicate preview orchestration, obsolete mutation adapters and unused product routes after the supported journey passes. Keep useful fixtures confined to tests or explicit development previews. Update local UI contracts and operator/developer instructions so they match the release. Preserve historical data and intentional compatibility entry points with documented reasons.

Reuse: the V5 retirement register, architecture checks and replacement evidence. Do not start a package-wide rename, blanket deletion or a separate retirement program.

Acceptance: removal commits identify the replacement and repeat relevant caller/build checks. There is one authority for work, approval, identity and budgets on the release path. Any reachable enforcement bypass or false-state fallback remains a P0 blocker under OCT-302 even if optional dead-code cleanup is deferred. Unrelated unfinished work from other contributors is preserved.

## OCT-306 Assemble release evidence and make the release decision

Work: compile a compact release checklist linking each P0 ticket to its actual evidence. Publish the supported install/provider/tool/concurrency profile, release notes, known limitations, config examples and recovery instructions. Record deferred P1/P2 work and broader V5 obligations without calling them complete. Select the release candidate revision only after reviewed commits include all required source and artifacts; exclude incidental local logs, databases and credentials.

Reuse: epic progress, existing product/system contracts and the evidence produced in these sprint folders. Adapt the standalone portions of MVP-701 and applicable MVP-702 gates; retain production-HA requirements for the later milestone.

Acceptance: all 15 P0 tickets across the three sprints have current evidence and no unresolved critical privacy, permission, data-loss, duplicate-effect or control failure. Confirm installation and smoke checks against the packaged candidate, not only the development server. On October 25, record go/no-go with an explicit supported scope. Any scope reduction is visible and preserves safety requirements. Publication or a wider rollout is a separate release action; this planning ticket does not silently deploy anything.

## OCT-307 Apply small usability refinements from the pilots

Work: take only small, evidence-backed refinements such as clearer wording, focus restoration or compact empty-state help. No additional views, providers, connectors, agent roles or redesign after the freeze. Fixes needed to complete or safely understand the core journey belong to P0 OCT-303, not this stretch ticket.

Reuse: existing components and Tetonic styles.

Acceptance: each change names an observed problem, preserves keyboard/reduced-motion behavior and passes the affected regression check. Defer changes that jeopardize candidate validation time.

## Sprint exit and release evidence

By October 24, have a packaged candidate, clean-install record, current boundary/recovery checks, actual user observations and completed multi-day evidence. Keep the evidence index and final release decision in this folder, linking rather than duplicating prior sprint evidence. The October 25 label is a limited single-owner, single-machine MVP preview; it does not certify shared human use, distributed execution, production HA or arbitrary harness isolation.

If a P0 gate is incomplete, record it and revise the release claim or date explicitly. Passing older local-preview tests, a visually polished map, or successful simulated work cannot substitute for the required evidence.
