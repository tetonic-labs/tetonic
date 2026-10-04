# October sprint 1 Coherent workspace

Dates: October 4 to 10, 2026. Status: in progress. Release target: October 25. Follow the [shared scope and priority rules](../README.md). This sprint makes one useful work journey real and understandable before expanding coordination.

User outcome: open Tetonic, describe a problem without assembling a team, see acknowledged progress, inspect the actual result, and redirect the same work after returning. The map and composer remain the primary workspace.

The [UI consolidation sprint](ui-consolidation-sprint.md) now specifies the human product experience in detail: one workspace, one composer, one focused work view and clear requests for human input. Its [ten implementation tickets](ui-consolidation-tickets.md) and [validation protocol](ui-consolidation-validation.md) decompose this sprint's existing parent work. They do not add another calendar week or replace OCT-101/102's engine and baseline requirements. New UI child tickets remain planned.

## Ticket overview

OCT-101, OCT-102 and OCT-103 are in progress; targeted acknowledgement fixes for OCT-105 are also underway. Other tickets remain planned. The [baseline](baseline.md) and [October 4 evidence](evidence-2026-10-04.md) record fresh checks, commits and gaps. Dependencies identify exit requirements, not a ban on independent preparation. No ticket is marked verified until all its acceptance conditions have evidence.

| Ticket | Work | Priority | Size | Depends on |
|---|---|---|---|---|
| OCT-101 | Establish the baseline and supported release profile | P0 | M | None |
| OCT-102 | Prove one permitted inference and tool path | P0 | L | OCT-101 |
| OCT-103 | Unify durable work and conversation lifecycle | P0 | L | OCT-101, OCT-102 |
| OCT-104 | Consolidate the main workspace and truthful states | P0 | L | OCT-103 |
| OCT-105 | Connect decisions, results and controls to engine acknowledgements | P0 | L | OCT-102, OCT-103 |
| OCT-106 | Improve optional agent and team setup | P1 | M | OCT-103, OCT-104 |
| OCT-107 | Add composer conveniences | P2 | S | OCT-104, OCT-105 |

## OCT-101 Establish the baseline and supported release profile

Work: inventory the current tracked and untracked changes without discarding them. Identify the supported launch path, one primary OS, provider, harness, tool set and storage profile. Reproduce frontend build/typecheck/test results and focused engine tests covering the services touched by this release. Separate pre-existing failures from new regressions. Confirm disk/build prerequisites before expensive runs. Recruit three to five fresh pilot users for sprint 3 and prepare the same three scenario inputs described in the [release plan](../README.md).

Reuse: existing V5 progress, `web/package.json`, `scripts/dev-live.ps1`, local UI contract, managed-runtime tests and architecture checks. Source launch scripts are development aids, not proof of installable packaging.

Acceptance: by October 6, record the source revision and dirty-tree state, actual command results, supported capability matrix, highest risks, pilot arrangements and installation prerequisites in this sprint folder. Identify blockers to the three-week scope. A failed or unrun check is explicitly recorded. The full historical inventory validator must not be treated as a current runtime verifier.

## OCT-102 Prove one permitted inference and tool path

Work: connect one useful provider/harness/tool combination end to end. Support bounded work on explicitly selected files and documents without requiring a code repository. Make frontend tool choices match the backend grant; no tools selected must not silently expand to all default tools. Preserve broker egress authorization, credential-store exclusions and secret handling. If a hosted route is selected, complete its authorized context-disclosure path before granting workspace tools. Keep unsupported model/harness/tool combinations out of release controls.

Reuse: `engine/litho/tetonic-app/src/resources/general_harness.rs`, `registered_executor.rs`, `execution_grants.rs`, the existing broker, tools and local provider-key storage. Use `LocalAgentSetup` and `agentConfiguration` as adapters to those contracts, not an independent permission model.

Acceptance: an actual model reads an allowed source and produces a useful output; authorized edits stay within the selected workspace. Deny unauthorized tools, out-of-scope paths including traversal/symlink escape, credential stores and unauthorized network access before effects occur. Missing or invalid provider credentials produce a clear recoverable error. Record the actual isolation guarantee; do not enable arbitrary shell/process execution without a validated containment and cancellation path.

## OCT-103 Unify durable work and conversation lifecycle

Detailed UI delivery: UI-002, UI-003 and UI-007 in the [consolidation tickets](ui-consolidation-tickets.md). The existing resource and runtime lifecycle remains authoritative.

Work: route the main UI through the existing resource and activation services. Fix the `work-` request identifiers that conflict with the UUID contract. Preserve one request identity across lost-response retries. Bind work, conversation, agent identity, run and attempt explicitly; never infer ownership from a display name or use a work ID as a conversation parent. Creating or shaping a proposal must not dispatch it before the user or configured policy authorizes execution. Intent updates have a durable version and acknowledged application state.

Reuse: `LocalWorkspace`, `LocalEngineContext`, `localEngine`, `engineAdapters`, `engine/litho/tetonic-app/src/local_workspace.rs`, `ResourceService`, `team_work_activation.rs`, `ManagedRunService` and `DurableRunReader`. Promote the reliable retry and conversation behavior already present in the dedicated local UI into the single supported workspace.

Acceptance: create, run, inspect and follow up on one work item without switching to a separate raw/local UI. Two conversations with the same agent remain separate. Double click and lost-response retry produce one accepted operation. Reload and engine restart retain accepted work and history with honest recovery status. An intent edit is shown as pending until the engine accepts it; unhandled edits remain visibly unresolved. Reading state must not acquire execution ownership or trigger recovery.

## OCT-104 Consolidate the main workspace and truthful states

Detailed UI delivery: UI-001, UI-002, UI-004, UI-006 and UI-007 in the [consolidation tickets](ui-consolidation-tickets.md). Optional UI-009 refines finding work and does not gate this P0 exit. The complete journey is sized L; review capacity at the October 6 gate.

Work: retain the full-screen map, floating composer and focused work inspector. Keep a compact way to find work without building a second competing work system. Move Agents and Teams into optional management overlays; keep operator configuration out of the ordinary work flow. Remove the Experiment entry and duplicate boards from the supported journey. Remove fallback examples when real lists are empty or requests fail. Distinguish loading, empty, stale, disconnected, rejected and active states. Preserve last-known real data with its freshness.

Reuse: `App.tsx`, `Workroom`, `TeamActivityMap`, existing focus/zoom controls, `LocalWorkspace` state handling and the readable decision-brief components. Retain the established Tetonic visual language; this is not a theme redesign.

Acceptance: a new empty workspace provides one clear next action. An empty engine response shows no sample agents or work. Disconnects retain clearly marked last-known data and never animate fabricated execution. Creating an agent or team through any exposed route uses the same durable service. The map, inspector and composer refer to the same records. Keyboard focus, basic contrast and reduced-motion behavior work on the supported viewport.

## OCT-105 Connect decisions results and controls to engine acknowledgements

Detailed UI delivery: UI-003, UI-005, UI-006 and UI-007 in the [consolidation tickets](ui-consolidation-tickets.md). Clear language does not replace exact effect inspection, enforced scope or acknowledged control behavior.

Work: show a concise decision with the exact proposed effect, reason, relevant evidence and available choices. Resolve each approval once and display success only after engine acknowledgement. Remove invented cryptographic-verification or result-validation claims. Separate execution completion, available evidence, actual verification and user acceptance. Wire pause, cancel and stop to existing controls with distinct semantics and visible pending/failed outcomes. A pause prevents new actions at a declared safe boundary; cancellation requests termination and reports any unresolved effect.

Reuse: `ActionInboxView`, Workroom decision briefs, `resources/human_controls.rs`, durable approvals/stops and managed runtime cancellation. Adapt lossy fields in `engineAdapters` to authoritative data. Do not create a UI-only approval receipt or state override that can authorize effects.

Acceptance: denial, expired/stale approval, request failure, duplicate click and lost response never show false authorization or dispatch twice. Evidence opens the real output or actual test result. Pausing and canceling a supported run stop the applicable work and tool path; failures are visible. Private context remains inaccessible from a team execution. A blocked decision leaves an unrelated work item usable. Child propagation is extended and proven before delegation is enabled in OCT-201.

## OCT-106 Improve optional agent and team setup

Detailed UI delivery: UI-008 in the [consolidation tickets](ui-consolidation-tickets.md). A useful configured assistant and honest identity/access inspection remain part of the required experience even when custom composition is deferred.

Work: make the existing agent/team forms concise and guided, with understandable roles, allowed resources and limits. Show only supported choices. Make optional membership editing available from either agent or team detail. Stable default identities are sufficient for P0; custom composition must not become a prerequisite for asking a question.

Reuse: `AgentsView`, `TeamsView`, `LocalAgentSetup`, agent resource definitions and membership controls. Do not store a parallel team registry in browser state.

Acceptance: create or edit a permitted composition, reload, and see the same identities and memberships. A permission change does not retroactively broaden an in-flight run. Removing this P1 ticket leaves a usable default small-team configuration and hides unfinished customization.

## OCT-107 Add composer conveniences

Detailed UI delivery: UI-010 in the [consolidation tickets](ui-consolidation-tickets.md).

Work: add one or two small conveniences justified by the journey, such as a clear submit shortcut. Draft preservation on recoverable errors and retained operation identity are required under OCT-103/UI-002, not optional conveniences. Keep larger attachment systems and command palettes out of scope.

Reuse: the existing composer and durable submission contract.

Acceptance: keyboard and pointer paths behave consistently, unsent text is not lost on recoverable errors, and no convenience starts duplicate or unintended work. Do this only after all P0 checks are secure.

## Sprint exit and evidence

Demonstrate a research assignment on supplied documents and a bounded repository investigation through the same workspace. Show one lost-response retry, a denied effect, a successful acknowledged decision, an intent change, a reload and an engine restart. Actual model usefulness and deterministic permission/failure checks are separate evidence entries.

By October 10, all five P0 tickets must have evidence. If the connected journey remains incomplete, defer P1/P2 and reassess sprint 2 capacity. Do not compensate with sample agents or timed success messages. Keep evidence and any split implementation tickets in this folder, and summarize verified changes in the epic progress record. Implementation commits should be small and scoped; preserve unrelated worktree changes.

Apply the [UI validation protocol](ui-consolidation-validation.md) to the ordinary product path. Passing its seven P0 child tickets does not close a parent whose remaining engine, privacy, cancellation or recovery conditions are unmet. Fresh-user observations are distinct from developer tests and remain required release evidence under OCT-303.
