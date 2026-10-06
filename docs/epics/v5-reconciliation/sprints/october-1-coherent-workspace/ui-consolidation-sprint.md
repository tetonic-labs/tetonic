# Sprint 1 Autonomous team workspace

Created and revised October 4, 2026. Status: partial implementation, re-scoped after product review within the active [Sprint 1](plan.md). Target window: October 4 to 10; release decision: October 25. Existing filenames and ticket IDs remain stable. This plan shapes one product journey around autonomous teams under human direction. It adds no fourth sprint and does not reset completed engine work.

Product promise: digital autonomous teams do the work the person wants, while the person controls how that work is shaped. Tetonic handles orchestration and coordination. The person can understand outcomes, work structure, agent activity and interactions at a glance, then inspect the underlying detail or intervene. Chat is one means of direction; it is not the organizing unit of the product. The interface should feel considerate, clear and personable, with visible complexity following the actual work.

## Outcome and scope

Deliver one map-based home with a floating composer, one focused presentation of shared work, and one concise place for requests that need a person. Agent/team management stays in overlays; its minimum required capability is selecting and inspecting a real permitted composition. Custom team building is optional. Operator settings remain separate. Simple work can use one agent without setup; the sprint exit must also prove two agents collaborating on one outcome.

This is the detailed product delivery plan for OCT-103/104/105 and the pulled-forward COORD-A/B/C slices of OCT-201/202/205 in the parent plan. Optional improvements remain under OCT-106/107. The ten UI tickets are implementation children, not extra parent release gates. OCT-101/102 still own prerequisites and the supported execution boundary. Interface changes cannot satisfy missing engine evidence.

The [implementation tickets](ui-consolidation-tickets.md) and [validation protocol](ui-consolidation-validation.md) define the revised scope. UI-001–007 have partial uncommitted foundations; their complete acceptance is unverified. The newly required baseline of UI-008 and COORD-A/B/C remain planned. See the [source-based rebaseline](baseline.md#product-review-rebaseline-october-4); previous test results are not evidence for newly planned collaboration.

## What human and intuitive means here

| Principle | Observable product behavior |
|---|---|
| A clear next step | A fresh workspace accepts an unclear problem or a direct assignment with a visible permitted agent/team. Help the person understand and choose; only acknowledged work appears as active. No forced wizard or product tour |
| Autonomous work is visible | The overview shows actual ownership, activity, dependency and state; it is useful without reading a transcript |
| Ordinary language | People ask, discuss, review, pause and continue. They do not need to understand dispatch, harnesses, grants or run topology |
| Recognition over recollection | Stable names, portraits, work titles and map locations help people recognize what they left. Returning never requires reconstructing a log |
| Progressive detail | Show the current outcome, state and next action first. Evidence and technical activity are one deliberate expansion away |
| Respect for attention | Group updates by work. Escalate when a person can usefully act. Quiet, healthy work does not repeatedly demand acknowledgement |
| Visible consequences | Before a consequential action, explain what changes, where, for whom and under which limit. Friendly copy does not conceal an effect |
| Honest reassurance | Acknowledged work, attempted work and completed work look different. Missing information is explicit; no invented agent emotions, activity or certainty |
| Recoverable interaction | Preserve unsent text, context and last-known results through recoverable failures. Explain what happened and the useful next action |
| Direct access | Keyboard, pointer and touch reach the same work. Motion and spatial memory enrich the experience without becoming prerequisites |

Keep the established copper, ink and warm neutral brand, readable typography, agent portraits and restrained spatial character. A new visual theme, mascot system or animation engine is outside this sprint. Human language and understandable behavior are P0 work; ornamental refinement is optional.

The same visual continuity applies to interactive examples and prototypes. Reuse `web/src/brand.css`, the bundled display/body/mono fonts, and established map/control treatments. Do not introduce project-specific pastel palettes or a separate component style to demonstrate scale. This requirement was reaffirmed by the user during the multi-agent project example review.

## The product people should learn

There is one primary workspace. A selected piece of work opens a focused view in that workspace. Requests for input open a decision brief with the originating work still identifiable. At most one management overlay is open at a time; closing it restores the previous selection and focus.

```text
Tetonic                   Teams   Agents   Tools & MCPs   Needs you

                 Your work and its activity
           Goals, people and resources on one map
          Select work to understand or redirect it

          [ Who will own this work / its audience ]
          [ What are you working through?         ] [Send]
             Explore an idea or give a direction
```

"Needs you" appears prominently only when there is a real actionable request. A compact work list opens from the workspace and selects the same records as the map. It is a navigation aid, with keyboard access and clear labels, not another board or work database. Settings contain installation, provider and operational controls; those details do not occupy the everyday workspace.

The diagram describes hierarchy, not fixed pixel positions. Narrow screens retain the same language and records, using a focused map and accessible work list rather than shrinking a desktop constellation until labels become unreadable.

## First use and returning

### First use

If the configured engine is ready, show a quiet real workspace and its available registered agent or team. Accept an unclear problem as naturally as a direct assignment. Current copy direction: **What are you working through?** Supporting line: **Think it through with your team, then put the plan to work.** The assignment names its owner and scope. Do not advertise recurrence until an engine schedule exists.

Do not require selecting a work type, mode, team, model or role before typing. Use permitted configured defaults. If inference, authentication or an allowed resource is missing, name the missing prerequisite and provide the supported next step. Never imply that the workspace is ready when it cannot execute. Do not create a sample agent to fill an empty state.

Preserve the full original words in their scoped discussion or work record. For
a direct assignment, show sending until acceptance, then place acknowledged work
on the map; opening its conversation is a choice. For exploration, retain the
discussion and developing brief without inventing assignments. Show working only
from execution evidence, including any authorized research. A proposed plan does
not authorize its own execution; existing bounded policy can permit continuation
without approval at every step.

The [shaping, capability and skills contract](shaping-capabilities-and-skills.md)
extends this journey: a persistent conversation and concise evolving brief help
the person understand alternatives before committing work. Discussion alone does
not dispatch implementation. A visible scoped start action applies when a plan
needs authorization; clear already-authorized requests need no additional wizard.
Tools & MCPs includes a distinct Skills area for create/import and availability.
Missing access opens a concise request linked to the affected work and setup path,
with alternatives, rather than a generic failure or an implied connection.

### Returning

Show the same map locations and selected work where practical. Present a brief factual account of material changes, grouped by work: a result available, a request needing input, or an unresolved interruption. If no meaningful change occurred, avoid manufacturing an update.

Use "Since you left" only when an actual last-seen cursor and current evidence support it. Otherwise use "Latest updates" with real timestamps. Initially this can be a deterministic summary of recorded events; it does not require a new summarizing agent. Cross-agent and recurring-work summaries are completed under OCT-205 in sprint 2.

## Work and spatial behavior

The October 4 follow-up adds four product requirements: collision-aware dependency
layout with reserved tool destinations, an original-output team blackboard,
labeled/color-reinforced areas containing parallel projects and teams, and a
read-only director context path from the main composer. The
[work-context and blackboard integration plan](work-context-and-blackboard.md)
separates the implemented product example from the existing engine readers that
must be extended. Example lookup is not live model answering or a completed
governed collaboration gate.

Organize the map around human goals or responsibilities. Agents remain recognizable circles with images or stable accessible fallbacks. Destinations represent actual connected resources relevant to that work. The view should help answer who is working on what, what is being used, and where help is needed.

Keep layout stable across polling. New activity must not recenter the camera or displace what the user is reading. A work item with no participating agent yet remains visible. A failed or offline resource has a labeled state. Never draw a tool destination, communication edge or successful handoff from a guess based on an agent's name or a generated sentence.

Three levels of detail belong to the same workspace: overview of outcomes and exceptions; selected work with its child tasks, owners and dependencies; selected agent/interaction with actual resource, tool and evidence detail. Selection or expansion can implement this for the small team without a new renderer or large-fleet semantic-zoom system. Polling never forces a zoom. Reuse pan, zoom, focus and reduced motion. COORD-C requires at least one real interaction on the supported tool path; missing fields remain unavailable. Rich docking refinement stays in OCT-207.

One work view presents its title, intended outcome, current state, next step, actual result and associated conversation. A task finishing, a test passing, a result being accepted and an external action being authorized remain separate facts. Detailed activity exposes available requests, responses, tools, errors and timestamps with redaction intact; it does not promise private model reasoning or token streaming from buffered providers.

## Work model and implementation ownership

Use the existing goal, team-work, delegation and run records. Add missing relationships or reader fields there before drawing them. A goal/project is a human grouping, work may have children, and dependencies are distinct edges; ordinary work does not require every level. Reject dependency cycles, preserve provenance and versions, and make unknown/stale state explicit. A conversation reply's `parent_id` is not a work dependency or delegation edge. Configured team membership is not evidence of participation in a run.

An overview status is derived from its actual children and explicit completion policy. A running child, unresolved required dependency, failed contribution or human wait must remain visible even if another child is complete. No invented percent-done or “healthy” summary. A displayed owner comes from acknowledged assignment; optional local notes, `lead_id` or `agent_ids` presentation metadata cannot create ownership, authority or progress.

| Contract | Existing authority to adapt | Product projection |
|---|---|---|
| Goals, ownership, child work and dependencies | ResourceService and existing team-work storage; extend missing relationships with migrations | Outcome and child-work views |
| Execution, inherited limits and stop | Registered job activation, ManagedRunService, broker and human-control records | Actual status, accepted controls and unresolved effects |
| Agent collaboration | ResourceService/team participation contexts; add durable work-scoped exchanges to existing storage and delivery through the managed execution path | Contribution/help edges and acknowledged state |
| Resource/tool interactions | Existing run inspection/journal and authorized artifact readers | Actual destination, operation, timestamps, outcome and evidence |
| Human judgment and direction | Existing approvals, work versions and scoped controls; extend clarification/blocker records where needed | One concise brief with a durable resolution |

No browser scheduler or message relay, second team registry, independent approval store or replacement budget ledger. Extend contracts that are incomplete; do not bypass governed child admission by launching unrelated root jobs and drawing a collaboration edge between them.

## Bounded collaboration and interruption

COORD-B needs a small protocol, not an unrestricted agent group chat. Each exchange has an immutable request ID, originating work/budget, sender, authorized recipient and context, type, related dependency/issue, expiry, status and result/evidence references. Delivery can retry; accepting the same request cannot create a second task or effect. Acknowledged receipt is distinct from accepting responsibility and completing it. Recipient absence, rejection, expiry and failure become explicit outcomes.

| Signal | Required behavior |
|---|---|
| Update or contribution | Queue without interrupting; attach to the relevant work and publish only authorized content |
| Request for help | Accept, defer or decline durably; accepted work has an owner, inherited allocation and completion/failure response |
| Urgent correction or dependency change | Request attention at the next supported safe execution boundary; urgency alone grants no permission and never blindly kills an in-flight effect |
| Human flag | Concise question, reason, affected work, available choices and consequence of waiting; unrelated eligible work can continue |
| Coordination cannot progress | Stop the repeated exchange, preserve the work and escalate once with the unresolved conflict/dependency |

Before enabling agent-to-agent delivery, enforce a finite root-work exchange allowance, queue capacity, deadline, urgent-interruption allowance and same-issue cooldown in engine policy. Proposed starting defaults for this two-agent profile: 8 accepted coordination requests and 2 urgent attention requests per root work; 8 pending requests per recipient; a 120-second help-request deadline; a 60-second same-issue cooldown. These are conservative engineering defaults to validate and record at the October 6 review, not user-selected promises or universal values. Status updates/acknowledgements do not consume a new help request, but delivery volume must still be bounded/coalesced. Agent messages cannot increase limits or reset origin identity by inventing new message IDs or proxying through another agent.

Only one unresolved request per work/sender/recipient/issue is active; retain its identity across retries. Reject explicit dependency cycles, including A waiting on B while B waits on A; after the coordination allowance is exhausted, park the affected work and create one actionable human flag instead of retrying indefinitely. Eligible execution must receive scheduling time between attention requests; the system must not stay busy only handling interruptions. Counters, receipts and pending requests survive restart. Separate deterministic tests prove these properties, rather than relying on models to be polite.

Record the interrupted work and the safe continuation point. For the initial slice, queued attention at a tool/turn boundary is sufficient; advertise the actual boundary. Mid-inference preemption or forced tool abortion is not required. An unavailable boundary means queued/deferred attention, never a false “interrupted” receipt. Human pause/cancel/emergency stop remains distinct and cannot be throttled by an agent-message allowance.

## Requests that need a person

Use one queue grouped by work, covering clarification, action authorization, result review, budget intervention and recoverable blockers. Distinguish these request types in plain language. Do not turn every update into an approval or every failure into a request for user action.

Each brief presents the question, why it needs the person, the exact effect or consequence, relevant evidence, and available choices. A recommendation is useful when supported. Preserve uncertainty and alternatives; do not preselect consent. The person can open deeper details without losing the brief. Scope, cost, recipients and irreversibility remain visible when material to the choice.

Resolving one request cannot appear to approve all the team's work. Success waits for acknowledgement. An expired or changed proposal is not still approvable. A lost response remains uncertain until reconciled. Parking a request leaves other eligible work available; a global stop is an explicit, separately scoped control.

## Language and interaction examples

These are copy patterns, not new backend states. Use them only when the recorded condition is true.

| Current or mechanical expression | Preferred presentation |
|---|---|
| Deploy a task | What would you like accomplished? / Start work |
| Dispatch queued | Waiting to start |
| Executing autonomous turns | Working on your request; name the actual activity when known |
| Objective delivered and verified by engine | Result ready; list the checks that actually ran separately |
| Idle | Available, or Waiting for the next scheduled check when a schedule exists |
| Applied agent replans on its next decision | Updating your request, followed by Updated only after acknowledgement; explain when it takes effect |
| Tool execution failed | Could not read this file; offer a retry or the required access step when supported |
| Connection lost | Connection lost. Last update at the recorded time. Work may still be running |
| Private | Use only for an enforced private context; otherwise name the actual audience or local-owner scope |

Avoid military metaphors, forced enthusiasm, decorative typing, implied emotions and repeated congratulations. Keep the person's full request available; an agent-generated title or summary never silently replaces it. Error details remain available without leading with internal identifiers.

## Reuse and retirement inventory

Paths are current code entry points, not an instruction to delete whole files without tracing callers.

| Existing surface or owner | Decision | Destination and removal proof |
|---|---|---|
| [App](../../../../../web/src/App.tsx) Work, Map and Experiment navigation | Consolidate | Map-based workspace is the default. Remove competing navigation after its useful actions have replacements |
| [Workroom](../../../../../web/src/components/work/Workroom.tsx) and [MissionDeck](../../../../../web/src/components/work/MissionDeck.tsx) | Extract and retire duplicate shell | Reuse useful work detail and result presentation. Replace board/list orchestration with one work selector and one mutation owner |
| [DirectorExperimentView](../../../../../web/src/components/work/DirectorExperimentView.tsx), workload presets and sample lineage | Remove from supported journey | Preserve useful fixtures in explicit development/test entry points; release code must not fall through to simulation |
| [TeamActivityMap](../../../../../web/src/components/graph/TeamActivityMap.tsx) and camera/motion helpers | Retain and adapt | Canonical work and interaction projection; stable geometry, focus and accessible labels |
| [FloatingChat](../../../../../web/src/components/graph/FloatingChat.tsx), Workroom start form, [GuideIntake](../../../../../web/src/components/work/GuideIntake.tsx) | Consolidate input | One composer contract. Reuse useful input and clarification presentation; remove independent send paths |
| [LocalWorkspace](../../../../../web/src/components/work/LocalWorkspace.tsx), [LocalEngineContext](../../../../../web/src/context/LocalEngineContext.tsx), [localEngine](../../../../../web/src/lib/localEngine.ts) | Reuse integration | Promote reliable request, retry, history and connection handling into the main workspace. No raw UI needed for ordinary work |
| [engineAdapters](../../../../../web/src/lib/engineAdapters.ts) | Repair projection | Remove invented milestones, verification, scope, timestamps and status aliases; retain explicit unavailable states |
| [ActionInboxView](../../../../../web/src/components/views/ActionInboxView.tsx) and [AttentionView](../../../../../web/src/components/views/AttentionView.tsx) | Combine entry and detail | One Needs you queue; reuse acknowledgement handling and concise briefs |
| [TeamRoom](../../../../../web/src/components/work/TeamRoom.tsx) and [HuddleView](../../../../../web/src/components/views/HuddleView.tsx) | Fold useful context into work | Team and work conversations remain distinctly scoped. No separate room required to finish the same assignment |
| [AgentsView](../../../../../web/src/components/views/AgentsView.tsx), [TeamsView](../../../../../web/src/components/views/TeamsView.tsx), [LocalAgentSetup](../../../../../web/src/components/work/LocalAgentSetup.tsx) | Retain optional management | One durable create/edit path where supported; hide unsupported mutations rather than storing a browser-only roster |
| [ToolsView](../../../../../web/src/components/views/ToolsView.tsx) resource library | Retain a visible Tools & MCPs section | Reuse the existing library for browsing and setup; link team availability and map destinations to the same resource identity. Distinguish configured availability, enforced permission and observed activity |
| Castle console screens | Move to operator settings | Keep infrastructure controls available without requiring an infrastructure tour |

Keep ResourceService, the existing local workspace composition, managed run ownership, broker, approvals and storage as authorities. Consolidating React state must not create a replacement scheduler, identity registry, approval log or conversation store. Browser persistence may retain presentation preferences and unsent drafts, but cannot authorize work or pretend to save accepted work.

Tools & MCPs remains directly discoverable from the workspace. Its development
example currently supports sample resources and setup drafts using the existing
library; it does not connect MCP servers or grant permissions. Live integration
must use the existing resource and policy authorities for connection state and
team/agent access. Resource activity on the map is evidence of use, never proof
of permission or successful setup. This requirement does not introduce a tool
marketplace or a second configuration store.

## Ticket overview

| Ticket | Deliverable | Priority | Size | Parent | Depends on |
|---|---|---|---|---|---|
| UI-001 | One workspace and a clear navigation hierarchy | P0 | M | OCT-104 | None |
| UI-002 | One composer and a useful first moment | P0 | L | OCT-103, OCT-104 | UI-001; OCT-102 execution path |
| UI-003 | Shared work, child tasks and progressive inspection | P0 | L | OCT-103, OCT-105, OCT-202 slice | UI-002; COORD-A/B engine contracts for team exit |
| UI-004 | Outcome, agent and resource activity on one map | P0 | L | OCT-104, OCT-205 slice | UI-001, UI-003; COORD-C reader/event contract |
| UI-005 | Human flags, shaping and acknowledged controls | P0 | L | OCT-105, OCT-202 slice | UI-003; COORD-A/B for team exit |
| UI-006 | Human language and accessible interaction | P0 | M | OCT-104, OCT-105 | UI-001 through UI-005 for final review |
| UI-007 | Remove duplicate journeys and prove the replacement | P0 | L | OCT-103, OCT-104, OCT-105 | UI-001 through UI-006; UI-008 baseline; COORD-A/B/C |
| UI-008 | Select and inspect a real permitted team | P0 | M | OCT-103, OCT-104, OCT-202 slice | UI-001; existing membership/identity authority |
| UI-009 | Faster return and work finding | P1 | S | OCT-104 | UI-007 |
| UI-010 | Small composer conveniences | P2 | S | OCT-107 | UI-007 |

Sizes describe scope and uncertainty, not days. Eight required child tickets concentrate existing parent work; one P1 and one P2 remain. UI-008's baseline is now required; advanced custom setup stays P1 under OCT-106 and does not block it. P0 includes human presentation, understandable controls and accessibility. See the parent plan for COORD gate ownership; they are not duplicate implementation tickets.

Engine contract readiness precedes UI integration; full COORD scenario verification follows it. The COORD-C reader/event contract can be implemented before UI-004, while the complete COORD-C visibility gate is evidenced by UI-004's browser scenario. Do not interpret these references as a circular requirement that each entire ticket be finished before the other can start.

## Execution sequence and capacity

1. Preserve the working single-agent path; settle goal/work/dependency/assignment identities and readers under OCT-103. Review the existing child-admission blocker and safe delivery boundaries before more UI polish.
2. Complete COORD-A enforcement, then COORD-B scoped exchange and bounded coordination. UI-008 selects an existing permitted two-agent composition; no new setup wizard or browser-only membership.
3. Complete UI-003/004 with actual shared work and interaction evidence through COORD-C. UI-002 supports direction and durable acknowledgement rather than becoming a separate chat product.
4. Complete UI-005 human flags and controls, then integrated UI-006 review and UI-007 cutover proof using the real team scenario. Record failed as well as successful scenarios.
5. Keep advanced OCT-106 setup, UI-009/010 and OCT-207 polish deferred until required acceptance is secure. Do not spend release-validation time on them.

By October 6, reassess the P0 chain against current API gaps and available capacity. OCT-104 is now sized L at the parent level because this is a complete journey consolidation. If it cannot fit the October 10 gate, defer optional work first and record the effect on sprint 2 and the release scope/date. Do not silently consume the feature freeze or weaken truthful state, privacy, acknowledgement or cancellation requirements.

## Boundary with later sprints

Sprint 1 proves the smallest governed two-agent journey through COORD-A/B/C. OCT-201/202/205 retain full ownership and finish broader collaboration, accounting/fault coverage and return-state acceptance in Sprint 2. OCT-203/204 add independent queues and durable recurrence. OCT-207 remains optional motion polish. No later ticket is marked complete by moving its first slice earlier.

The brief supports independent work of different kinds without assuming a coding repository. Large-fleet semantic zoom, shared human rooms, chat replacement features, voice, a workflow builder, a new graph renderer, broad tool marketplaces and a full design-system rewrite remain outside this sprint.

## Definition of done

All eight P0 UI tickets and COORD-A/B/C slices have linked evidence under the [validation protocol](ui-consolidation-validation.md). A person can begin, understand and shape real team work without manually relaying agent messages or reading every transcript. They can identify the outcome, work status, responsible agents, observed interactions and needed judgment, then inspect details deliberately. A solo result or two decorated agent portraits cannot close this sprint.

Build and contract tests pass for the changed path. The supported viewport, keyboard flow, reduced motion and failure states are checked in the browser. Duplicate product routes and live-mode fixtures are removed or isolated with replacement evidence. Usability observations are recorded separately from developer checks; missing participant evidence is not called a usability pass. Parent tickets remain open until their full engine and product conditions are met.
