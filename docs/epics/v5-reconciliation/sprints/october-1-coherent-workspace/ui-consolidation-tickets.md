# UI consolidation implementation tickets

These ten child tickets implement the [Sprint 1 UI consolidation plan](ui-consolidation-sprint.md). All are planned as of October 4, 2026. Dependencies and priorities are authoritative in its ticket overview. Earlier engine and approval fixes are reusable foundations, not evidence that these product journeys are complete.

## UI-001 One workspace and a clear navigation hierarchy

Priority P0. Size M. Parent OCT-104.

**User outcome:** opening Tetonic makes the next step apparent without choosing among several products or views.

**Implementation:** make the map and floating composer the ordinary home. Retain one work focus view, one Needs you entry, optional Agents/Teams overlays and settings. Build a compact work selector that targets the same records as the map. Inventory existing deep links and actions before removing navigation. Keep clear loading, no-agent, empty-work and connection-required states. Define a route/selection identity that survives a page reload without saving authority in the browser. A scoped operator stop remains deliberately discoverable.

**Reuse and consolidation:** App, TeamActivityMap, existing dialog/focus primitives and work selection. Remove Work/Map/Experiment as competing top-level experiences. Extract useful parts of Workroom rather than introducing a second shell around it. Existing resource, agent and operator detail remain available in their relevant context.

**Acceptance:** a ready empty workspace has one prominent next action; no example work appears. Selecting the same work through the map or list opens the same identity and state. Closing an overlay returns to its originating selection and focus. A known old link opens its actual record or explains why it is unavailable; it never silently opens an unrelated demo. No stacked management dialogs, mandatory product tour or prerequisite team wizard.

**Evidence:** route/action inventory with replacement destinations, empty and populated browser captures, keyboard walkthrough, and tests for record selection and empty/live separation.

## UI-002 One composer and a useful first moment

Priority P0. Size L. Parents OCT-103 and OCT-104. Requires the supported OCT-102 execution path.

**User outcome:** the person can ask for help in their own words and knows what was sent, to whom, and whether it was accepted.

**Implementation:** give FloatingChat, Workroom and GuideIntake one send contract. Display the current recipient, work/conversation context and enforced audience. Start simple work with the existing permitted default agent. Keep drafts while the person switches context or encounters an error, with drafts scoped to their destination. Persist one operation identity across retries after uncertain replies; do not deduplicate unrelated new requests merely because their text matches. Distinguish initial send, follow-up, clarification and changed direction explicitly in the command contract.

If setup is incomplete, keep the draft and explain the one required next action. Select files or other resources through the already supported grant path. Do not infer file access from installation, an agent's role or the directory in which the UI started. A missing attachment feature is not represented by an upload button that discards content.

**Reuse and consolidation:** LocalWorkspace submission/history behavior, LocalEngineContext, localEngine, durable team work and ResourceService. Remove the fire-and-forget send plus separately created browser work item. Preserve OCT-103's request/attempt identity semantics.

**Acceptance:** double-click and a lost-response retry accept one operation. A subsequent intentional identical request remains possible. The full original text remains accessible. Failure leaves the draft intact and does not produce a success toast. Switching a private conversation to a team context never forwards its transcript or existing draft automatically. A proposed huddle waits for the applicable authorization; typing a broad goal does not consent to unlimited delegation or resources. A simple supported request needs no team form or mode selector.

**Evidence:** browser-to-API tests for duplicate and uncertain send, two conversations with the same agent, changed-context drafts, denied resources and unavailable engine. Run one actual model task through this composer. If a required durable command is missing, resolve it under OCT-103; a local optimistic record cannot close this ticket.

## UI-003 One durable work view and contextual conversation

Priority P0. Size L. Parents OCT-103 and OCT-105.

**User outcome:** opening work explains where things stand and makes continuing the conversation natural.

**Implementation:** show the work's intended outcome, present state, latest material change and next step. Bring the real result forward when available. Keep conversation with its own durable identity, related to the work; do not treat work ID, agent name and conversation parent as interchangeable. Keep the original request available alongside summaries. Progressive details expose source material, contributing agents, available artifacts and actual activity. Use explicit states for pending, applied and rejected intent changes, including when an accepted change affects execution.

Use an optional huddle presentation for a proposed outcome, a small intelligible breakdown and meaningful intervention points. Do not impose a fixed plan, review or manager ceremony on every task. Complex collaboration activates only once OCT-201/202 provides its governed behavior.

**Reuse and consolidation:** Workroom detail, HuddleView, TeamRoom's useful conversation elements, FormattedMarkdown and the existing local work/run/artifact readers. Replace duplicate detail windows with this focus view. Remove adapter assertions of completed milestones, attached artifacts or verification that lack actual evidence.

**Acceptance:** map, list, agent inspection and Needs you resolve the same work identity. Reload/restart preserves accepted input, history, result and truthful recovery state. Editing a title does not mutate the accepted instruction. An intent update is never displayed as applied before acknowledgement. Evidence opens the real artifact or recorded result. Execution completion does not imply verified correctness or human acceptance. Activity details show available tool names, inputs, outputs, timestamps and failures with protected values redacted; unavailable fields stay unavailable.

**Evidence:** one completed document task and one interrupted task reopened in the ordinary UI; a same-agent conversation isolation check; current versus changed intent checks; artifact access and unavailable-evidence cases. Buffered inference is not presented as live token streaming.

## UI-004 Truthful map overview and return state

Priority P0. Size M. Parent OCT-104. Multi-agent and recurrence extensions remain OCT-205.

**User outcome:** the map gives orientation and awareness without requiring the person to follow every moving dot.

**Implementation:** group work by its human outcome or responsibility. Show actual assigned agent portraits and relevant connected destinations. Retain stable locations and camera position during updates; preserve selection by stable identity. Use labels and quiet activity indicators readable without hover. Link the compact work selector and map focus. A known tool interaction may dock an agent at its resource, but a task merely marked running cannot fabricate a tool event.

Project loading, empty, current, stale, disconnected, failed, canceled and interrupted states explicitly. Preserve last-known records on polling failure and mark their freshness. Stop live motion when its evidence is stale. Initial return copy can summarize recorded changes deterministically. A last-seen marker is scoped to the current user/context; when unavailable, show Latest updates rather than Since you left. Activity that needs no action does not create an attention badge.

**Reuse and consolidation:** TeamActivityMap, camera and existing motion helpers; repaired engineAdapters and LocalEngineContext. Keep explicit sample playback solely in development/test paths. Replace fallback-to-fixture branches in the ordinary App and Workroom path. Avoid another event bus or mutable work store.

**Acceptance:** an empty connected engine shows zero invented agents, requests and work. A disconnect preserves identifiable last-known work with a timestamp and never changes to a sample scene. Recovery merges by identity/revision without duplicate nodes. Selecting one work item makes its actual participants and resources understandable. Failed work is not mislabeled paused or done. The idle default assistant is available, not apparently working. A responsibility displays Watching or a next check only after an actual schedule exists.

**Evidence:** browser captures for empty, active, stale, failed and recovering states; deterministic state-mapping tests; repeated refresh selection/camera checks; actual event-to-map correspondence. No large-fleet capacity or real collaboration claim from synthetic layout fixtures.

## UI-005 Clear human decisions and acknowledged controls

Priority P0. Size L. Parent OCT-105.

**User outcome:** the person can understand and resolve a request without investigating a wall of logs or accidentally authorizing something broader.

**Implementation:** combine AttentionView and ActionInboxView behind Needs you, grouped by work. Distinguish clarification, authorization, result review and a blocker needing help. Lead with the actual question, why it matters now and the recommended option when justified. Show exact consequential effects, audience, resource scope, cost/limit changes and irreversibility as relevant. Keep supporting evidence expandable. Deferring a decision preserves it without blocking unrelated eligible work.

Connect pause, cancel, continue and emergency stop to their existing engine commands. Specify scope visibly: one task, a work tree, a team or the applicable host scope. Place ordinary task controls in work detail and a clear broader stop in workspace controls; avoid accidental global stop from an ambiguous icon. Use requesting/acknowledged/failed states, with unresolved effects disclosed. Do not promise to undo an already completed external action.

**Reuse and consolidation:** existing approval acknowledgement component, resource human controls, stop records, managed cancellation and durable inspection. One mutation owner resolves a decision. Extend read projections for inspectable proposed effects rather than inventing a command from a digest.

**Acceptance:** the person can state what Agree/Approve/Accept will do before choosing it. Accepting a recommendation is distinct from authorizing a purchase or publication. Only the exact acknowledged decision gets a receipt. Changed, expired or incomplete effects cannot be approved. Lost response and duplicate-click paths cannot imply confirmed authorization. A failed stop stays visible and does not pretend everything has stopped. No bulk approval or preselected consent in the MVP.

**Evidence:** one successful real acknowledgement and deterministic denial, expiry, changed-digest, offline and uncertain-response cases; supported pause/cancel scope demonstrated through the ordinary UI; an unrelated work item remains usable while another awaits input. Missing effect details keep the authorization acceptance gate open.

## UI-006 Human language and accessible interaction

Priority P0. Size M. Parents OCT-104 and OCT-105.

**User outcome:** the interface feels welcoming and understandable without hiding what matters.

**Implementation:** apply the plan's copy patterns to first use, ongoing work, return, decisions, failures and settings. Preserve stable actual names and portraits; fallbacks remain recognizable without inventing a biography, emotion or coworker. Use generous spacing and a clear reading order. Keep one dominant action per local decision and avoid competing panels. Remove token counters, internal topology and model identifiers from default work summaries; keep operational detail accessible in inspection/settings.

Audit focus entry/return, keyboard navigation, screen-reader names, live-region behavior, touch targets, contrast, zoom and reduced motion. Text and shapes carry status without color alone. Stop automatic camera movement, looping urgency and toast floods. A long response or error must not obscure the composer or its context. No nested modal wizard for ordinary work.

**Reuse and consolidation:** current brand tokens, Portrait, dialog primitives and existing typography. Trim inconsistent copy and duplicate component variants before adding a design library. Existing accessibility utilities remain useful; a visual redesign does not justify replacing working primitives.

**Acceptance:** default flow contains no unexplained mechanical labels. User-facing failures explain the next supported action; deeper diagnostics remain reachable. The real audience is visible before sending or sharing; the word Private is not used for an unenforced boundary. Keyboard users can begin work, inspect evidence, decide and return without a drag gesture. Reduced motion preserves all meaning. Test at 1440x900, 1280x800 and 390x844, plus browser zoom at 200 percent on the desktop profile. No clipped decisions, inaccessible controls or content-dependent overlap.

**Evidence:** annotated browser review across required states, keyboard/focus and reduced-motion walkthroughs, targeted automated accessibility checks where available, and a short copy audit. A screenshot alone does not prove keyboard or assistive behavior.

## UI-007 Remove duplicate journeys and prove the replacement

Priority P0. Size L. Parents OCT-103, OCT-104 and OCT-105.

**User outcome:** every visible entry point works consistently; using the product does not require knowing which screen is real.

**Implementation:** run the validation protocol on the ordinary route. Migrate or preserve existing local drafts/preferences deliberately, and retain historical work. Trace imports, routes and mutations before removing duplicate shells or timers. Separate explicit development fixtures from the production dependency graph; checking a URL flag is insufficient if fake activity can still become a live fallback. Document any retained compatibility route and its expiry or reason.

Retire old components only after their required behavior has a verified replacement. Remove orphaned CSS and unused imports in scoped commits. Update local launch and product guidance to the actual journey. Preserve unrelated uncommitted work; do not use consolidation as permission for a blanket repository deletion.

**Acceptance:** no normal route presents simulated execution, invented validation, a browser-only approval or a second roster. Real work can be submitted, followed up, inspected and controlled without `engine=raw`. Equivalent actions from map/list/agent/attention use the same backend command. Fresh users or available pilot participants exercise the specified scenarios without an introductory tour. Record any help needed and missing observations honestly. No P0 parent is closed with only fixture screenshots or a developer demonstration.

**Evidence:** route replacement ledger, current builds and meaningful contract tests, real browser scenario record, usability observations, and narrowly scoped removal diffs. Release-wide fault and fresh-user trials remain OCT-302/303; this ticket supplies formative product evidence and cutover proof.

## UI-008 Optional agent and team setup

Priority P1. Size M. Parent OCT-106. Start after UI-007.

**User outcome:** someone who wants more control can create or adjust a composition without turning ordinary work into an administration session.

**Implementation:** give Agents and Teams concise overlays: current identities, purpose, members, permitted resources and meaningful limits. Reuse one supported creation form and one durable membership service from either entry point. Place model/harness detail behind an advanced disclosure. Clearly explain that sharing an agent does not publish its private history. Roles do not confer privileges by their name.

**Acceptance:** supported changes survive reload and agree across agent/team/work views. A permission edit cannot silently widen an active run. Unsupported mutation paths are absent or clearly unavailable. Omitting this ticket leaves a useful configured default assistant/team and read-only identity/access inspection; it does not leave decorative create buttons or browser-only data.

**Evidence:** permitted creation/membership change and a rejected change through existing services, refresh/restart checks, and a focused comprehension check of who can see what.

## UI-009 Faster return and work finding

Priority P1. Size S. Parent OCT-104. Start after UI-007.

**User outcome:** someone carrying several responsibilities can find the one they want and identify meaningful changes quickly.

**Implementation:** refine the existing compact work selector with simple title search and a small number of evidence-backed filters when justified by observed difficulty. Preserve readable names and stable selection; group repeated updates within the same work. Keep the base return summary from UI-004 usable without this ticket. Store presentation preferences per user/context, never as engine work state.

**Acceptance:** finding and resuming any of the three validation assignments needs no new navigation model. Search and map select identical records. Completed work remains retrievable. A filter never silently hides a critical unresolved request; the queue remains available. No new dashboard, board or notification center.

**Evidence:** recorded find-and-return tasks with realistic titles and a clearly labeled populated-layout fixture. Synthetic scale checks do not establish runtime concurrency.

## UI-010 Small composer conveniences

Priority P2. Size S. Parent OCT-107. Start after UI-007.

**User outcome:** repeated interaction feels quick without making submission or scope surprising.

**Implementation:** add a discoverable submit shortcut and at most one convenience justified by the observed flow. Preserve ordinary multiline editing and IME composition. Keep destination and audience visible. No command language, attachment subsystem, voice feature or shortcut-only function.

**Acceptance:** pointer and keyboard use the same acknowledgement/retry contract; an IME confirmation or multiline input does not accidentally send. Escape dismisses the local overlay predictably and does not discard a draft. Removing this ticket leaves ordinary typing, sending and recovery fully usable.

**Evidence:** focused keyboard/IME and draft-retention checks. Defer before consuming required release validation time.
