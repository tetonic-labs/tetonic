# October sprint 2 Coordinated work

Dates: October 11 to 17, 2026. Status: planned, sequencing revised October 4. Follows the [Sprint 1](../october-1-coherent-workspace/plan.md) P0 and COORD-A/B/C exits. The first slices of OCT-201/202/205 now start there; this sprint retains ownership of their complete acceptance and broadens the demonstrated team behavior. Follow the [shared scope and priority rules](../README.md). Adding work must not require the human to manage each handoff.

User outcome: several different responsibilities progress, a small team performs useful collaboration, and one recurring responsibility continues while the user is away. Work awaiting human judgment remains visible without stopping unrelated work.

## Ticket overview

All full-ticket exits below remain unverified. COORD-A/B/C have partial implementation in Sprint 1: governed child execution, finite agreed-plan dispatch, scoped contributions and actual work projections. See [October 5 plan execution evidence](../october-1-coherent-workspace/plan-execution-evidence-2026-10-05.md). These are not extra tickets or full gate completions. Size reflects breadth and uncertainty, not calendar days.

| Ticket | Work | Priority | Size | Depends on |
|---|---|---|---|---|
| OCT-201 | Enforce delegation lineage and shared limits | P0 | L | OCT-102 execution; OCT-103 identity contract; OCT-105 single-run control contract |
| OCT-202 | Connect real huddles collaboration and combined results | P0 | L | OCT-201 |
| OCT-203 | Keep independent work moving and conflicts visible | P0 | M | OCT-103, OCT-201 |
| OCT-204 | Run one durable bounded recurrence | P0 | M | OCT-201, OCT-203 |
| OCT-205 | Drive map activity and return summaries from evidence | P0 | M | OCT-104, OCT-202, OCT-203, OCT-204 |
| OCT-206 | Improve result comparison and evidence navigation | P1 | M | OCT-105, OCT-202 |
| OCT-207 | Refine event-driven docking motion | P2 | S | OCT-205 |

## OCT-201 Enforce delegation lineage and shared limits

October 5 partial implementation: the [COORD-A execution slice](../october-1-coherent-workspace/child-execution-evidence-2026-10-05.md)
connects funded child tasks to the existing managed run and provider path. Wider
fault coverage and the complete acceptance below remain open.

**Sequencing:** COORD-A moves the smallest governed child-execution path into Sprint 1, including every safeguard reachable by that slice. This ticket retains full ownership of allocation races, proxy resistance, private context, stop and restart behavior as the team/concurrency envelope expands. Do not launch unrelated root runs to sidestep the current governed-child guard.

Work: finish governed child admission through the existing managed lifecycle before exposing agent delegation. Persist parent work/run/attempt relationships and inherit scope, context grants, limits and stops. Reserve and reconcile the selected token/task/concurrency allocations atomically so concurrent children cannot each spend the same remaining allowance. Bound retries, depth and task count. Model-proposed role or membership changes cannot grant authority. An unavailable enforcement service denies new admission rather than resetting the allowance.

Reuse: `Application::activate_team_work`, `ResourceService`, `ManagedRunService`, broker admission/budget logic, `tetonic-memory` team-work and human-control records. Prior work already stores delegation ceilings, but its recorded local-preview exit explicitly left managed child admission and cumulative accounting unfinished. Adapt the existing owner rather than creating another coordinator or ledger.

Acceptance: two children racing for one remaining allocation cannot both obtain it; retrying admission is idempotent. A child's request to another agent preserves the originating work's budget and scope. Private unpublished material stays unavailable across delegation. Parent stop reaches admitted descendants and active supported tools; no child starts after a confirmed stop. Restart retains reservations and lineage without double spending. Unknown usage stays visible and cannot silently restore budget. No financial cap is advertised without corresponding cumulative enforcement.

## OCT-202 Connect real huddles collaboration and combined results

**Sequencing:** COORD-B in Sprint 1 proves two real contributors, scoped durable exchange, bounded coordination and a human flag. Reuse its protocol and receipts; do not add a parallel agent-chat coordinator here. The [bounded collaboration contract](../october-1-coherent-workspace/ui-consolidation-sprint.md#bounded-collaboration-and-interruption) covers update/help/urgent/human signals, limits, cycles, deadlines and safe attention boundaries. This sprint adds wider usefulness/failure trials, huddle revision and the repository scenario moved from Sprint 1.

Work: let an agent propose a concise task-specific huddle for larger requests, including intended outcome, contributors, dependencies and limits. Small requests can proceed directly. Dispatch accepted assignments through the same durable work API. Allow permitted exchanges of task context and artifacts; integrate contributions into a result with traceable sources. Support human steering without restarting an unrelated conversation or discarding completed work. Begin with an explicit small team; arbitrary recursive agent creation is unnecessary.

Reuse: existing goal/huddle/delegation resources and the general harness. Salvage useful presentation from `GuideIntake` and `DirectorExperimentView`; remove fixed discovery/analysis/implementation tracks, asynchronous response assumptions, timed reviews and fallback findings. Reuse team participation contexts for scoped collaboration.

Acceptance: two actual agents contribute different useful pieces to an input-specific assignment and produce a combined result linked to their work. A changed input changes the proposed breakdown where appropriate; no test requires one exact model plan. Failure of a contributor produces an honest blocked, partial or revised outcome. A human can adjust the goal and see which work is retained, superseded or still in flight. Accepting a huddle twice does not duplicate assignments. Demonstrate both research and repository work without domain-specific branches in the engine.

Extended acceptance: duplicate/lost exchange replies, unavailable recipients, competing help requests and repeated urgent messages produce bounded accepted/deferred/declined/expired outcomes. Root-work limits survive retries, proxying and restart. Explicit dependency cycles are rejected; stalled exchanges escalate once instead of spinning. Other eligible work receives execution time. Human emergency controls are independent of collaboration throttles. Measure useful contributions and manual coordination avoided, not message volume alone.

## OCT-203 Keep independent work moving and conflicts visible

October 7 engine prerequisite: [runtime boundaries and durable waiting](runtime-boundaries-and-durable-waits-2026-10-07.md)
adds checkpoint-backed suspension and exclusive restoration for a scoped root
through the existing managed lifecycle. Production team waiting is **not enabled**.
The follow-on adds work-scoped delegated grants, lease-fenced child admission,
and lease checks at model/tool boundaries. New plan assignments select that
contract; existing grants retain their original lifetime. A second follow-on
now shares registered harness preparation/assembly with explicit reconstruction,
preserves scoped audit history, checks tool-host checkpoint readiness, and
transfers the original usage reservation inside a journaled resume. Its restart
proof uses a controlled answer hook, not the production human-question path.
Durable question/answer integration, authorization horizons, the work controller
and supported subtree restoration remain;
OCT-203 and its product acceptance stay open. See the same evidence document's
follow-on sections for the precise boundary.

October 5 partial prerequisite: [human handoff evidence](../october-1-coherent-workspace/human-handoff-evidence-2026-10-05.md)
proves a live child can await an answer without model polling while another
eligible agent continues. The wait still owns its admission slot and deadline.
It is not durable parking, general queue fairness or resource-conflict control;
those requirements below remain open.

Work: connect queue, park, resume and reprioritization operations to engine authority. Apply bounded scheduling and avoid starvation. Work blocked on a human or dependency releases usable capacity where safe. Preserve durable ownership and dependencies. For concurrent writes in the supported local workspace, use existing resource claims or a conservative serialized write boundary; do not rely on agent courtesy to prevent clobbering.

Reuse: existing team-work state, activation ownership, execution grants and managed admission. The browser projects this state; it must not run a separate scheduling loop.

Acceptance: run the proposed three-work-item scenario with one approval wait, one active item and one queued item. Unrelated eligible work proceeds; reprioritization takes effect without creating another copy. Paused work does not resume merely because the page reloads. Two tasks targeting the same declared resource wait or expose a conflict before conflicting effects occur. The supported overlap guarantee is documented without promising semantic detection of all related work.

October 7 human-question prerequisite: explicitly configured independent registered roots now use the production persisted question/answer hook with checkpoint verification, a response deadline separate from active execution, original grant expiry, stop generations and offline answer storage. Shared reconstruction retains the original conversation/audit/budget. Default team waits remain live-only; the controller/subtree cutover and the three-item acceptance above are still required. See the [runtime integration record](runtime-boundaries-and-durable-waits-2026-10-07.md#persisted-human-question-follow-on).

October 7 controller prerequisite: plan launch now uses the extracted application `TeamWorkController`, with a narrow local host adapter and a transactional engine progress reader. Dependency readiness no longer relies on UI projections, and agent occupancy comes from existing child-admission records. The previous local scheduling implementation is removed. Durable pending dispatch and subtree recovery remain open; see the [controller integration record](runtime-boundaries-and-durable-waits-2026-10-07.md#team-controller-extraction-follow-on).

## OCT-204 Run one durable bounded recurrence

Work: expose a minimal interval-based ongoing responsibility with an understandable cadence, next activation, limits and pause control. Trigger activation from the engine, not a browser timer or continuous model polling. Reuse durable activation cursors and prevent overlapping occurrences by default. Coalesce missed intervals into at most one catch-up occurrence, with a documented maximum backlog. Persist occurrence identity and commit cursor/work creation consistently so crashes cannot silently lose or multiply accepted occurrences.

Reuse: the existing event/schedule cursor, team-work storage and managed activation path. Add the missing engine driver or API adapter only where needed; do not start another scheduler authority or general workflow system.

Acceptance: with the browser closed, two scheduled occurrences perform real work against changed documents. A no-change occurrence finishes truthfully. Duplicate delivery, crash between scheduling and activation, engine restart and a long offline period do not create unbounded or duplicate accepted work. Pause suppresses new occurrences; resume shows the next activation and rechecks authority. Failure or budget exhaustion escalates instead of spinning. External effects with uncertain completion are reconciled rather than blindly replayed.

## OCT-205 Drive map activity and return summaries from evidence

**Sequencing:** COORD-C and UI-004 supply one real team's work/agent/resource projection in Sprint 1. Keep that identity model and reader. This ticket completes cross-responsibility summaries, missed-event reconciliation and recurrence visibility after OCT-203/204; those later dependencies do not block the first real interaction view.

Work: connect agent presence, destinations, docking, tool badges and detachment to authoritative run/tool events. At a distance show purposes and relevant work groups; selection reveals contributors, resources and evidence. On return, present a concise account of changes since the user's last view, with links to the underlying records. Include partial results and items needing judgment. Begin with deterministic event summaries; optional model wording must remain grounded in those records.

Reuse: `TeamActivityMap` zoom/focus behavior, the main inspector, run inspection and existing event cursors. Replace sample tracks in `useOrganizationActivity`, preset timers in `MissionDeck`, and canned normality in `DigestSummary` on the release path. Keep fixture data only in explicitly labeled development/test surfaces.

Acceptance: replay a known sequence of real events and verify visible state changes correspond to it. A tool icon and destination appear only when supported by evidence. Finishing, failing or losing connection ends or marks the associated activity correctly. Reconnect reconciles missed events without duplicate notifications or false movement. The same work identity is selected from map, summary and conversation. Keyboard navigation and reduced motion retain the same information. Fresh users can inspect what changed without reading raw logs.

## OCT-206 Improve result comparison and evidence navigation

Work: make useful result revisions, source excerpts and diffs easier to compare in the existing inspector. Show which contributor produced an artifact and which checks were actually run. Keep private sources permission-filtered.

Reuse: existing artifact/run inspection, `FormattedMarkdown`, lineage/evidence components and the result presentation from sprint 1. A full document suite is out of scope.

Acceptance: a person can navigate from a combined result to its contributing artifact and distinguish superseded from current evidence. Missing evidence is explicit. Deferring this ticket leaves basic inspectable artifacts and truthful validation states intact.

## OCT-207 Refine event driven docking motion

Work: refine approach, magnetic capture, settling and release for actual interaction events. Keep motion quick, readable and restrained; standing motion must not suggest unobserved activity. Preserve stable camera and focus while work updates.

Reuse: existing map motion and reduced-motion preferences. No replacement renderer or visual-theme rewrite.

Acceptance: interaction changes are understandable without prose, stationary states remain legible, and reduced motion preserves status. Cosmetic animation never invents network/tool events or blocks control interactions. Defer this ticket before reducing fault-testing time.

## Sprint exit and evidence

By October 17, demonstrate three independent work items with a real two-agent collaboration and a bounded recurring responsibility. One waits for approval while another completes. Change intent, exhaust a shared allowance, deny a cross-scope read, stop the work tree and restart it into the documented recovery state. Show corresponding truthful map and return-summary behavior.

Keep deterministic tests for races, budgets, context boundaries and stop propagation separate from real-model usefulness trials. Record intervention counts and supervision time without assuming multiple agents outperform one. Begin the multi-day release soak as soon as the stable path exists. If P0 fails, defer optional work and record an explicit release decision; simulated coordination cannot satisfy this exit. Store evidence and any ticket splits in this sprint folder.
