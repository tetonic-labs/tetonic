# October sprint 1 Direct and understand a small agent team

Dates: October 4 to 10, 2026. Status: in progress; scope revised October 4 after product review. Release target: October 25. Follow the [shared scope and priority rules](../README.md). The directory and ticket IDs stay stable. This sprint must prove the smallest real autonomous-team journey, with the enforcement it needs, rather than stopping at a single-agent conversation.

User outcome: hand over an outcome, understand the resulting work and its owners on the map, leave a small team to collaborate within authorized limits, and return to inspect progress, shape direction or answer a concise request for judgment. Simple work still needs no team assembly. The human directs outcomes; the engine coordinates execution.

The revised [UI consolidation sprint](ui-consolidation-sprint.md) specifies the work map, progressive inspection, human direction and bounded agent collaboration. Its [ten implementation tickets](ui-consolidation-tickets.md) and [validation protocol](ui-consolidation-validation.md) remain the delivery breakdown. Required slices of OCT-201/202/205 move forward from sprint 2 under the gates below; this adds no calendar week or duplicate parent ticket. Partial uncommitted UI implementation is recorded in the [baseline update](baseline.md#product-review-rebaseline-october-4); it is not proof of team execution.

## Changes to scope and order

Keep the existing map shell, durable submission/retry path, scoped drafts, result inspection, identity forms and acknowledged controls. Rework the single-assignee conversation projection into a view of actual shared work, dependencies, contributors and resource interactions. Do not discard useful integration or build a second orchestration service.

| Gate pulled forward | Existing owner | Required by the Sprint 1 exit | Remainder in Sprint 2 |
|---|---|---|---|
| COORD-A Governed child execution | OCT-201 | One authorized delegation edge, durable origin/attempt lineage, atomic shared allocation, private-context isolation, bounded retries, parent stop and restart reconciliation | Wider measured team/concurrency envelope and stress/fault coverage; no reachable safeguard is deferred |
| COORD-B Useful collaboration | OCT-202 | Two registered agents make useful, input-specific contributions to one outcome; durable help/contribution exchange, acknowledgement, finite coordination limits and concise human escalation | Richer huddle revision, wider collaboration cases and the second-domain proof |
| COORD-C Work visibility | OCT-205 with OCT-104 | Overview of the outcome, actual child work, owners, blockers and one observed permitted tool/resource interaction; inspect the contributing records | Cross-responsibility return summaries, recurrence activity and richer event presentation |

These are slices of existing P0 tickets, not additional parent tickets or evidence that their full acceptance is met. COORD-A follows the OCT-102 execution boundary and OCT-103 identity contract plus the applicable OCT-105 stop/authorization checks. COORD-B follows COORD-A. COORD-C integrates their evidence into UI-003/004. Full UI cutover does not block preparation of these engine slices; no gate depends on the full exit of a later gate.

Make room by deferring optional custom composition, advanced role editing, work filters, composer shortcuts and motion refinement. Sprint 1 runs one supplied-document team scenario plus a solo baseline. Move its second real repository scenario to OCT-202 in Sprint 2; retain all reachable permission/tool denial checks now. Keep the October 18 freeze and October 21–24 validation window. At the October 6 review, estimate the admission and exchange gaps from code and report any date conflict explicitly; do not silently equate a working single-agent UI with this exit.

## Ticket overview

OCT-101 through OCT-105 have partial work in progress; no full exit is verified. COORD-A/B/C are newly scheduled and remain planned. OCT-106/107 remain deferred until required gates pass. The [baseline](baseline.md) and [October 4 evidence](evidence-2026-10-04.md) record checks, commits and gaps. Dependencies identify exit requirements, not a ban on independent preparation. No ticket is marked verified until all its acceptance conditions have evidence.

| Ticket | Work | Priority | Size | Depends on |
|---|---|---|---|---|
| OCT-101 | Establish the baseline and supported release profile | P0 | M | None |
| OCT-102 | Prove one permitted inference and tool path | P0 | L | OCT-101 |
| OCT-103 | Unify durable work and conversation lifecycle | P0 | L | OCT-101, OCT-102 |
| OCT-104 | Consolidate the main workspace and truthful states | P0 | L | OCT-103 |
| OCT-105 | Connect decisions, results and controls to engine acknowledgements | P0 | L | OCT-102, OCT-103 |
| OCT-106 | Improve optional agent and team setup | P1 | M | OCT-103, OCT-104 |
| OCT-107 | Add composer conveniences | P2 | S | OCT-104, OCT-105 |

Contract dependencies are distinct from full-ticket exits. OCT-103 establishes identities and versioned commands before its final team proof. OCT-105 supplies the single-run authorization/stop contract before COORD-A; its descendant/control and human-flag acceptance finishes against COORD-A/B. This staged ordering avoids requiring OCT-105's entire team exit before implementing the children it must control.

## OCT-101 Establish the baseline and supported release profile

Work: inventory the current tracked and untracked changes without discarding them. Identify the supported launch path, one primary OS, provider, harness, tool set and storage profile. Reproduce frontend build/typecheck/test results and focused engine tests covering the services touched by this release. Separate pre-existing failures from new regressions. Confirm disk/build prerequisites before expensive runs. Recruit three to five fresh pilot users for sprint 3 and prepare the same three scenario inputs described in the [release plan](../README.md).

Reuse: existing V5 progress, `web/package.json`, `scripts/dev-live.ps1`, local UI contract, managed-runtime tests and architecture checks. Source launch scripts are development aids, not proof of installable packaging.

Acceptance: by October 6, record the source revision and dirty-tree state, actual command results, supported capability matrix, highest risks, pilot arrangements and installation prerequisites in this sprint folder. Identify blockers to the three-week scope. A failed or unrun check is explicitly recorded. The full historical inventory validator must not be treated as a current runtime verifier.

## OCT-102 Prove one permitted inference and tool path

Work: connect one useful provider/harness/tool combination end to end. Support bounded work on explicitly selected files and documents without requiring a code repository. Make frontend tool choices match the backend grant; no tools selected must not silently expand to all default tools. Preserve broker egress authorization, credential-store exclusions and secret handling. If a hosted route is selected, complete its authorized context-disclosure path before granting workspace tools. Keep unsupported model/harness/tool combinations out of release controls.

Reuse: `engine/litho/tetonic-app/src/resources/general_harness.rs`, `registered_executor.rs`, `execution_grants.rs`, the existing broker, tools and local provider-key storage. Use `LocalAgentSetup` and `agentConfiguration` as adapters to those contracts, not an independent permission model.

Acceptance: an actual model reads an allowed source and produces a useful output; authorized edits stay within the selected workspace. Deny unauthorized tools, out-of-scope paths including traversal/symlink escape, credential stores and unauthorized network access before effects occur. Missing or invalid provider credentials produce a clear recoverable error. Record the actual isolation guarantee; do not enable arbitrary shell/process execution without a validated containment and cancellation path.

## OCT-103 Unify durable work and conversation lifecycle

Detailed UI delivery: UI-002, UI-003, UI-007 and the UI-008 baseline in the [consolidation tickets](ui-consolidation-tickets.md). The existing resource and runtime lifecycle remains authoritative.

Work: route the main UI through the existing resource and activation services. Preserve one request identity across lost-response retries and the UUID contract. Bind work, conversation, agent identity, run and attempt explicitly; never infer ownership from a display name or treat a conversation reply as a delegated subtask. Extend the existing work projection with goal, parent/child work, dependency, actual contributor, blocker and evidence references. Validate cycles and versioned mutations at engine authority. Creating or shaping a proposal must not dispatch it before the user or configured policy authorizes execution. Intent updates have a durable version and acknowledged application boundary; distinguish retained, superseded and still-in-flight work.

Reuse: `LocalWorkspace`, `LocalEngineContext`, `localEngine`, `engineAdapters`, `engine/litho/tetonic-app/src/local_workspace.rs`, `ResourceService`, `team_work_activation.rs`, `ManagedRunService` and `DurableRunReader`. Promote the reliable retry and conversation behavior already present in the dedicated local UI into the single supported workspace.

Acceptance: create, run, inspect and follow up on one work item without switching to a separate raw/local UI. Two conversations with the same agent remain separate. Double click and lost-response retry produce one accepted operation. Reload and engine restart retain accepted work and history with honest recovery status. An intent edit is shown as pending until the engine accepts it; unhandled edits remain visibly unresolved. Reading state must not acquire execution ownership or trigger recovery.

## OCT-104 Consolidate the main workspace and truthful states

Detailed UI delivery: UI-001, UI-002, UI-004, UI-006, UI-007 and the UI-008 baseline in the [consolidation tickets](ui-consolidation-tickets.md). Optional UI-009 refines finding work and does not gate this P0 exit. The complete journey is sized L; review capacity at the October 6 gate.

Work: retain the full-screen map, floating composer and focused work inspector. Lead with outcomes and their status, reveal child work and owners on selection, and expose actual agent/resource interactions and evidence on deeper inspection. Starting work produces an acknowledged assignment, not a mandatory transcript view. Keep a compact way to find the same work. Agents and Teams are management overlays, not prerequisites to ordinary work. Support selecting the permitted small team under UI-008. Remove competing boards, the Experiment entry and fallback examples. Distinguish proposed, queued, active, waiting on a dependency, waiting on a human, failed, stopped, completed and stale state from actual records; do not infer project completion from one completed run or display invented percentages.

Reuse: `App.tsx`, `Workroom`, `TeamActivityMap`, existing focus/zoom controls, `LocalWorkspace` state handling and the readable decision-brief components. Retain the established Tetonic visual language; this is not a theme redesign.

Acceptance: a new empty workspace provides one clear next action and makes delegation understandable. In COORD-C, a returning person can identify the outcome, current child work, responsible agents, dependency/blocker and observed resource interaction without reading a transcript. The map, list and detail select the same authoritative records. Empty responses show no samples; disconnected records retain freshness and no fabricated motion. Identity/membership mutations use the durable service. Keyboard access, baseline contrast and reduced motion preserve the same information.

## OCT-105 Connect decisions results and controls to engine acknowledgements

Detailed UI delivery: UI-003, UI-005, UI-006 and UI-007 in the [consolidation tickets](ui-consolidation-tickets.md). Clear language does not replace exact effect inspection, enforced scope or acknowledged control behavior.

Work: show a concise decision with the exact proposed effect, reason, relevant evidence and available choices. Resolve each approval once and display success only after engine acknowledgement. Remove invented cryptographic-verification or result-validation claims. Separate execution completion, available evidence, actual verification and user acceptance. Wire pause, cancel and stop to existing controls with distinct semantics and visible pending/failed outcomes. A pause prevents new actions at a declared safe boundary; cancellation requests termination and reports any unresolved effect.

Reuse: `ActionInboxView`, Workroom decision briefs, `resources/human_controls.rs`, durable approvals/stops and managed runtime cancellation. Adapt lossy fields in `engineAdapters` to authoritative data. Do not create a UI-only approval receipt or state override that can authorize effects.

Acceptance: denial, expired/stale approval, request failure, duplicate click and lost response never show false authorization or dispatch twice. Evidence opens the real output or actual test result. Pausing and canceling stop the applicable work at the declared boundary; unresolved effects stay visible. Private context remains inaccessible from team execution. A blocked decision leaves unrelated eligible work usable. Child stop propagation is extended and proven in COORD-A before team execution is enabled. Human clarification, authorization, delivery review and coordination failure have distinct, durable requests with one owner and an acknowledged resolution; a generic failure badge is insufficient.

## OCT-106 Improve optional agent and team setup

Detailed UI delivery: advanced refinements of UI-008 in the [consolidation tickets](ui-consolidation-tickets.md). Its required baseline moves to OCT-103/104 and COORD-B: choose a permitted existing small team, inspect its actual members and resources, and authorize its work. Custom creation/editing remains P1 here.

Work: make the existing agent/team forms concise and guided, with understandable roles, allowed resources and limits. Show only supported choices. Make optional membership editing available from either agent or team detail. Stable default identities are sufficient for P0; custom composition must not become a prerequisite for asking a question.

Reuse: `AgentsView`, `TeamsView`, `LocalAgentSetup`, agent resource definitions and membership controls. Do not store a parallel team registry in browser state.

Acceptance: create or edit a permitted composition, reload, and see the same identities and memberships. A permission change does not retroactively broaden an in-flight run. Removing this P1 ticket leaves a usable default small-team configuration and hides unfinished customization.

## OCT-107 Add composer conveniences

Detailed UI delivery: UI-010 in the [consolidation tickets](ui-consolidation-tickets.md).

Work: add one or two small conveniences justified by the journey, such as a clear submit shortcut. Draft preservation on recoverable errors and retained operation identity are required under OCT-103/UI-002, not optional conveniences. Keep larger attachment systems and command palettes out of scope.

Reuse: the existing composer and durable submission contract.

Acceptance: keyboard and pointer paths behave consistently, unsent text is not lost on recoverable errors, and no convenience starts duplicate or unintended work. Do this only after all P0 checks are secure.

## Sprint exit and evidence

Demonstrate a solo baseline and a supplied-document assignment with two distinct registered agents and real model contributions through the ordinary workspace. Serial execution is acceptable for the measured local profile; two portraits alone are not collaboration. One agent obtains and uses a scoped contribution from the other without the human relaying it. The browser can close while the engine continues. On return show the outcome, child work, owners, observed resource interaction and real result. Exercise a human flag and a changed constraint with acknowledged application state. No fixed scenario plan, canned contribution or predetermined answer may satisfy this proof.

Use deterministic checks for lost-response retry, denied effects, acknowledged decisions, shared-allocation races, private-history isolation, duplicate help delivery, conflicting requests, interruption limits, dependency cycles, parent stop and restart. Forced denial/failure scenarios must not depend on a model choosing to misbehave. Follow the bounded coordination contract in the UI plan. Actual model usefulness, contract checks and fresh-human comprehension are separate evidence entries.

By October 10, all five Sprint 1 P0 parents and COORD-A/B/C slices must have evidence. This does not close all of OCT-201/202/205. If the team journey or a reachable safeguard remains incomplete, leave the gate open and revise the schedule explicitly. Do not compensate with sample agents, a fixed orchestration demo or timed success messages. Keep evidence here and summarize verified changes in the epic progress record. Implementation commits should be small and scoped; preserve unrelated worktree changes.

Apply the [UI validation protocol](ui-consolidation-validation.md) to the ordinary product path. Passing its eight P0 child tickets does not close a parent whose remaining engine, privacy, cancellation or recovery conditions are unmet. Fresh-user observations are distinct from developer tests and remain required release evidence under OCT-303.
