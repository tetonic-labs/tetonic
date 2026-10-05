# UI consolidation validation

This protocol verifies the revised [autonomous-team workspace sprint](ui-consolidation-sprint.md). Updated October 4, 2026. The single-agent build, contract tests and developer browser checks recorded in the [baseline](baseline.md#product-review-rebaseline-october-4) are partial foundations. The new team, coordination and comprehension scenarios below are not yet run. Neither earlier checks nor this revised plan establishes their success.

## What to record

For each scenario, record the build revision and dirty-tree state, browser/viewport, hardware, supported model/profile, relevant work IDs, expected behavior, observed behavior, evidence location and remaining issue. Keep real-model usefulness, deterministic contract tests, design fixtures and human observations separate. Never export credentials or protected private content with evidence.

The usability sample is formative, not a market claim. Aim for three fresh participants using the supported profile, shared with the already planned pilot recruitment; do not create a second recruitment program. Outreach requires separate user authorization. If participants are unavailable, record missing evidence and continue useful engineering checks. An author walkthrough cannot be recorded as a fresh-user success.

## Human scenarios and targets

These are proposed usability targets to test. They have not been measured and are not guarantees about model speed. Observe before explaining; do not start with a product tour. Count facilitator help, wrong turns and mistaken assumptions, as well as elapsed time.

| Scenario | Prompt or situation | Target and required observation |
|---|---|---|
| First moment | Open a ready, empty workspace. Ask what Tetonic lets the person do and how they would compare the supplied documents | Within 30 seconds identify delegation and the next action, without a team/mode explanation; record if it is mistaken for only a chatbot |
| Missing prerequisite | Open without a usable engine connection or selected model | Explain what is unavailable and identify the next supported step; do not mistake the page for active work |
| Start useful work | Supply two documents and a comparison criterion | Begin through the ordinary composer, understand the resource scope and distinguish sending from accepted work |
| Return | Reopen the team outcome after one contribution and one material change | Within 60 seconds identify the outcome, current child work, responsible agents, actual interaction, blocker/needed decision and what changed; no transcript reconstruction |
| Continue the same work | Ask a follow-up, then change a constraint | Use the existing work conversation without starting an unrelated assignment; recognize pending versus applied change |
| Make a decision | Show a bounded action proposal with meaningful consequences | Before consenting, correctly describe the action, destination/scope and relevant consequence. A critical misunderstanding is a failure, regardless of speed |
| Inspect a result | Open the recommendation and its supporting material | Distinguish the result from evidence of validation; reach the actual supporting record without changing products |
| Take control | Ask the person to pause or stop one supported work item | Identify the scope, request the correct control and distinguish acknowledgement from uncertain effect; avoid stopping unrelated work |
| Understand privacy | Compare a personal conversation with an available team context | Correctly identify the audience and whether prior private history is shared. Use only actually implemented scopes |
| Carry several kinds of work | Show three assignments: supplied research, project investigation and an ongoing responsibility where supported | Locate each without a new navigation model. Measure coordination actions and return effort; do not claim the team reduces burden without observations |
| Team contribution | Ask two authorized agents to examine supplied material toward one outcome | The engine delivers scoped context and the contribution; no human copy/paste between agents; both contributions affect an inspectable combined result |
| Agent needs help | One contribution needs information or correction from another agent | The recipient acknowledges acceptance/defer/decline, work ownership remains clear, and bounded coordination results in progress or one understandable escalation |
| Shape ongoing work | Change a relevant constraint after a contribution exists | Recognize pending/accepted direction, which work is retained or superseded, and the boundary for in-flight effects; a chat acknowledgement alone is insufficient |
| Human flag | An agent encounters ambiguity or a coordination limit | Understand the question, affected work, choices and effect of waiting; answer once and observe its acknowledged resolution |

Sprint 1 must demonstrate the solo baseline and the smallest governed two-agent journey through the real browser path (COORD-A/B/C). The full multi-work and recurrence scenarios remain in Sprint 2 under OCT-203/204/205. Layout fixtures may test legibility only; they cannot satisfy execution or release gates. A two-agent fixture, fixed sequence of model calls, or two unrelated root jobs without shared authority/lineage cannot satisfy the team gate.

Record individual outcomes rather than hiding problems in an average. Require zero unresolved critical misunderstandings of audience, authorization, result certainty or stop scope. For softer 30/60-second targets, record misses, repairs and retests; assess product acceptance explicitly rather than treating a small sample as proof that every user will understand.

## Deterministic contract checks

| Condition | Required product behavior |
|---|---|
| Empty successful response | Empty real workspace; no starter agents, approvals or work substituted |
| Endpoint failure or partial polling failure | Preserve affected last-known data with freshness/error state; do not silently replace it with an empty list or sample |
| Duplicate click or lost create response | Same retained operation identity; one accepted operation; uncertainty until reconciled |
| New intentional identical request | A separate operation is possible; content similarity alone does not suppress it |
| Draft and recipient change | Text preserved per destination; no automatic forwarding or context leakage |
| Reload and engine restart | Accepted work/history survives; unacknowledged work is not claimed durable; interrupted execution is labeled accurately |
| Same agent in two conversations | No accidental transcript merge or wrong parent binding |
| Intent changed while running | Versioned accepted change with actual application boundary; UI does not claim the in-flight action changed retroactively |
| Digest-only, changed or expired proposal | Cannot approve an unknown or stale effect; obtain current details where supported |
| Approval response lost | No success receipt without confirmation; retry/reconciliation cannot dispatch a repeated effect |
| Pause, cancel and scoped stop | Distinct command semantics and pending/acknowledged/failure presentation; disclose unresolved external effects |
| Result without validation | Result available; no Verified, checks passed or fabricated evidence |
| Private and team records | Existing authorization governs reads, sends and publication; inspection cannot broaden access |
| Missing artifact or unavailable telemetry | Explain unavailable evidence; do not infer zero activity, zero cost or success |
| Reduced motion and stale connection | All state remains legible without motion; unavailable updates do not animate fictitious activity |
| Conversation versus work lineage | A reply never becomes a delegated subtask implicitly; actual goal/work/dependency/attempt references drive the view |
| Mixed child states | A completed sibling cannot hide a required child that is running, blocked, failed or awaiting a person; no invented percent-done |
| Shared allocation race or proxy request | One remaining allocation admits at most one child; forwarding preserves origin and allowance; denial never launches an unrelated root job |
| Duplicate delivery or lost acknowledgement | Same exchange identity returns the original receipt; no duplicate task/effect; received, accepted and completed remain distinct |
| Routine versus urgent agent message | Routine messages never preempt; urgent attention is authorized, bounded and acknowledged only at an available safe boundary |
| Busy/unavailable recipient | Durable pending/deferred state, finite deadline and explicit expiry/failure; no silent loss, unlimited queue or retry spin |
| Reciprocal interruption or dependency cycle | Reject explicit cycles; exhausted exchange/interrupt allowance parks affected work and raises one human flag; eligible execution is not starved |
| New IDs or proxy used to evade limits | Root-work counters and scope still apply; another agent cannot renew the same coordination allowance |
| Restart during an exchange or interruption | Preserve receipts, origin, counters, ownership and continuation boundary; reconcile uncertainty rather than replaying external effects |
| Human emergency stop under message pressure | Not throttled by agent-message limits; no new descendant admission after confirmed stop; unresolved tool effects remain visible |

Use existing test layers: frontend interaction tests for acknowledgement and context, adapter tests for state projection, application/resource tests for authority and lifecycle, and browser-to-engine tests for the full command path. Add tests for behavior that can regress, not assertions that merely reproduce component markup. Run changed-path checks and current build; broaden only when a change crosses additional contracts.

## Small-team exit demonstration

1. Use two actual registered agents in one authorized team and the supported local provider/tools. Record the engine revision, effective limits and work/run/attempt/exchange identities without secrets. A serial local model is acceptable; record measured concurrency honestly.
2. Give a document-based outcome with criteria through the ordinary workspace. Let the agents choose an input-specific breakdown; accept its bounded scope where policy requires. Do not seed the expected plan or answer.
3. Observe acknowledged child work, an actual permitted tool/resource interaction, and a contribution or help request delivered by the engine. The human does not forward context manually. Close and reopen the browser while work continues.
4. Return to the overview, inspect child work and resource activity, and open the combined result and its contributing records. Change one constraint and exercise a human flag; record actual pending/applied boundaries rather than assuming immediate effect.
5. Exercise stop/restart and the adversarial coordination cases through deterministic tests. Preserve available outputs and unresolved effects. Do not require the real model to trigger every fault voluntarily.

Report supervision actions, manual handoffs, coordination requests, urgent requests, escalations, actual useful contributions, elapsed time and model/tool usage. Compare to the solo baseline where inputs are comparable; more agent traffic is not evidence of more useful work. Required functional gate: no human relay of agent-to-agent contributions, finite coordination, truthful return state and inspectable results. Product usefulness and the 30/60-second comprehension targets still require observations; do not infer them from test counts.

## Interaction and visual review

Check desktop at 1440x900 and 1280x800, narrow layout at 390x844, and 200 percent browser zoom on the desktop profile. The narrow layout must reflow; it cannot depend on a miniature unreadable map. Record the tested browser and rendering conditions.

- Start, inspect, send, decide, pause and return using only the keyboard. The work selector is an accessible alternative to spatial targeting.
- Open and close each overlay. Focus enters meaningfully, Escape acts locally, and closing restores focus to a sensible origin. No obscured active control or nested modal trap.
- Inspect long names, large responses, empty descriptions, validation errors and long decision evidence. Keep the composer, its scope and consequential controls readable.
- Verify status through text/shape as well as color, readable contrast and usable touch targets. Use targeted assistive checks where available; automated scans alone do not prove accessibility.
- Turn on reduced motion. Camera, status, docking and focus still communicate the same information. Polling must not pull focus or flood live announcements.
- Compare ready, waiting, failed, disconnected and recovered states. They should be understandable without a tutorial, a developer console or a permanent legend.
- Confirm the copper brand, consistent typography and purposeful spacing across the remaining surfaces. Visual refinement must not push important consequences below an expansion.

## Cutover and removal record

Before deleting a surface or mutation path, record the following fields for each change:

| Field | Required evidence |
|---|---|
| Existing route, component or action | Actual path and current callers |
| Behavior that must survive | User action, record identity, privacy boundary and recovery behavior |
| Replacement | New entry into the existing service/record, with no second mutation owner |
| Compatibility | How existing links, drafts and historical work are handled |
| Verification | Relevant test or browser scenario and observed result |
| Removal | Scoped diff and residual callers/assets, with reasons for anything retained |

Remove fake activity from the normal journey as P0. A full historical package cleanup remains OCT-305. Preserve test fixtures and useful development previews only when explicitly separated from production routes and state. Do not delete databases, credentials, unrelated work or another contributor's changes as UI cleanup.

## Exit checklist

- [ ] UI-001 through UI-008 have current evidence; no ticket is closed by a design sketch alone.
- [ ] COORD-A/B/C have linked evidence under their existing OCT-201/202/205 owners; their full parent exits remain separate.
- [ ] The ordinary home and one composer complete a real supported task without team setup or `engine=raw`.
- [ ] A real two-agent outcome includes scoped contribution/help exchange without human relay, inherited limits and an observed resource interaction.
- [ ] Overview, child-work and interaction detail answer the operator's questions without requiring a transcript or adding another product mode.
- [ ] Coordination limits, receipt replay, recipient absence, dependency cycles and human escalation pass deterministic checks; human stop remains effective.
- [ ] The map, selector, focused work, conversation and decision brief resolve the same authoritative identities.
- [ ] A real result and its evidence are inspectable; verification and acceptance claims match recorded facts.
- [ ] Lost response, denied action, reload, disconnect and restart show truthful recoverable state.
- [ ] Applicable human controls and privacy boundaries pass their required contract checks.
- [ ] Required viewports, keyboard flow, reduced motion and long-content cases are checked in the browser.
- [ ] Usability observations are recorded; missing participants or missed targets are explicitly unresolved.
- [ ] Duplicate journey cutover has replacement evidence; production no longer substitutes sample activity.
- [ ] Current builds and meaningful affected tests pass; previous results are not reused as proof of changed code.
- [ ] Advanced OCT-106 setup and optional UI-009/010 are verified or explicitly deferred without weakening UI-008's required baseline.
- [ ] OCT-103/104/105 statuses reflect their full parent acceptance, including engine obligations beyond this UI plan.

Carry confirmed defects and evidence links into the existing sprint progress record. Do not create a separate reporting dashboard or mark the October release ready because this UI sprint passes.
