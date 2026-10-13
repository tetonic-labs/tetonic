# Guide enhancement sprint: a partner that keeps your place

Created October 13, 2026. Status: **implemented and engineering-verified**.
Code: `71f4ab8`. See [implementation and live evidence](evidence-2026-10-13.md).
The owner's product walkthrough and the broader October release gates retain
their separate status; neither is implied by this engineering completion.
Baseline: `539766d`. Product target: the existing October 25 local MVP preview.

## Outcome

**A person can think freely, move between concerns, put useful work into motion,
and return to it without repeatedly explaining, organizing or supervising.**

The Guide stays responsible for following relevant work between exchanges. Human
messages and meaningful engine events can activate it; an idle Guide need not
consume inference. Its behavior follows the situation, evidence and authority.
These tickets describe qualities of assistance, not request categories, modes,
keywords, a mandatory planning sequence or an event-to-response script.

Keep the map, expanding Guide conversation, inspector and established appearance.
Improve the interactions within them. A new dashboard, chat-folder system or
graph of every thought is outside this sprint.

## What should feel better

| Ticket | User-visible improvement | Impact | Relative effort |
|---|---|---|---|
| [GPR-001](tickets.md#gpr-001) | Keep thinking without hitting a wall or losing decisions | High | L |
| [GPR-002](tickets.md#gpr-002) | Change trains of thought without filing and reconstructing chats | High | L |
| [GPR-003](tickets.md#gpr-003) | Get useful, proportionate help from the situation at hand | High | M |
| [GPR-004](tickets.md#gpr-004) | Shape and adjust real work through the conversation | High | M–L |
| [GPR-005](tickets.md#gpr-005) | Have important developments noticed between messages | High | L |
| [GPR-006](tickets.md#gpr-006) | Stay oriented across ongoing work, interruptions and results | High | M |

Effort is relative implementation uncertainty, not elapsed days. Context/subject
binding and durable event activation are the largest unknowns. This is not a
promise that six tickets fit in the five days before the October 18 freeze.

## Delivery order and stopping points

1. Record the [baseline journeys](acceptance.md) against the current Guide. Fix
   observed failures rather than judging the content of attractive fixtures.
2. Deliver GPR-001: continue a substantial discussion, preserve corrections and
   return after a reload. Include its UI and real-model check in the same slice.
3. Deliver GPR-002 with GPR-003: move among related and unrelated concerns and
   receive relevant help without forced filing, planning or setup.
4. Deliver GPR-004 with GPR-006: connect that understanding to actual work and
   explain what happened, while maintaining the user's place and control.
5. Deliver the bounded GPR-005 event path, using the same context and work
   capabilities. Exercise it while the person is discussing another subject.
6. Repeat the complete journey, document limits and request the owner's product
   walkthrough. Every earlier slice already includes behavior and UI verification.

The first delivery decision is after GPR-001: record measured remaining effort
and feasibility against October 18/25. If scope does not fit, identify exact
unfinished behavior and propose a release cut or date tradeoff. Do not silently
move the date, weaken existing release gates or label unfinished tickets complete.

## Starting point: reuse, then extend

| Current evidence | Existing support | Actual gap |
|---|---|---|
| [Conversation assembly](../../../../../engine/litho/tetonic-app/src/work/conversations.rs) | Authorized history, current brief and parent lineage | Raw input stops at 12,000 bytes or 64 ancestors; one linear chain is still the main continuity mechanism |
| [Saved understanding](../../../../../engine/litho/tetonic-app/src/work/director/understanding.rs) and [brief store](../../../../../engine/strata/tetonic-memory/src/control/work_briefs.rs) | Notes without a proposal; exact retries and protection against stale overwrites | Notes alone do not solve long-history retrieval, cross-subject context or live-model judgment |
| [Guide capability](../../../../../engine/litho/tetonic-app/src/resources/work_director.rs) | Resources, work, inspection, remember, propose and bounded repair | Broader contextual assistance needs composition and scoped target binding; the Guide cannot currently agree, dispatch or grant through this tool |
| [Overview packing](../../../../../engine/litho/tetonic-app/src/work/director/context_budget.rs) | A 16,000-byte engine observation with explicit omissions | This is not conversation memory or a model token budget |
| [Scoped context binding](../../../../../engine/litho/tetonic-app/src/resources/context_compiler.rs) | Current credential/context checks around context access | The [source interfaces](../../../../../engine/strata/tetonic-context/src/interfaces.rs) are repository-oriented; reuse requires a suitable general mechanism, not a fake repository |
| [Controller](../../../../../engine/litho/tetonic-app/src/team_work_controller.rs) and [managed lifecycle](../../../../../engine/mantle/tetonic-run/src/service.rs) | Existing parallel work, managed admission and durable execution owners | Guide event attention must integrate with them; current telemetry is not proof of durable, replayable Guide activation |
| [LiveShaping](../../../../../web/src/components/team-work/LiveShaping.tsx), [Guide activity](../../../../../web/src/components/team-work/GuideActivity.tsx) and [notes](../../../../../web/src/components/team-work/GuideUnderstanding.tsx) | One expanding conversation with visible real operations, drafts and inspectable understanding | Make focus, context changes and background developments clear without adding persistent control panels |

The previous slice's automated tests passed. They do not establish live-model
usefulness, and `539766d` was not loaded into the running engine by that slice.
Record the actual binary/version and profile used for the new baseline.

## Product rules

- Small questions can end with a useful answer. Discussion does not require a
  team, folder, work item, memory update or proposal.
- A subject can span conversations; a conversation can cover multiple subjects.
  Subject association is revisable metadata, not execution or sharing authority.
- Recent corrections outrank stale summaries. Preserve tentative alternatives,
  explicit rejections, uncertainty and the evidence behind decisions.
- The model chooses useful capabilities; permissions, revision checks, budgets
  and lifecycle transitions remain enforced by the engine.
- Focus changes do not stop work. Remembered intent does not authorize work.
  Background signals do not override a human stop or change another effort.
- Existing work can continue while the Guide responds to the person. Background
  reactions must not block foreground conversation behind a long inference run.
- Show actual activity and receipts. A useful answer can include uncertainty;
  an attractive status message cannot stand in for a real tool call or outcome.
- Make consequential ambiguity explicit. Cheap, reversible associations can be
  corrected easily; mutations need an unambiguous target and current authority.

## Bounded event awareness for this sprint

Cover engine-observable developments in existing authorized work: an unresolved
human request or blocker, a usage threshold, a recorded tool/connection failure,
and a result or state change relevant to an existing effort. Trace each source;
do not manufacture events by interpreting a long response time as failure.

The delivered background capability set is scoped reads and an operator-facing
assessment. It deliberately cannot mutate notes or proposals. This stricter
ceiling protects foreground context while the Guide is checking work. It does not gain worker
tools, retry external mutations, dispatch new work, reopen stopped work or widen
access. Execution controls continue through their existing authorized surfaces.
The model chooses whether/how to respond within this set. Raw events need not
produce a model call or visible notification each time.

Persistent activation/deduplication state is necessary if existing records do not
already satisfy it. The application owns the relevance/attention use case; the
store owns transactional records and the managed runtime owns any inference run.
No second scheduler or process supervisor. A sleeping or stopped local engine
cannot observe continuously; reconnect must clearly catch up from actual state.

## Engineering boundaries

Follow the [ownership map](../../../../architecture/ownership.md) and
[work-service boundaries](../../../../architecture/work-services.md).

- `tetonic-app`: Guide use cases, typed capability adapters and authorized product
  projections. No application-local replacement for storage, retrieval or runtime.
- `tetonic-context`: reusable selection, bounded packing and evidence handling
  where its contracts fit; generalize minimally without regressing coding users.
- `tetonic-memory` and existing context/control services: history, revisions,
  access, source associations and any justified event cursor/receipt. New records
  need migration, retention and compatibility rules; no duplicate transcript store.
- `tetonic-run`, runtime, broker, policy, inference and egress: keep their existing
  ownership of activation, cancellation, accounting and effects. Background Guide
  calls are metered and bounded; a different trigger does not bypass grants.
- Web: current Guide/map/inspector, typed contracts and evidence-based projections.
  Browser focus is a hint; it cannot authorize a target, publish notes or schedule work.

New package creation requires a real dependency boundary. A product-specific
module in the application is appropriate; generic engine mechanisms retain their
owners. Capability design must remain composable rather than encoding a list of
human intents into a router.

## Scope reconciliation

This is the active, dedicated implementation breakdown for the Guide. It replaces
the overlapping sequence in the [earlier Guide plan](../october-first-use-repair/guide-system-refinement-2026-10-13.md),
retaining that document's implementation evidence and product obligations.

| Previous outcomes | Delivery here |
|---|---|
| GDE-001 useful exchanges | GPR-003; acceptance in every ticket |
| GDE-002 thinking and continuity | GPR-001/002/003 |
| GDE-003 delegation | GPR-004; flexible capabilities, no fixed handoff script |
| GDE-004 overseeing work | GPR-005/006 |
| GDE-005 changes and recovery | GPR-002/004/006, within existing execution contracts |
| GDE-006 useful results | GPR-006 |

Multi-subject continuity and event-activated attention make previously broad
goals more concrete and add engineering work. Do not count existing notes or
telemetry as completion of those capabilities. Track the earlier GDE outcomes
through this crosswalk rather than creating duplicate delivery tickets.

This is one focused workstream within the October schedule, not another calendar
week. Existing EXP/OCT requirements and the unsuccessful large-width proof remain
open. Successful Guide interactions cannot close execution/recovery/release gates.

Outside this sprint: a new navigation system, arbitrary generated layouts, agent
families, reinforcement learning, general recurring teams, marketplace work,
multi-user sharing and distributed execution. Existing commitments in those areas
retain their own status; this sprint does not cancel or silently reclassify them.

## Definition of done

Every ticket must pass its user journey and applicable regression checks in
[acceptance](acceptance.md), with source/version, provider/model, usage, failures
and remaining limits recorded. No ticket is complete from a prompt edit, fixture,
attractive layout or green unit tests alone. The final owner walkthrough must
show less re-explanation, filing, navigation and supervisory effort.
