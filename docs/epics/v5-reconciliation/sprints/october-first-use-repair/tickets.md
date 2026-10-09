# First-use repair tickets

All nine tickets are **P0 and required for the additional product-experience release gate**. EXP-001–004 are in progress with implementation and test evidence; EXP-005–009 remain planned. None has passed full product acceptance yet. See [the first implementation slice](implementation-2026-10-08.md) for exact progress and unresolved qualification. See the [sprint plan](plan.md) for the schedule, existing-owner mapping and added scope, and [validation](validation.md) for cross-ticket proof. Reusing existing owners does not erase the new delivery obligation.

<a id="exp-001"></a>
## EXP-001 — One starting point; discussion before unnecessary planning

**Status:** implemented entry/context changes; actual-model orientation remains imperfect. The owner clarified that discussion must expand upward from the existing composer on the map, using one continuous, subtly translucent surface and compact message rows. It must not open a side drawer or a separate page.

**Problem:** the first screen asks the person to understand two agents; an orientation request becomes filesystem exploration or a premature coding plan.

**Work**

- Make the existing map composer the prominent entry, with concise language explaining that teams can carry out work while the person directs it. Keep Agents and Teams available without making configuration the opening exercise.
- Present the Guide as the default coordination entry. Keep built-in/user-created agent identity and history intact; distinguish idle team members from active assignments. Do not delete agents to simplify an empty screen.
- Refine the existing Guide's context/tools so it can explain actual product capabilities, answer, clarify, brainstorm, or propose execution as appropriate. Avoid a universal coding persona for general requests.
- Change proposal affordances so unresolved orientation is not dressed as an executable project. Keep explicit work/exploration requests possible; do not implement keyword routing or domain-specific scripted plans.
- Offer short contextual help when useful; no mandatory tour, questionnaire, new mode tabs or fixed sequence of prompts.

**UI acceptance:** a new owner can identify where to start without opening an agent form. Asking how to begin receives a useful response and an understandable next action. Brainstorming stays in the same readable space.

**Engine acceptance:** ordinary orientation does not cause purposeless file scans or executable assignments. A genuine request for research or a plan can still invoke the appropriate permitted tools. Actual-model trials include unseen phrasing and noncoding work; no exact reply or exact decomposition is required.

**Reuse:** [bootstrap](../../../../../engine/litho/tetonic-app/src/local_workspace/bootstrap.rs), [Guide director](../../../../../engine/litho/tetonic-app/src/work/director.rs), [shaping](../../../../../engine/litho/tetonic-app/src/work/shaping.rs), [TeamWorkspace](../../../../../web/src/components/team-work/TeamWorkspace.tsx), [WorkComposer](../../../../../web/src/components/team-work/WorkComposer.tsx). No new intent-routing service.

<a id="exp-002"></a>
## EXP-002 — Make relevant access and execution limits usable

**Status:** implemented revisioned selection of host-approved folders, disclosure checks, control-directory exclusions and higher finite ceilings. In-app addition of new host-approved folders and reference model/profile qualification remain open.

**Problem:** an agent can see only an empty folder, cannot reach the requested repositories, and spends its short allowance searching. The owner cannot meaningfully adjust the 8-step/120-second ceilings.

**Work**

- Make the actual granted folder and unavailable capabilities discoverable beside the request/agent. Provide an owner-authorized way to attach or select the relevant local folder without losing the discussion. Do not require a repository for noncoding work.
- Begin with the smallest supported folder-binding path. Trace existing execution grants, workspace handles and host configuration before selecting the API. Any needed authenticated owner mutation must reuse those authorities and be durable/versioned; a browser-selected path alone grants nothing.
- Keep new access separate from model disclosure consent. Recheck before tools run; reject escaping paths, forbidden control/credential locations and stale scope. Existing work keeps its pinned access until an explicit supported rebind/continuation.
- Keep runtime control artifacts out of the user-facing working tree where feasible; otherwise explicitly exclude them from ordinary context discovery. Do not hide arbitrary user files or loosen denial checks.
- Qualify finite default execution settings for the supported profile. Explain per-agent allowance versus host ceiling. Provide an owner-level way to adjust permitted limits and a clear route from a blocked agent form. Preserve higher-level policy limits.
- Make insufficient context a useful question/access request. Do not simply raise timeouts to allow longer unproductive searching.

**UI acceptance:** a request concerning an ungranted repository explains the missing folder and offers the supported setup action. After selection, the original request and draft remain. The agent editor can use a deliberately configured limit above 8 steps/120 seconds, and explains any binding ceiling.

**Engine acceptance:** allowed files become usable only through validated scope. A denied path remains denied; scope changes cannot retroactively expand running work or reset budgets. Changed limits affect the intended new executions, preserve saved identities, and survive restart. The owner can cancel supported tools. Default model/profile tests report response latency and useful output rather than assuming every installed model is viable.

**Sizing checkpoint:** this is substantive authority and configuration work. By October 10 prove the binding boundary or report the missing infrastructure and schedule impact. No unrestricted path editor or implicit whole-drive access as a shortcut.

**Reuse:** [workspace execution configuration](../../../../../engine/litho/tetonic-app/src/host/workspace_execution.rs), existing registered execution/grants and process broker, [agent setup](../../../../../web/src/components/work/LocalAgentSetup.tsx), [workspace settings](../../../../../web/src/components/team-work/WorkspaceSettings.tsx). No second permission store.

<a id="exp-003"></a>
## EXP-003 — Connect first, then select a model inside Tetonic

**Status:** implemented connect-first flow, credential-triggered catalog refresh, advanced fallbacks and typed discovery errors. Real account qualification remains open; no provider credential was supplied for this slice.

**Problem:** the unconnected frontier picker appears to require external research and manual model IDs.

**Work**

- Order the existing editor's provider flow as connect, load available models, select. Keep connection state and its next action next to the model control.
- Refresh the account-aware catalog after credentials are saved without discarding the agent draft. Display searchable names with exact identifiers available secondarily.
- Keep manual model entry and official catalog links as explicit advanced/fallback actions. Do not make them the primary unconnected path.
- Distinguish missing credentials, failed authentication, unavailable discovery and no available models. A catalog entry alone does not prove tool compatibility.
- Preserve grants across provider changes and require any new disclosure consent. Show supported harness choices honestly.

**Acceptance:** from a fresh agent draft, connect the supported provider and select a returned model without leaving the app or typing an ID. Cover credential replacement, discovery failure/retry and no results without losing the draft. An unsupported route is explicit. Qualify one real provider route; do not claim all vendors from mocked catalogs.

**Reuse:** [AgentModelSelect](../../../../../web/src/components/views/AgentModelSelect.tsx), [provider connections](../../../../../engine/litho/tetonic-app/src/workspace/providers.rs), [discovery](../../../../../engine/litho/tetonic-app/src/workspace/providers/discovery.rs). No new provider registry or public catalog scraper.

<a id="exp-004"></a>
## EXP-004 — Proposals the team can actually execute

**Status:** October 9 validation and correction slices implemented. Allocation/roster checks precede proposal writes, with shared launch checks and exact retry preservation. A rejected allocation permits one durable correction in the existing Guide reply; only assignment token allocations may change. Brief and proposal publication is atomic. Actual-model decomposition/synthesis, malformed-argument recovery and the full journey acceptance remain open. See [validation evidence](implementation-2026-10-08.md#october-9-proposal-validation-before-review) and [bounded correction](guide-proposal-correction-2026-10-09.md).

**Problem:** a plan invents a coding objective, assigns every step to one worker, and allocates all tokens to workers before asking the user to repair internal coordination arithmetic.

**Work**

- Use the existing discussion, brief, selected roster, permitted tools and actual limits as planning input. Separate unresolved material questions from execution-ready direction. Let the model author task-specific work.
- Have the engine validate allocation constraints before a proposal is presented as ready. Make the coordination reservation explicit in the existing plan contract and account it within the original total.
- Return structural errors to the planner for at most one budgeted repair attempt. Revalidate the revision afterward. If repair is unavailable or fails, show a concise actionable outcome rather than a raw formula.
- Do not silently increase the total, reduce promised deliverables or change team membership. Material changes remain reviewable. Internal rebalancing must preserve the agreed bounds.
- Show outcome, contributors, deliverables and actual dependencies before Start. Independent assignments to available agents can run concurrently; do not manufacture parallelism where dependencies prevent it.
- Preserve exact agreement, scope checks, idempotent start, shared allocation and stop lineage.

**Acceptance:** the baseline zero-headroom proposal cannot be shown as ready or reach dispatch. A corrected proposal retains the same total unless the owner explicitly changes it. A clear two-agent assignment uses both selected agents meaningfully and produces a combined result; a simple answer does not require a team. Duplicate start and failed repair do not duplicate work or consume unbounded inference. Test new coding and noncoding requests, not a hardcoded baseline plan.

**Reuse:** [plans](../../../../../engine/litho/tetonic-app/src/work/plans.rs), [execution readiness](../../../../../engine/litho/tetonic-app/src/work/plan_execution.rs), existing huddle validation/budget ledger and TeamWorkController, [PlanReview](../../../../../web/src/components/team-work/PlanReview.tsx).

<a id="exp-005"></a>
## EXP-005 — A focused place to discuss, decide and read

**Problem:** long injected context is hard to read; models, limits and accounting compete with the purpose of the work panel.

**Work**

- Keep discussion and current work in the existing focus surface with a stable, explicitly scoped composer. Give long discussion/results a spacious reader and a predictable expand action.
- Grow the composer with its content to a usable viewport-bound height. Long context supplied by the application should be an inspectable attachment/summary where practical, not an unexplained block the user must edit in a two-line field. Sending remains deliberate.
- Put routine model selection and detailed execution limits in agent/workspace settings. Keep concise model/access repair and required data-disclosure decisions beside affected work.
- Replace default accounting prose with a short Work allowance label, a bounded horizontal meter, and a useful remaining amount. Separate used, reserved/unconfirmed and available values; details disclose units and source.
- Present proposal, current action, necessary decision or result as the main content. Preserve drafts, scroll/focus and recipient through settings detours; no nested modal stack or mandatory phase tabs.

**Acceptance:** discuss a long readiness issue, edit it, enter/cancel/save setup and return without losing text or context. Read a long result and evidence without fighting the composer. The meter matches the ledger, never releases unknown usage, never implies money from tokens, and works without color. Routine settings do not precede the work's purpose.

**Reuse:** [LiveShaping](../../../../../web/src/components/team-work/LiveShaping.tsx), [WorkDetails](../../../../../web/src/components/team-work/WorkDetails.tsx), [UsagePanel](../../../../../web/src/components/team-work/UsagePanel.tsx), existing navigation/draft ownership. No new conversation identity or accounting layer.

<a id="exp-006"></a>
## EXP-006 — Make the chosen team visible and delegation understandable

**Problem:** Work with this team returns to Personal without an obvious team presence; creating an agent feels like opening another isolated chat.

**Work**

- After creating/selecting a team, show its name and member portraits in the map's current recipient context. Distinguish the containing Personal workspace from the selected team.
- Give saved idle teams an appropriate visible presence without fake work cards or motion. Link to the existing roster editor and preserve team/agent identity.
- Make the composer indicate who will coordinate which team. Keep direct agent assignment available as an explicit choice.
- When a proposal is made, show how selected contributors participate, what can run together and the combined outcome. Creation alone does not launch work, add tools or copy agents.
- Keep selected roster revisions attached to accepted work. Later edits affect future selections and remain distinguishable from historical execution membership.

**Acceptance:** create a team, select it and immediately identify it on the map and composer; reload retains a valid selection or explains a stale one. Start useful two-agent work without messaging each agent separately. Start a second effort while the first remains visible. Deleting/changing a recipient elsewhere never silently retargets an unsent request.

**Reuse:** [work teams](../../../../../engine/litho/tetonic-app/src/work/work_teams.rs), [TeamPanels](../../../../../web/src/components/team-work/TeamPanels.tsx), [TeamWorkspace](../../../../../web/src/components/team-work/TeamWorkspace.tsx), WorkComposer and existing map projection/layout. No new team registry.

<a id="exp-007"></a>
## EXP-007 — Recognizable state and useful failure recovery

**Problem:** state colors are too subtle and failed work leads with accounting and raw records rather than a useful explanation.

**Work**

- Strengthen status borders/badges or restrained surface fills within the existing brand: active blue, needs-you amber, failed/blocked red, finished green and neutral queued/stopped. Pair with text/icons.
- Keep team/group identity separate from status. Reserve purposeful activity motion for observed running work; preserve reduced motion, stale/disconnected and hidden-document behavior.
- Lead failures with what happened, what is retained, and the supported next action. Distinguish missing access, timeout, provider failure, allowance exhaustion and uncertain effects using recorded reasons.
- Keep technical records, unconfirmed accounting and blackboard available for inspection. A failed attempt cannot become successful merely because its message is friendlier.
- Show waiting for a saved answer to apply separately from needing another answer. Never label a planning run's completion as the user's whole goal being finished.

**Acceptance:** at overview and focused zoom, the owner identifies active, waiting-for-human, failed and completed work. All remain readable in both themes and reduced motion. Timeout offers the relevant settings/continuation path; missing access offers scope repair. A retry cannot blindly repeat an uncertain effect. No invented percentages or model-generated factual status.

**Reuse:** [workSignals](../../../../../web/src/lib/workSignals.ts), existing activity/attention projections, map styles, work details and continuation controls. No new status authority.

<a id="exp-008"></a>
## EXP-008 — Clear unwanted work with archive and restore

**Problem:** abandoned experiments remain on the map with no way to remove them.

**Work**

- Add a scoped durable archive marker to the existing effort/work ownership and API. Resolve source discussion, plan and continuation relationships through existing lineage rather than archiving random child rows.
- Offer Archive from the effort and work list, with undo/restore and a compact archived view/filter. Keep default map and list clear after reload.
- Restrict archiving to settled, non-actionable work for this slice. Active, waiting, approval-pending or uncertain work offers existing control/inspection first; hiding cannot substitute for stopping or resolving it.
- Retain execution history, artifacts, budget records and links. Restoring changes visibility only and never resumes execution.
- Use revision-aware/idempotent mutations. A concurrent transition to actionable work must block an unsafe archive.

**Acceptance:** archive the abandoned proposal and settled failed requests; they disappear from the default map after reload and can be restored. No inference, deletion of results or budget reset occurs. Cross-scope requests fail. Unknown-outcome work and pending approvals cannot vanish from attention through archive.

**Scope:** permanent deletion/data erasure remains deferred and must not be advertised by an Archive label. This ticket includes new engine-backed lifecycle presentation, not just a browser filter.

**Reuse:** [team work storage](../../../../../engine/strata/tetonic-memory/src/control/team_work.rs), existing ResourceService and local API, [workJourneys](../../../../../web/src/lib/workJourneys.ts), current work list/map projection. No second history store.

<a id="exp-009"></a>
## EXP-009 — Prove the repaired journey with actual work

**Work:** follow [validation](validation.md), capture a baseline before code changes, repeat it with a new isolated workspace after integration, and ask the owner to review the repaired experience. Do not overwrite the original review workspace or manufacture successful records. Run model trials within an explicit small allowance and record actual consumption.

**Acceptance:** EXP-001–008 have evidence at the candidate revision; the person reaches a useful result and observable two-agent collaboration, starts a concurrent effort, understands blockers/results and clears abandoned work. Record remaining confusion and failed cases. Passing fixtures, attractive sample data, or a single-agent answer alone cannot pass. The wider recovery/recurrence/install gates remain separately open.
