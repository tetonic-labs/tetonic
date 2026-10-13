# Guide sprint: evidence and product acceptance

Status: **engineering journeys verified**. See the
[implementation and evidence record](evidence-2026-10-13.md) for actual tests,
live failures, repairs and limits. The owner's subjective product walkthrough is
still a release-review activity. Keep fixture tests, live-model behavior and owner
judgment separate. The journeys below remain the regression and review rubric.

## Baseline and repeatable journeys

Use a separate local profile/database. Record commit, running binary, settings,
provider/model, tools/agents, permissions and limits. Do not seed the owner's
workspace or interpret sample wording as representative of a real user's work.

| Journey | What to exercise | Passing product behavior |
|---|---|---|
| A small question | A direct question requiring no environment access | Useful answer; no forced team, setup, proposal or notes |
| Uncertain intent | Explore alternatives, reject one, change a constraint | Substantive help; decisions stay correct; no unapproved work |
| Long discussion | Exceed prior byte and ancestor limits; return after reopening | Continue in place; relevant older evidence remains retrievable; context stays bounded |
| Several trains of thought | A → unrelated B → back to A; connect a relevant observation from C | Correct context without manual filing, private leakage or unwanted work creation |
| Mistaken association | Intentionally correct which subject a remark belongs to | Clear reversible association change; original sources and unrelated work preserved |
| Real work | Develop an approach, edit it in conversation, authorize work | Feasible current proposal and actual governed parallel contributions |
| Concurrent attention | Two efforts; one blocked; another still progressing; user discusses a third idea | Correct effort updates without taking over the discussion or stopping unrelated work |
| Background incident | Repeated failure, budget threshold, recovery and delayed delivery | One meaningful incident, bounded assessment, truthful resolution/unknown state |
| Return and scrutiny | Close/reopen, inspect a result, challenge a claim, request a correction | Brief reorientation, usable output, evidence access and a scoped next step |
| Control | Stop the relevant work, then deliver a late event or model reply | No revival or new work from stale intent; acknowledged stop state remains visible |

Repeat the conversational journeys with unfamiliar phrasing in at least three
domains, including a noncoding problem. Include incomplete statements and mixed
intent. Do not add production keyword rules to make a specific test prompt pass.

For a live model, use a named, bounded provider/profile and record actual usage.
Do not invent a new spending authorization or run an unbounded model comparison.
If a live check cannot be run, label that part unverified rather than substituting
a fake-provider transcript for successful assistance.

## What to measure

Record before/after observations for each journey:

- Times the person had to repeat known context or correct a wrong association.
- Unnecessary clarification questions, filing steps, clicks and editor detours.
- Time to first useful response; foreground responsiveness during background work.
- Unwanted proposals/tasks, duplicated work or unjustified setup demands.
- Whether the person could identify the active subject, current proposal, affected
  work, next relevant decision and delivered result without reading raw traces.
- Model calls, reported usage, context size, event assessments per incident and
  any unknown usage. Better continuity cannot mean unlimited prompt growth.

These observations are exploratory evidence, not statistically established UX
claims. Record model failures and manual repairs; do not report a curated success
as a general capability. Collect the owner's assessment of clarity and effort.

## Non-negotiable behavior checks

- Current scope and grants govern retrieval/actions; association is not authority.
- Human edits defeat stale derived notes/proposals. Source history remains intact.
- Duplicate requests/events and restart do not multiply accepted effects. External
  uncertain effects are reported/reconciled; do not promise universal exactly-once.
- Background work has finite activation, context, concurrency and usage bounds.
  Self-generated events and agent requests cannot form unbounded reaction loops.
- Budgets, cancellation and permission enforcement work without Guide inference.
- Independent work continues during a sibling wait or background Guide check.
- UI activity, map state, notifications and the Guide's claims share real evidence.
- Discussion focus changes, unread/read state and permission/approval are distinct.

Run applicable tests and repository quality gates for each changed owner, then
visually inspect changed interactions using the existing styling. A fixture is
valid for layout/regression testing, not proof of reasoning or successful work.

## Exit record

For each GPR ticket record: implemented behavior and source paths; automated
checks; live observations; owner walkthrough; unresolved limitations; accepted
scope. Leave incomplete or unverified acceptance open. Preserve links to the
earlier [Guide evidence](../october-first-use-repair/guide-system-refinement-2026-10-13.md)
and [large-project proof](../october-first-use-repair/project-width-proof-2026-10-13.md).

This sprint's focused two-effort interaction journey does not replace the separate
5–10-agent / 10–30-task useful-width proof or any existing release gate. No release
readiness claim follows from creating this plan.
