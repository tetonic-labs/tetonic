# Refine the experience of directing work

Date: October 13, 2026. Status: **revised product plan; not implemented or validated**.
Source baseline: `55b77acf`; current map and Guide remain the design foundation.

## Current delivery direction

The dedicated [Guide partner sprint](../guide-partner/plan.md) now supplies the
active Guide sequence, including conversation continuity, multiple subjects and
bounded event attention. It maps the preceding GDE outcomes into GPR tickets.
The broader journey obligations below remain open; this does not authorize a
new layout or establish release readiness.

The owner subsequently locked the current MVP design and requested Guide
improvements, then rejected the engineering-led ticket breakdown. The
[revised Guide proposal](guide-system-refinement-2026-10-13.md) now organizes
GDE-001–006 around useful help, clearer thinking, delegation, supervision,
course corrections and results. Its first slice improves conversation and idea
development; architecture work supports those outcomes. Proof is included in every
slice. The broader JRN obligations below remain open; their original implementation
order is superseded for this proposed workstream. Layout exploration is deferred.
There is one integrated Guide, not a second system or duplicate completion claim.

## Correction to the previous plan

The owner rejected the preceding CONT-001–008 plan because it prioritized engine
components instead of the end-to-end product experience. This document replaces
that delivery order and those proposed tickets. The work below is organized around
how a person uses Tetonic. Technical fixes support the relevant journey; they do
not determine the product's navigation or require the person to understand engine
concepts.

This refinement lives within the existing first-use repair sprint and maps to its
EXP/OCT obligations. It adds no calendar sprint, changes no release date and closes
no existing acceptance gate. Earlier control, budget and reliability requirements
remain open. The proposed DAG/Tasks switch is deferred under the current design lock.

## The experience we are refining

**I can think here, hand over work when ready, keep several things moving, change
my mind, and come back to something I can understand and use.**

The following are moments in a flexible journey, not a wizard or mandatory modes:

| Moment | The person's question | What Tetonic should provide |
|---|---|---|
| Arrive | Where do I begin, or where did I leave off? | A recognizable workspace with an obvious way to express intent and a concise account of existing work |
| Think | What do I actually want? | A comfortable conversation with an editable understanding of emerging ideas, decisions and uncertainties |
| Hand off | What am I agreeing to, and what happens next? | One clear proposal and a direct transition into visible execution |
| Oversee | What is happening while I think about other things? | Understandable efforts, progress and exceptions without inspecting every agent |
| Intervene | How do I change this, unblock it or stop it? | Contextual actions and a clear account of what the change affects |
| Finish or return | What did I get, can I trust it, and what changed? | Usable results, accessible evidence, contextual correction and easy reorientation |

A short question may finish in the conversation. A clear request can move quickly
to handoff. A vague idea can stay exploratory. Work may branch, pause, change or
end without progressing through every moment. Switching attention is not an
instruction to stop, and thinking aloud is not permission to dispatch.

## Surface responsibilities to test

Keep the current visual identity and map. Refine the relationship between existing
surfaces before adding another place the person must learn.

- **Map:** where my ongoing efforts live; purpose, activity, progress and exceptions
  should be understandable at the relevant viewing distance.
- **Guide:** where I think, ask about work and express direction. Keep its expansion
  from the existing composer. Its current target must be legible when I refer to
  an effort, so a new idea cannot accidentally redirect an unrelated team.
- **Selected effort / inspector:** where I understand that effort and act on it,
  including results and requests. Keep enough surrounding context to retain my place.
- **Settings and capability editors:** where I deliberately configure the system.
  If setup is necessary mid-journey, return to the original context and draft.
- **Blackboard and detailed activity:** optional depth for investigating collaboration
  and execution. Ordinary supervision should not require reading these feeds.

Treat these as interaction responsibilities, not a specification for extra panels.
The interface need not show all of them simultaneously. Review each existing modal
by asking whether its function belongs beside the relevant work instead.

## Concrete product tickets

### JRN-01 — Walk the entire experience and remove navigation loops

**Broader journey baseline; capture Guide friction within each relevant GDE slice.** Trace the actual paths for a new arrival, a quick question,
uncertain brainstorming, a substantial handoff, parallel efforts, an interruption,
an unexpected result and a return after time away.

**Deliverables**
- A current journey map showing each location, primary action, information needed,
  context carried forward, detour and dead end.
- A proposed journey using the existing surfaces, with before/after transitions.
- A content and control placement inventory: keep, move, combine or remove.
  Identify duplicate explanations, competing primary actions and configuration
  exposed before it is useful.
- A reviewable interaction walkthrough for the most confusing transition:
  conversation → agreement → execution → correction. Use an existing real effort;
  any simulated state must be labeled and kept separate from engine records.

**Acceptance:** every primary journey has a discoverable next action, a way back,
and explicit context retention. Record current versus proposed navigation steps
and comprehension failures. A modal has a stated purpose and return destination.
Do not start by changing colors or adding a task view.

**Existing parents:** EXP-001/004/005/009. Inspect `TeamWorkspace`, `LiveShaping`,
`PlanReview`, `PlanExecution`, `PlanRecovery` and the existing inspector.

### JRN-02 — Make arriving and thinking feel natural

**Person's experience:** I can start with a question, an uncertain idea or a clear
request. As we talk, my thinking becomes clearer without forcing me into planning.

**Product changes**
- Refine the empty and returning states separately. Empty state explains the
  immediate useful action; returning state restores useful context. Neither asks
  the person to learn the entire product.
- Keep conversation in the expanding Guide. Organize the emerging understanding
  in a concise editable form within that context, using the existing brief.
  Explore whether it should be visible or folded based on its usefulness; do not
  assume another permanent summary panel is necessary.
- Make decisions, alternatives and unresolved questions distinguishable. Preserve
  direct user corrections and avoid rewriting the display after every sentence.
- Surface agent/team or access setup only when relevant, with the existing editors
  and draft-preserving return. Do not require team assembly to get a quick answer.
- Keep a new thought separate from a change to an active effort when intent is
  ambiguous. Clarify consequential scope instead of guessing it.

**Acceptance:** someone can explore two alternatives, reject one, switch attention
and return to a correct saved understanding. No work assignments appear merely
because they conversed. A short request remains short. No modal or route change
unexpectedly replaces the place where they were thinking.

**Support:** reuse revisioned briefs and Guide context. Share only reviewed,
authorized context with dispatched agents, not unrelated personal discussion.

**Existing parents:** EXP-001/003/005; `LiveShaping`, `WorkingBrief`, `WorkComposer`.

### JRN-03 — Turn agreement into an obvious, continuous handoff

**Person's experience:** I understand the proposed outcome and can send the team
to work without hunting for a button or interpreting several copies of a plan.

**Product changes**
- Present one current handoff in the conversation: what will be produced, what
  success means, who will do it, relevant access and the applicable limits.
  Keep detailed assignments and allocation arithmetic secondary.
- Make the primary action unmistakable. Asking for a change stays in the same
  conversation and updates the same visible proposal.
- Solve missing access or configuration with a contextual route back. Avoid
  bouncing the person between plan review, chat and settings without orientation.
- After start, evolve the handoff into a truthful progress summary. Keep the
  discussion accessible and connect the work to its place on the map.
- For a simple task, use a compact handoff; do not impose the presentation needed
  for a large multi-agent plan.

**Acceptance:** request two revisions, fix one setup issue and start the intended
version without losing a draft or creating duplicate visible plans. The person
can explain what starts and its limits without opening advanced details. An
uncertain start shows a clear pending state; retry does not duplicate execution.

**Support:** preserve existing exact agreement/start receipts and readiness checks.
Correct demonstrated execution blockers as dependencies of this journey; never
substitute attractive progress for successful dispatch.

**Existing parents:** EXP-002/004/006; `PlanReview`, `PlanExecution`, current
single-proposal and dispatch implementation.

### JRN-04 — Make several ongoing efforts understandable

**Person's experience:** I can pursue something new while knowing what is happening
elsewhere, without managing each agent or remembering every task.

**Product changes**
- Refine the information hierarchy at workspace, effort and task depth. Start with
  purpose, meaningful progress, what needs me and available results.
- Compare compact summaries and grouping within existing map/overview surfaces.
  Use real plan structure and lineage; do not invent a mandatory taxonomy.
- Make selection and focus stable. Opening detail, switching efforts and closing
  inspection should retain camera, discussion draft and place.
- Distinguish working, waiting for me, waiting on a dependency, stopped and result
  available with consistent language, color and restrained truthful motion.
- Keep agent assignment legible on earlier tasks even when the agent's avatar is
  currently attached to later work.
- Keep the Guide aware of the selected context so I can ask about progress without
  navigating into every task. Show its scope without making selection an implicit
  authorization to change work.

**Acceptance:** with three efforts and a 10–30-task project, a person can identify
which effort needs them, what is active and what is ready without inspecting every
node. Measure correct comprehension and navigation effort against the baseline.
Do not use fabricated percentages or polished sample names as proof.

**Design gate:** add a new view only if the comparison establishes a concrete
comprehension benefit that refinements to current surfaces cannot provide. No
Map / Tasks switch is committed by this ticket.

**Existing parents:** EXP-005/006/007; OCT-205; `WorkOverview`, `WorkShelf`,
`workJourneys`, `workSignals`, current map projections.

### JRN-05 — Make decisions, redirection and stopping feel local

**Person's experience:** when work needs me, the product helps me make the decision.
When I change my mind, I can act where I already am and understand the consequence.

**Product changes**
- Present a human request as a concise question with why it matters, available
  choices and relevant evidence. Put the response controls with the question.
  Expand context on demand rather than opening an information-dense review wall.
- Let me request a change conversationally or from the effort. Explain what will
  continue, change or stop before applying a consequential change.
- Keep scope and limits visible where the decision is made. A budget interruption
  should explain the consequence and choices without requiring a trip through
  general configuration.
- Place stop controls beside the work they affect and make workspace stopping
  consistently reachable. Show stopping versus confirmed stopped.
- Distinguish setting aside a discussion, pausing supported work, stopping an
  execution and archiving settled history. Do not give different effects the same
  reassuring label.
- Show a concise receipt after action and preserve the place where the person was
  reading. Independent work remains understandable while a question is unresolved.

**Acceptance:** respond to a request, revise one part and stop another without
reconstructing the plan. The person can tell which work changed and which continues.
Unsupported live edits offer an honest stop-and-review path. A stale request or
failed stop remains visible and cannot look successfully resolved.

**Support:** use existing amendments, cancellation, human controls and continuation.
Implement missing scope enforcement before enabling the corresponding control.
Do not imply that stopping reverses completed external effects.

**Existing parents:** EXP-005/007/008; OCT-203; `AttentionPanel`, `WorkDetails`,
`PlanDirectionEditor`, `PlanRecovery`, existing control authority.

### JRN-06 — Make results and returning worth the delegation

**Person's experience:** I come back to useful answers and outputs, understand what
changed, and can investigate or correct them without starting the explanation over.

**Product changes**
- Lead completion with the actual result, location and concise account of what was
  checked or remains unresolved. Keep transcripts and internal execution secondary.
- Connect the result to its evidence and contributors using the existing inspector
  and Blackboard. A completed task is not automatically a satisfactory outcome.
- Attach a correction to the result being corrected. Carry its context into the
  Guide and preserve prior output/history.
- Present meaningful changes since last review alongside the relevant effort,
  emphasizing decisions and usable results. Avoid an automatic digest modal.
- Keep continuation attempts recognizable as the same ongoing effort, with
  individual attempts available in history. Prevent stale plans and abandoned
  drafts from competing with current work.
- Provide contextual archive/restore for eligible settled work. Never hide active
  or unresolved work to make the overview appear tidy.

**Acceptance:** return after two efforts progress and one blocks; identify the
changes, inspect one result and request a correction without pasting context again.
Earlier results remain available; continuations do not produce confusing duplicate
efforts. Viewing one item does not silently mark every other result reviewed.

**Support:** reuse lineage, result references and authoritative events. Add only
missing persistence needed for reliable return awareness and visibility choices.

**Existing parents:** EXP-005/007/008/009; OCT-205; `WorkDetails`, `ProjectBlackboard`,
`workJourneys`, current recovery and artifact records.

## Original proposed sequence and completion

The Guide workstream linked above now takes priority for its overlapping journeys.
Retain this broader sequence as backlog context, not a competing immediate plan.

1. Complete JRN-01 first and establish the interaction direction in a reviewable
   walkthrough. This is the immediate product work, not an engine-budget sprint.
2. Refine JRN-02/03 as one coherent slice: arrive, think, agree and see work begin.
3. Refine JRN-04/05 together: oversee parallel efforts and intervene in context.
4. Complete JRN-06 and repeat the whole journey, including changing direction and
   returning. Evaluate small requests as seriously as the large-project scenario.

Each slice includes layout, content, control placement, transitions, empty/error
states, keyboard/focus behavior, responsive behavior and reduced-motion support.
Changes to wording alone or passing component tests do not close product acceptance.
Preserve existing design strengths; avoid cosmetic redesign as a substitute for
removing friction.

Capture baseline and revised paths with navigation counts, lost-context incidents,
whether the next action was found, correct understanding of state, and successful
completion. Target improvements rather than arbitrary click reductions that hide
necessary decisions. Include actual owner walkthroughs; do not claim validated
usability from source inspection or from the implementer's opinion alone.

## Engineering dependencies remain visible, not the organizing principle

| Journey affected | Known dependency / boundary |
|---|---|
| Handoff and useful completion | The latest [large-project trial](project-width-proof-2026-10-13.md) failed after branch allowance overrun; input-aware admission and safe exhaustion isolation remain open |
| Budget choices | Current work accounting exposes tokens; monetary enforcement and explicit effort-level time controls require real engine support before the UI promises them |
| Stop and change direction | Per-work controls exist; workspace-wide stopping is not exposed by the local connection. Upcoming amendments and linked continuation do not establish arbitrary live editing |
| Return and continuity | Preserve real lineage, accounting and results; prove any new reviewed-state or archive persistence across reload/restart |

These are dependencies to investigate and repair within the applicable vertical
slice, not waived requirements. A UI demonstration may show an explicitly labeled
unsupported state; it cannot pretend the engine executed it. Do not run more paid
qualification merely because this plan exists.

No product code or runtime state was changed in this planning revision. October 25
readiness remains unproven; any scope/date decision must be made explicitly.
