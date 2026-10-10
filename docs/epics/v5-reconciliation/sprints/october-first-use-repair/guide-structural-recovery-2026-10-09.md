# Guide proposal diagnostics and loop recovery — October 9

EXP-004/009 repair slice. The complete Guide → useful team result journey is not
accepted by this change.

## Real-model baseline

Isolated workspace: `.lokai/guide-journey-20261009-a`. No hosted providers,
external tools or changes to the original first-use workspace. Its temporary
engine was stopped after retaining the database, transcript/proposal projections,
and usage snapshot. Local model: installed `qwen3.5:latest`.

Brainstorming about workshop scheduling completed without a proposal or dispatch.
That first observation used the previously built binary; proposal testing then
used a fresh build of `87217705` through the normal local HTTP product API.

Two saved workers were created, each with 4,096 tokens, five steps, 120 seconds and
no external tools. The conversation used the workspace roster, not the saved team
as an enforced roster constraint. The request named those workers, asked for
independent format/scheduling analysis and a concise decision memo, and allowed
10,000 tokens for proposed work. Guide reply allowance remained 12,288 tokens.

The model inspected actual resources, then proposed assignment keys containing
spaces and dependencies referring to agent IDs instead of assignment keys. It
also added a synthesis worker and allocated its entire proposed 9,000-token total
to workers. Structural validation rejected the call without saving or dispatching.
Its generic error did not identify the incorrect field. The reply stopped at its
allowance before producing a valid proposal.

Recorded proposal-turn usage: 12,365 input + 2,940 output tokens; held allowance
and unknown calls settled to zero. Already-admitted inference can report usage
beyond an allowance; this is not evidence of a strict provider-side token cap.
The trial contains actual model output, not a supplied plan or scripted answers.

## Changes and ownership

- Existing plan schema explains assignment IDs, saved agent keys, dependency
  references, independent work and the existing coordinator's synthesis role.
- Existing `PlanContent` validator returns field-specific errors for key syntax,
  references/cycles, text bounds, duplicates and budgets. Accepted shapes and
  authority checks are unchanged. Error paths do not echo arbitrary private text.
- Core runtime argument rejections and failed asynchronous host controls now
  participate in the existing consecutive no-progress cutoff. They previously
  bypassed it. Corrected calls can proceed; successful control reads reset the
  failure streak, including repeated reads with changing system state.
- The Guide binding retains detailed argument errors. The work inspector explains
  a no-progress stop as repeated unsuccessful tool calls and directs the owner to
  activity, rather than suggesting a connection problem.
- A quoted/stringified plan is rejected with an explicit object-versus-string
  diagnostic rather than a deserialization excerpt of the submitted plan. The
  advertisement now spells out that requirement as well.

No separate planner, scheduler, UI surface, new authority or marketplace was added.
The map and Guide design are unchanged.

## Automated evidence

`cargo test -p tetonic-memory -p tetonic-core -p tetonic-app --lib --locked -j 2`:
**436 passed, 5 opt-in tests ignored, 0 failed.** CLI build passed.
After the quoted-plan diagnostic, the 14 Guide tests were rerun and passed.

New cases cover detailed managed proposal feedback, correction to one saved
proposal/brief without dispatch, stopping repeated malformed/host failures before
the step cap, successful correction, mixed failure paths, repeated successful
host reads and safe failure text. Existing parallel dispatch, dependency,
idempotency, scope, provider parity and contribution-delivery regressions passed.

## Focused repaired-build check

`.lokai/guide-journey-20261009-b` used a fresh database and the repaired binary,
with the same two worker roles and local model. Unlike the baseline, the saved
two-agent team was explicitly pinned at conversation creation, the full request
was supplied in one turn, and the host was bounded to six steps/240 seconds. This
is a focused check, not a like-for-like replay of the brainstorming sequence.

The model read resources and generated syntactically valid assignment IDs and
dependency references, but encoded the whole plan as a JSON string. It still
included an extra synthesis assignment and no coordination headroom. The call
was rejected; no proposal or execution was created. The reply hit its token
allowance: 11,589 input + 2,174 output tokens, with no outstanding holds or unknown
calls. The temporary engine was then stopped and the data retained.

The quoted-plan diagnostic was added after this trial and checked in automated
tests; there was no third real-model trial. Neither trial qualifies the useful
team-output journey. Do not attribute this failure solely to model capability:
the number and size of engine-generated model exchanges also need attention.

## Still open

- Actual-model decomposition and useful combined output remain unproven; both
  recorded local-model trials stopped before dispatch.
- Guide prompt/context and repeated inference input can consume its allowance
  before a useful handoff. Measure and reduce overhead; do not silently raise user
  limits or treat this failed trial as successful.
- Durable allocation correction still permits one attempt. Structural errors now
  have a runtime consecutive-failure cutoff, not a unified durable one-correction
  policy across all malformed shapes and interleaved reads. That requirement is
  not fully closed.
- Complete the real reviewed-plan → independent agents → combined result path,
  then a concurrent second effort and owner review. EXP-004/009 remain open.
