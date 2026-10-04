# Sprint 1 UI consolidation

Created October 4, 2026. Status: planned implementation detail within the active [Coherent workspace sprint](plan.md). Target window: October 4 to 10; release decision: October 25. This plan replaces scattered UI work with one product journey. It adds no fourth calendar sprint and does not reset completed engine work.

The intended experience is simple: a person describes something worth doing, Tetonic helps move it forward within their authority, and the person can return to understand progress or make a decision without supervising every agent. The interface should feel considerate, clear and personable. Its visible complexity should follow the work the person is doing, rather than the number of subsystems available.

## Outcome and scope

Deliver one map-based home with a floating composer, one focused presentation of a piece of work, and one concise place for requests that need a person. Agents and Teams are optional management overlays. Operator settings remain reachable separately. Simple useful work must not require building a team or learning a graph vocabulary.

This is the detailed UI delivery plan for OCT-103, OCT-104 and OCT-105, with optional improvements under OCT-106 and OCT-107. The ten UI tickets are children of those existing tickets, not ten additional release gates. OCT-101 and OCT-102 still own installation prerequisites and the supported execution boundary. Their missing evidence is not solved by interface changes.

The full child backlog is in [UI implementation tickets](ui-consolidation-tickets.md). The [validation protocol](ui-consolidation-validation.md) defines scenario evidence, usability targets and the exit checklist. All new UI tickets start planned; the [October 4 implementation evidence](evidence-2026-10-04.md) records earlier partial foundations only.

## What human and intuitive means here

| Principle | Observable product behavior |
|---|---|
| A clear next step | A fresh workspace has one prominent invitation to describe work. No forced team wizard, product tour or unexplained activity |
| Ordinary language | People ask, discuss, review, pause and continue. They do not need to understand dispatch, harnesses, grants or run topology |
| Recognition over recollection | Stable names, portraits, work titles and map locations help people recognize what they left. Returning never requires reconstructing a log |
| Progressive detail | Show the current outcome, state and next action first. Evidence and technical activity are one deliberate expansion away |
| Respect for attention | Group updates by work. Escalate when a person can usefully act. Quiet, healthy work does not repeatedly demand acknowledgement |
| Visible consequences | Before a consequential action, explain what changes, where, for whom and under which limit. Friendly copy does not conceal an effect |
| Honest reassurance | Acknowledged work, attempted work and completed work look different. Missing information is explicit; no invented agent emotions, activity or certainty |
| Recoverable interaction | Preserve unsent text, context and last-known results through recoverable failures. Explain what happened and the useful next action |
| Direct access | Keyboard, pointer and touch reach the same work. Motion and spatial memory enrich the experience without becoming prerequisites |

Keep the established copper, ink and warm neutral brand, readable typography, agent portraits and restrained spatial character. A new visual theme, mascot system or animation engine is outside this sprint. Human language and understandable behavior are P0 work; ornamental refinement is optional.

## The product people should learn

There is one primary workspace. A selected piece of work opens a focused view in that workspace. Requests for input open a decision brief with the originating work still identifiable. At most one management overlay is open at a time; closing it restores the previous selection and focus.

```text
Tetonic                                  Teams   Agents   Needs you

                 Your work and its activity
           Goals, people and resources on one map
          Select work to understand or redirect it

          [ Who and what this message is about ]
          [ What would you like to move forward? ]
```

"Needs you" appears prominently only when there is a real actionable request. A compact work list opens from the workspace and selects the same records as the map. It is a navigation aid, with keyboard access and clear labels, not another board or work database. Settings contain installation, provider and operational controls; those details do not occupy the everyday workspace.

The diagram describes hierarchy, not fixed pixel positions. Narrow screens retain the same language and records, using a focused map and accessible work list rather than shrinking a desktop constellation until labels become unreadable.

## First use and returning

### First use

If the configured engine is ready, show a quiet real workspace, an available registered assistant and the composer. Suggested opening: **What would you like to move forward?** Supporting line: **Ask a question, hand over a task, or give your assistant something to look after.**

Do not require selecting a work type, mode, team, model or role before typing. Use permitted configured defaults. If inference, authentication or an allowed resource is missing, name the missing prerequisite and provide the supported next step. Never imply that the workspace is ready when it cannot execute. Do not create a sample agent to fill an empty state.

After sending, keep the person's words visible. Show sending until the engine accepts them; show working only after the execution state supports it. A larger or unclear request can lead to a short clarification or huddle. Simple requests proceed without a mandatory ceremony. A proposed plan does not authorize its own execution.

### Returning

Show the same map locations and selected work where practical. Present a brief factual account of material changes, grouped by work: a result available, a request needing input, or an unresolved interruption. If no meaningful change occurred, avoid manufacturing an update.

Use "Since you left" only when an actual last-seen cursor and current evidence support it. Otherwise use "Latest updates" with real timestamps. Initially this can be a deterministic summary of recorded events; it does not require a new summarizing agent. Cross-agent and recurring-work summaries are completed under OCT-205 in sprint 2.

## Work and spatial behavior

Organize the map around human goals or responsibilities. Agents remain recognizable circles with images or stable accessible fallbacks. Destinations represent actual connected resources relevant to that work. The view should help answer who is working on what, what is being used, and where help is needed.

Keep layout stable across polling. New activity must not recenter the camera or displace what the user is reading. A work item with no participating agent yet remains visible. A failed or offline resource has a labeled state. Never draw a tool destination, communication edge or successful handoff from a guess based on an agent's name or a generated sentence.

The initial map needs a readable overview and a deliberate focus on selected work. Full semantic zoom across large fleets is deferred. Reuse current pan, zoom, focus and reduced-motion behavior. Existing docking may remain when tied to a recorded interaction; where detailed events are unavailable, show an honest work state without inventing motion. New magnetic capture tuning stays with OCT-207 in sprint 2.

One work view presents its title, intended outcome, current state, next step, actual result and associated conversation. A task finishing, a test passing, a result being accepted and an external action being authorized remain separate facts. Detailed activity exposes available requests, responses, tools, errors and timestamps with redaction intact; it does not promise private model reasoning or token streaming from buffered providers.

## Requests that need a person

Use one queue grouped by work, covering clarification, action authorization, result review, budget intervention and recoverable blockers. Distinguish these request types in plain language. Do not turn every update into an approval or every failure into a request for user action.

Each brief presents the question, why it needs the person, the exact effect or consequence, relevant evidence, and available choices. A recommendation is useful when supported. Preserve uncertainty and alternatives; do not preselect consent. The person can open deeper details without losing the brief. Scope, cost, recipients and irreversibility remain visible when material to the choice.

Resolving one request cannot appear to approve all the team's work. Success waits for acknowledgement. An expired or changed proposal is not still approvable. A lost response remains uncertain until reconciled. Parking a request leaves other eligible work available; a global stop is an explicit, separately scoped control.

## Language and interaction examples

These are copy patterns, not new backend states. Use them only when the recorded condition is true.

| Current or mechanical expression | Preferred presentation |
|---|---|
| Deploy a task | What would you like help with? |
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
| Tools and Castle console screens | Move to relevant detail or settings | Show allowed resources with the work or agent; keep operator controls available without an infrastructure tour |

Keep ResourceService, the existing local workspace composition, managed run ownership, broker, approvals and storage as authorities. Consolidating React state must not create a replacement scheduler, identity registry, approval log or conversation store. Browser persistence may retain presentation preferences and unsent drafts, but cannot authorize work or pretend to save accepted work.

## Ticket overview

| Ticket | Deliverable | Priority | Size | Parent | Depends on |
|---|---|---|---|---|---|
| UI-001 | One workspace and a clear navigation hierarchy | P0 | M | OCT-104 | None |
| UI-002 | One composer and a useful first moment | P0 | L | OCT-103, OCT-104 | UI-001; OCT-102 execution path |
| UI-003 | One durable work view and contextual conversation | P0 | L | OCT-103, OCT-105 | UI-002 |
| UI-004 | Truthful map overview and return state | P0 | M | OCT-104 | UI-001, UI-003 |
| UI-005 | Clear human decisions and acknowledged controls | P0 | L | OCT-105 | UI-003 |
| UI-006 | Human language and accessible interaction | P0 | M | OCT-104, OCT-105 | UI-001 through UI-005 for final review |
| UI-007 | Remove duplicate journeys and prove the replacement | P0 | L | OCT-103, OCT-104, OCT-105 | UI-001 through UI-006 |
| UI-008 | Optional agent and team setup | P1 | M | OCT-106 | UI-007 |
| UI-009 | Faster return and work finding | P1 | S | OCT-104 | UI-007 |
| UI-010 | Small composer conveniences | P2 | S | OCT-107 | UI-007 |

Sizes describe scope and uncertainty, not days. Seven required child tickets concentrate existing parent work; they do not imply seven independently shippable features. P0 quality includes baseline human presentation, understandable controls and accessibility. P1/P2 refine an already usable experience.

## Execution sequence and capacity

1. Establish the shell, route inventory, state vocabulary and one primary journey with UI-001. Make a low-fidelity check of first use, return and decision handling before visual refinement.
2. Connect that journey through UI-002 and UI-003. Treat request identity, scope and acknowledgement as prerequisites to adding more entry points. Resolve missing engine contracts under OCT-103/105 rather than faking a UI success.
3. Adapt map and decisions with UI-004 and UI-005. Keep baseline copy, focus and keyboard work active throughout; UI-006 is their final integrated review.
4. Cut over using UI-007. Verify a real task through the ordinary route, failure/retry, return and controls before retiring its old entry points. Make small commits that each identify the replacement.
5. Pull UI-008/009/010 only after required acceptance is secure. Do not spend sprint 3's validation window completing optional customization or motion.

By October 6, reassess the P0 chain against current API gaps and available capacity. OCT-104 is now sized L at the parent level because this is a complete journey consolidation. If it cannot fit the October 10 gate, defer optional work first and record the effect on sprint 2 and the release scope/date. Do not silently consume the feature freeze or weaken truthful state, privacy, acknowledgement or cancellation requirements.

## Boundary with later sprints

Sprint 1 makes one ordinary work journey coherent. Real huddle collaboration and inherited delegation are owned by OCT-201/202; concurrent work and recurrence by OCT-203/204; their map and return projections by OCT-205. New docking polish stays under OCT-207. UI-004 provides the foundation and must not fabricate those future capabilities.

The brief supports independent work of different kinds without assuming a coding repository. Large-fleet semantic zoom, shared human rooms, chat replacement features, voice, a workflow builder, a new graph renderer, broad tool marketplaces and a full design-system rewrite remain outside this sprint.

## Definition of done

All seven P0 UI tickets have linked evidence under the [validation protocol](ui-consolidation-validation.md). A person can begin, inspect, redirect, review and return to real work through the ordinary workspace without a tutorial or a diagnostic route. They can tell who will receive a message, what an action will do, whether it was acknowledged, and what remains uncertain.

Build and contract tests pass for the changed path. The supported viewport, keyboard flow, reduced motion and failure states are checked in the browser. Duplicate product routes and live-mode fixtures are removed or isolated with replacement evidence. Usability observations are recorded separately from developer checks; missing participant evidence is not called a usability pass. Parent tickets remain open until their full engine and product conditions are met.
