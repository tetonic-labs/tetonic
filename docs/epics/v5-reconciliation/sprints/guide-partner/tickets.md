# Guide enhancement tickets

GPR-001 through GPR-006 are **implemented and engineering-verified** in `71f4ab8`.
The [evidence record](evidence-2026-10-13.md) maps each ticket to code, regression
checks, live behavior and limits. Owner usability signoff remains separate.
The [plan](plan.md) defines scope and reuse. These are product outcomes, not
conversation modes. Existing `539766d` notes support was the starting point.

## GPR-001

### Keep thinking without losing your place

**Need:** "I can develop an idea for as long as it takes, correct it, and return
without rebuilding the context."

Work:
1. Replace the raw-history dead end with bounded, authorized context assembly:
   recent exchange, current understanding, relevant older evidence and exact
   references to sources. Keep originals inspectable; do not silently discard them.
2. Retain corrections, rejected alternatives, open questions and important exact
   details. Derived summaries carry coverage/source references and revision bases.
   A summary cannot silently turn a tentative idea into an instruction or approval.
3. Reuse scoped retrieval to recover omitted details when needed. Missing access,
   stale evidence and incomplete summaries must produce an honest, usable response.
4. Preserve active drafts and the person's location across refresh, interruption
   and context maintenance. Meter any summarization using existing allowances.

**UI:** same conversation and existing understanding disclosure. No "start over"
instruction because older history no longer fits. Context maintenance must not
teleport the user, replace their draft or add a mandatory memory-management screen.

**Acceptance:** continue past both old 12,000-byte/64-ancestor limits; correct a
constraint, reject an approach, reload and continue. The correction remains in
effect, the rejected approach is not restored, and an older exact detail can be
retrieved. Test non-ASCII input, failed summarization, stale summary writes and
revoked source access. Other agent conversations retain their existing guarantees.

**Owners:** conversation adapters + applicable `tetonic-context` mechanisms;
context/control storage for provenance and revisions; existing Guide UI.
**Dependencies:** none. **Effort:** L.

## GPR-002

### Move between trains of thought without managing chat drawers

**Need:** "I can follow a detour, connect ideas, and return to where I was."

Work:
1. Introduce the smallest explicit subject association the existing context model
   needs. A subject has a stable reference, relevant source spans/turns, current
   understanding and optional work references. A subject need not create work.
2. Support several subjects in one interaction history and one subject across
   interactions. Preserve distinct decisions and alternatives; associate relevant
   evidence without copying a whole private transcript into another subject.
3. Let the Guide propose/use appropriate context from the conversation and engine
   evidence. Avoid fixed keyword routing, mandatory labels or a new subject for
   every message. Ambiguous consequential targets require clarification.
4. Make an incorrect association easy to correct. Revision the association and
   derived understanding; don't rewrite original messages or move existing work.
5. Apply current access checks to source retrieval and sharing. Linking subjects
   does not change grants, team visibility, assignments or execution scope.

**UI:** a quiet, correctable indication of current relevance when useful; natural
language switching and a concise "where we left off" on return. Preserve the
composer and map location. Keep a manual fallback through existing conversation/
work selection, without making filing a prerequisite. No thought-map redesign.

**Acceptance:** explore A, ask about unrelated B, refer back to A, link a useful
insight from C, reload and return. No unwanted project creation; conflicting
constraints remain attached to their subjects. Deliberately misassociate a remark
and correct it. An ambiguous "stop that" must not stop the wrong effort. Private
source canaries must not cross an unauthorized boundary.

**Owners:** Guide composition, existing context identity/access and source
associations; current Guide selection/projections. Record any new durable field's
actual invariant before adding it. **Dependencies:** GPR-001. **Effort:** L.

## GPR-003

### Receive useful help without learning how to operate the Guide

**Need:** "I can say what is on my mind and get a useful response."

Work:
1. Observe live small questions, uncertain ideas, comparisons, corrections and
   mixed requests. Fix demonstrated over-questioning, irrelevant setup demands,
   generic menus or premature planning through context/capability/prompt changes.
2. Give the model composable access to the relevant understanding and engine facts.
   Query detail on demand; don't stuff every effort and resource into every turn.
3. Improve response guidance around useful contributions, proportionate depth,
   explicit uncertainty and questions that actually change the next step.
4. Retain the ability to finish with a direct answer or just continue thinking.
   Do not force a save, proposal or event/action sequence after every reply.

**UI:** keep readable messages and real operation cues. Put necessary setup or
clarification beside the relevant exchange; preserve the draft/place when a
settings/editor surface is needed. No mode selector or intake wizard.

**Acceptance:** use previously unseen wording in at least three domains. A small
answer has no unwanted plan, a vague idea receives substantive help, and missing
access is explained precisely without blocking unrelated discussion. Live-model
observations, not exact scripted answers, determine usefulness.

**Owners:** Guide guidance and capability composition, authorized reads and current
activity UI. **Dependencies:** baseline first; GPR-001/002 for continuity cases.
**Effort:** M.

## GPR-004

### Advance and change work through the conversation

**Need:** "The Guide helps turn my developing intent into appropriate action,
and I can reshape it without becoming its task administrator."

Work:
1. Trace the capabilities needed by real journeys: inspect relevant resources,
   prepare/revise a scoped proposal, explain readiness, and connect the owner to
   existing execution controls. Fill actual gaps with small typed operations.
2. Let the model choose when and in what order to use those capabilities. A small
   request may need no plan; an uncertain problem may need more discussion or an
   authorized investigation. Don't implement a universal resource-check-to-plan
   sequence or make "let's do it" a magic command.
3. Translate corrections to the relevant current proposal. Preserve unchanged
   decisions and provide a concise change explanation. Recheck revisions and
   current grants; ambiguous or stale changes must not affect another effort.
4. Preserve existing approval/start and control authority. Running/accepted work
   remains governed by its execution receipt; only supported steering/continuation
   operations may change it. Clearly explain unsupported changes and unknown effects.
5. Preserve prior consent within its scope, and raise only concrete missing access
   or consequential decisions. Never treat discussion notes as that consent.

**UI:** one identifiable current proposal in the existing conversation, with
details/history secondary. Changes can begin in ordinary language; necessary
approval shows the actual target, scope and consequence. Avoid duplicate plan
cards and trips through task-field editors for routine conversational corrections.

**Acceptance:** build and revise an approach without predefined wording; approve
the applicable work; independent agents actually progress concurrently. Change a
pending proposal, challenge a result and request a bounded continuation. Stale
edits, double submissions and vague targets produce no duplicate or wrong-target
execution. Keep stop controls visible and report their acknowledged effects.

**Owners:** Guide adapters, proposal/continuation services, existing controller,
managed execution, approvals and resource services; PlanReview/inspector.
**Dependencies:** GPR-002/003. **Effort:** M–L; no new execution authority.

## GPR-005

### Have meaningful developments noticed between your messages

**Need:** "I don't have to keep asking whether something needs my attention."

Work:
1. Connect the bounded signal set in [the plan](plan.md#bounded-event-awareness-for-this-sprint)
   to existing durable work/usage/connection evidence. Record source identity,
   affected work/context, freshness and a deduplication key. In-memory telemetry
   alone is not a reliable activation source.
2. Associate each signal with the correct subject/effort before supplying context.
   Batch repeated symptoms of one incident. Reconcile current state before acting
   on delayed events; a recovered connection should not create a stale alarm.
3. Run bounded Guide assessments through existing managed admission, usage and
   cancellation. Persist activation/receipt state as required for replay/restart.
   Delivered background capabilities are scoped inspection and an update; they
   cannot mutate discussion notes or proposals. Preserve this stricter ceiling
   even when event content asks for more.
4. Let the model decide relevance and a helpful response. Supported outcomes include
   quietly recording a resolved incident, inspecting its impact, or surfacing a
   concrete human decision. These are examples, not mandatory scripted branches.
5. Bound repeated reactions, backlog and spend. Reactions must not recursively
   wake themselves through their own notes/events. Put foreground human exchanges
   ahead of background analysis; independent worker execution must continue.
6. Make automatic assessment enablement, allowance and suppression understandable
   in existing Guide/workspace settings. Zero/exhausted allowance prevents model
   activation while deterministic engine enforcement and visible state remain.

**UI:** a concise update attached to its effort, with impact and any needed action.
Background developments don't replace the active subject, steal focus or scroll
the person away from what they are reading. Existing Needs you/inspector surfaces
show the same record; urgent pending decisions remain reachable. Explain when
updates are paused because the local engine is stopped or unavailable.

**Acceptance:** repeated failures from one connection produce one incident, not
an alert/model-call storm. After browser closure/reopen and engine restart, a
pending item is still represented without duplicate accepted reactions. A budget
limit is enforced even if the Guide is unavailable. Recovered, stale, foreign and
self-generated events do not cause inappropriate activity. A human message is
accepted while a background assessment is outstanding; unrelated agents progress.

**Owners:** Guide attention use case; existing authoritative event/state readers,
store cursors/receipts, managed activation and usage. Trace gaps before selecting
record/module locations; no browser scheduler or parallel runtime.
**Dependencies:** GPR-002/004 and grounded presentation from GPR-006. **Effort:** L.

## GPR-006

### Understand ongoing work and return to useful results

**Need:** "I know what is moving, what needs me, and what I can use."

Work:
1. Ground status questions in the relevant work and resource records. Explain
   progress, blockers and consequences; distinguish this effort from workspace
   totals and completed assignments from a satisfactory outcome.
2. Associate results, human requests and noteworthy changes with their effort and
   current understanding. Offer concise context on return rather than a log dump.
3. Connect answers to the same work, agent, source and artifact identities used by
   the map/inspector. Provide evidence and limitations when a result is questioned.
4. Preserve the current draft, subject and location through inspection, approval,
   agent editing and returning to the conversation. Show actual operation/failure
   state rather than a timed sequence of reassuring labels.

**UI:** answer first, implications/decision next, inspectable detail underneath.
Use the existing Guide, map, inspector and human-request views; no separate status
dashboard or compulsory "while you were away" modal. Don't make reading an update
count as approving or resolving its underlying request.

**Acceptance:** oversee two concurrent efforts, inspect a dependency, answer a
human request, return after work finishes and question one conclusion. The person
can find the actual deliverable and supporting evidence without reconstructing
the transcript. Reconnect preserves unread/unresolved distinctions and reports
stale/unknown state honestly. Keyboard and reduced-motion paths remain usable.

**Owners:** authorized work/result/usage projections and current Guide/UI surfaces.
**Dependencies:** baseline can begin immediately; GPR-002/004 for the full journey.
**Effort:** M.
