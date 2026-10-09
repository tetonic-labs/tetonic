# Tetonic product experience priorities

Date: October 8, 2026. Implementation baseline: `3294b94c`. Status: implementation started; human decision slice delivered locally.

Later October 8 owner review: the [First use to useful team work sprint](../october-first-use-repair/plan.md) is now an explicit additional layer of required product scope. Its EXP-001–009 tickets and acceptance gate supersede this candidate backlog's delivery order. Reuse overlapping completed work; do not treat all 28 candidates as new commitments or imply the corrective scope was already accounted for.

The objective is to let one person keep more useful work aligned with their intent while spending less effort configuring, supervising and recovering context. The highest immediate returns are contextual setup, understandable decisions and a continuous place to direct work. Persistent direction and dependable execution are necessary foundations for the larger autonomy promise.

This breakdown refines the existing October work; it creates no additional calendar sprint and closes no parent acceptance. Task IDs 1.1 through 7.4 refer to the seven product slices discussed with the owner. The first implementation covers the existing-request UI in 5.1 and 5.2, plus the receipt and observed-state portion of 5.4; see [decision experience evidence and limits](decision-experience-2026-10-08.md). Remaining tasks are proposed. Existing partial implementations must be reused and verified, not recounted as new delivery. Work in groups 1, 2, 4 and 5 is detailed in [the coherent workspace breakdown](product-experience-tasks-2026-10-08.md); groups 3, 6 and 7 are in [the coordinated work breakdown](../october-2-coordinated-work/product-experience-tasks-2026-10-08.md).

## Ranking method

Impact is the expected reduction in user effort or increase in useful delegated work: 5 changes a central journey, 4 removes a frequent obstacle, and 3 improves understanding or an occasional interaction. These are product hypotheses, not measured benefits.

Effort includes design, implementation, integration and validation. Relative points: 1 is very local, 2 is a small integrated change, 3 spans components or an API, 5 crosses durable engine and product boundaries, and 8 changes runtime lifecycle semantics with substantial uncertainty. Points are not days or a calendar capacity estimate.

The ROI index is impact divided by effort, rounded to one decimal. It measures expected incremental return after dependencies exist; it is not financial ROI. Cheap dependent tasks do not become cheap complete projects when their prerequisites are missing. Ties have no meaningful precision. Delivery order therefore differs from the raw ranking. Scope, privacy, tool authority, stop semantics and truthful state are requirements, not optional improvements traded against ROI.

The highest estimation uncertainty is in active redirection (3.3) and durable team parking (6.3). Start each with a bounded runtime proof and re-estimate before broad implementation. Model-dependent routing and exploration (2.3, 4.1, 4.2) require live usefulness trials; passing deterministic tests does not establish quality.

## Ranked task inventory

| Rank | Task | Impact | Effort | ROI index | Prerequisites |
|---|---|---:|---:|---:|---|
| 1 | [1.3 Preserve context during setup detours](product-experience-tasks-2026-10-08.md#task-13) | 5 | 2 | 2.5 | Existing baseline |
| 2 | [5.1 Present decisions in human terms](product-experience-tasks-2026-10-08.md#task-51) | 5 | 2 | 2.5 | Existing baseline |
| 3 | [2.2 Make current direction readable and editable](product-experience-tasks-2026-10-08.md#task-22) | 4 | 2 | 2.0 | 2.1 |
| 4 | [3.2 Apply queued changes from the effort itself](../october-2-coordinated-work/product-experience-tasks-2026-10-08.md#task-32) | 4 | 2 | 2.0 | 3.1, 2.3 |
| 5 | [5.4 Acknowledge decisions and show the next state](product-experience-tasks-2026-10-08.md#task-54) | 4 | 2 | 2.0 | 5.1, 5.2 |
| 6 | [1.2 Keep work and conversation available together](product-experience-tasks-2026-10-08.md#task-12) | 5 | 3 | 1.7 | 1.1 |
| 7 | [2.4 Deliver scoped direction to each assignment](product-experience-tasks-2026-10-08.md#task-24) | 5 | 3 | 1.7 | 2.1 |
| 8 | [3.1 Preview which work a direction change affects](../october-2-coordinated-work/product-experience-tasks-2026-10-08.md#task-31) | 5 | 3 | 1.7 | 2.1, 2.4 |
| 9 | [3.4 Show whether direction actually reached the team](../october-2-coordinated-work/product-experience-tasks-2026-10-08.md#task-34) | 5 | 3 | 1.7 | 3.1, 3.2 |
| 10 | [4.4 Resolve missing capabilities without abandoning the work](product-experience-tasks-2026-10-08.md#task-44) | 5 | 3 | 1.7 | 1.3, 5.2 |
| 11 | [5.2 Resolve the same request in the effort or inbox](product-experience-tasks-2026-10-08.md#task-52) | 5 | 3 | 1.7 | 5.1 |
| 12 | [6.1 Make the scope of autonomous continuation clear](../october-2-coordinated-work/product-experience-tasks-2026-10-08.md#task-61) | 5 | 3 | 1.7 | 2.1 |
| 13 | [7.1 Show meaningful progress on the map](../october-2-coordinated-work/product-experience-tasks-2026-10-08.md#task-71) | 5 | 3 | 1.7 | 1.1 |
| 14 | [7.4 Make activity signals trustworthy and restrained](../october-2-coordinated-work/product-experience-tasks-2026-10-08.md#task-74) | 3 | 2 | 1.5 | Existing baseline |
| 15 | [1.4 Give results a proper reading space](product-experience-tasks-2026-10-08.md#task-14) | 4 | 3 | 1.3 | 1.1, 1.2 |
| 16 | [4.1 Handle simple requests without creating a planning ceremony](product-experience-tasks-2026-10-08.md#task-41) | 4 | 3 | 1.3 | Existing baseline |
| 17 | [4.3 Make exploration results into understandable choices](product-experience-tasks-2026-10-08.md#task-43) | 4 | 3 | 1.3 | 4.2 |
| 18 | [7.2 Show changes since the user last looked](../october-2-coordinated-work/product-experience-tasks-2026-10-08.md#task-72) | 4 | 3 | 1.3 | 1.1, 7.1 |
| 19 | [7.3 Organize related efforts at scale](../october-2-coordinated-work/product-experience-tasks-2026-10-08.md#task-73) | 4 | 3 | 1.3 | 1.1, 7.1 |
| 20 | [1.1 Keep one identity for an effort](product-experience-tasks-2026-10-08.md#task-11) | 5 | 5 | 1.0 | Existing baseline |
| 21 | [2.1 Extend the versioned brief with structured direction](product-experience-tasks-2026-10-08.md#task-21) | 5 | 5 | 1.0 | 1.1 |
| 22 | [2.3 Give the Guide explicit direction operations](product-experience-tasks-2026-10-08.md#task-23) | 5 | 5 | 1.0 | 2.1 |
| 23 | [4.2 Run bounded discovery before committing to implementation](product-experience-tasks-2026-10-08.md#task-42) | 5 | 5 | 1.0 | 2.1, 2.4, 4.1, 6.1 |
| 24 | [6.2 Persist eligible work and fair dispatch](../october-2-coordinated-work/product-experience-tasks-2026-10-08.md#task-62) | 5 | 5 | 1.0 | 6.1 |
| 25 | [6.4 Control several efforts without controlling every agent](../october-2-coordinated-work/product-experience-tasks-2026-10-08.md#task-64) | 5 | 5 | 1.0 | 1.1, 6.2 |
| 26 | [5.3 Group related requests without merging authority](product-experience-tasks-2026-10-08.md#task-53) | 4 | 5 | 0.8 | 5.2 |
| 27 | [3.3 Deliver direction to supported running agents](../october-2-coordinated-work/product-experience-tasks-2026-10-08.md#task-33) | 5 | 8 | 0.6 | 3.1, 2.4 |
| 28 | [6.3 Park and resume supported waiting team work](../october-2-coordinated-work/product-experience-tasks-2026-10-08.md#task-63) | 5 | 8 | 0.6 | 6.2 |

## Delivery order

Use these as dependency-aware increments rather than a new set of calendar sprints.

1. **Improve immediate decisions and preserve context.** Complete 1.3 and 5.1, then 5.2 and 5.4. These improve current workflows without waiting for new autonomy machinery.
2. **Establish the continuous effort.** Complete 1.1, then 1.2 and 7.1. Prototype and verify scope selection, persistent input and reading space together before broad layout changes. Take 1.4 next when result reading is the dominant remaining friction.
3. **Keep direction coherent.** Complete 2.1, 2.2 and 2.4, followed by 2.3, 3.1, 3.2 and the queued portion of 3.4. Deliver the direction contract and its visible representation together.
4. **Extend usable autonomy.** Complete 6.1 and 6.2 and begin the bounded 6.3 proof. Pair this with 4.1, 4.2 and 4.3 for evidence-based exploration. These paths share the same governed execution and allowance, not separate agent loops.
5. **Remove setup and return friction.** Complete 4.4 once decision context is available, then 7.2 and 7.3. The larger related-request grouping in 5.3 follows explicit blocker relationships.
6. **Broaden supported controls carefully.** Finish 3.3 and its receipts only for proven runtime boundaries. Complete 6.4 against real controller operations, and expose pause/resume only after 6.3. Fit 7.4 alongside map changes; do not let animation polish delay execution correctness.

Start the high-risk runtime investigation early while interface delivery continues; do not postpone discovery of hard lifecycle problems until the end of the schedule.

For the October 25 goal, treat this as a candidate backlog, not a promise to finish all 28 tasks. Protect a complete, truthful journey. Rich grouping, optional view filters and animation refinements can defer before dropping required execution controls. If active redirection or durable parking cannot be proven, show the narrower supported queued-change or stop/replan behavior and revise the release claim; do not simulate support. Existing required acceptance is not waived by this ranking.

## Placement and interaction rules

- Keep the map as the primary overview. Do not add a second home dashboard or replace the brand.
- Keep a compact, explicitly scoped way to start or steer work accessible while inspecting it. A message must never silently switch recipients because selection changed.
- Use an anchored peek for a short status check; an in-place editor for a small change; a spacious focus area for sustained reading, comparison or configuration; and a true dialog for a bounded decision that needs attention. Avoid nested dialogs.
- The main effort area shows the most relevant current content without imposing phase tabs. Automatic updates must not move reading position, reorder a focused decision or steal keyboard focus.
- Place the primary action beside the evidence needed to take it. Do not move Start above all meaningful context simply to reduce scrolling.
- Separate workspace installation, persistent agent grants, effort authorization and individual action approval. Label the scope of a change before saving it.
- Preserve drafts and position through detours; make saving, canceling, conflict and uncertain outcomes legible. A successful save is distinct from resumed execution.
- Use group identity separately from status color. Pair color and motion with text or icons. Preserve keyboard, reduced-motion and narrow-screen behavior.
- Keep the detailed blackboard and run evidence accessible without making them the default way to understand work.
- Treat sample data only as a layout and state fixture. Plans, names, contributors and domain content remain model/user authored. Product usefulness requires actual work.

## Shared completion standard

Every task includes implementation, a visible interaction and a checkable result. Its delivery evidence must show the relevant normal path, empty state, loading or stale state, error/conflict, and recovery behavior. Apply only the states reachable by the change.

For interface tasks, inspect the actual rendered interaction at the supported desktop and narrow widths. Verify reading order, primary-action placement, overflow, focus return and no lost draft. Avoid adding mirrored tests for purely cosmetic rules; use meaningful workflow regressions for saved state, scope, permissions, uncertain responses and navigation.

For engine tasks, test the boundaries they change: concurrent updates, idempotency, cancellation, restart, scope, budgets and unknown external effects as applicable. Pair each engine increment with the UI state that tells the user the truth. A green test suite does not replace an operator walkthrough.

## Product acceptance scenario

Run a controlled scenario and then a small real-model scenario within explicit tool and resource limits:

1. Begin with an uncertain goal; develop direction without completing a setup questionnaire.
2. Run two useful independent assignments through the agreed execution path.
3. Start a second effort while the first continues.
4. Resolve a genuine missing capability or human question without losing the original effort.
5. Change a constraint and observe exactly which work received it.
6. Leave and return to identify meaningful changes and any required judgment.
7. Read the delivered result and trace it to actual contributions.

Record repeated explanations, required navigation detours, interventions needed to keep agreed work moving, mistaken interpretations of state, time to recover context and whether the result was usable. Capture a baseline before the changes; compare with a solo path for work where team overhead may outweigh its benefit. Do not promise numeric improvement before measurement. One owner is an initial test case, not evidence for all users.

## Current integration anchors

- [TeamWorkspace](../../../../../web/src/components/team-work/TeamWorkspace.tsx) owns the current map, inspector, composer and return trail.
- [Work journeys](../../../../../web/src/lib/workJourneys.ts) currently groups launched work and suppresses its source from the list; use existing lineage to extend continuity.
- [Work briefs](../../../../../engine/strata/tetonic-memory/src/control/work_briefs.rs) already stores append-only versions on existing team work.
- [Plan direction store](../../../../../engine/strata/tetonic-memory/src/control/plan_human.rs) checks every affected assignment before accepting upcoming changes.
- [TeamWorkController](../../../../../engine/litho/tetonic-app/src/team_work_controller.rs) owns dispatch policy over existing journals and admission.
- [Attention projection](../../../../../web/src/lib/attentionItems.ts) and [decision component](../../../../../web/src/components/team-work/Decision.tsx) provide current request identities and exact approval behavior.
- [Latest handoff slice](delegation-experience-2026-10-08.md) already improves proposal placement, setup return and current activity. The remaining tasks extend that work rather than replacing it.
- [Runtime integration boundaries](../october-2-coordinated-work/runtime-boundaries-and-durable-waits-2026-10-07.md) records partial durable-wait support; it must not be represented as finished team parking.
