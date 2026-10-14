# Live project retest: context and handoff failures — October 13, 2026

**Live project acceptance failed.** The prior budget changes allowed safe parallel
execution, but this project did not deliver its requested files. Two specific
defects found during the run are corrected in code; a successful live project
with 5–10 agents and 10–30 assignments remains an open MVP requirement.

Follow-up: [bounded source reading and the October 14 retests](source-reading-project-retest-2026-10-14.md).
The reader was corrected, but the live project still did not qualify.

## What actually ran

The existing isolated profile on port 18740 retained its database, saved agents,
workspace boundaries and historical usage. It was restarted with the current
engine. Six saved specialists each had a 32,768-token personal ceiling; the
host retained its 384,000-token project ceiling and seven execution slots.
Only existing local file tools were granted. The independent reviewer also
retained its existing governed shell tool. No new network access was added.

Through the real connected Guide UI, we requested a bounded offline operator kit
based on snapshots of the current repository: setup, capabilities, coordination,
budget recovery, an HTML presentation, independent source review, exercises,
integration and validation. The model authored the plan and selected agents.
The normal proposal review and start flow admitted the work. No plan, tool
result or completed artifact was inserted directly into storage.

| Observation | Result |
|---|---|
| Saved model-authored assignments | 12 |
| Peak simultaneous workers | 6 |
| Completed assignments | 0 |
| Failed assignments | 5 |
| Canceled assignments | 1 |
| Dependency-gated assignments never started | 6 |
| Files delivered in the new output directory | 0 |
| Reported model tokens, including Guide planning and coordination | 83,123 |
| Reported model calls | 20 |
| Remaining holds / unknown calls / pending calls for this run | 0 / 0 / 0 |
| Reported allowance overruns for this run | 0 |

The Guide saved the 12-assignment proposal before its closing reply failed
budget preflight. The UI retained the proposal and explained that it was saved
before the reply ended. The agreed project allocation was 335,872 tokens:
303,104 across workers and 32,768 for coordination. Worker allocations were
16,384–32,768 tokens; these include repeated input, not only generated output.

Five workers read large source batches and then could not afford the next model
request. For example, the capabilities worker accumulated roughly 50 KB of
tool results after three calls and 15,249 reported tokens against its 24,576-token
assignment share. The HTML worker asked the human for filenames already present
in peer assignments, because those contracts were missing from its own input.
The stranded root was canceled through the normal task API. Dependency-gated
work remained unstarted; the run's unused holds were released through normal
settlement. Previous trials' unknown usage was retained.

## Corrections made

1. **Count native continuations once.** Request sizing previously counted both
   the portable assistant message and its native provider representation. The
   Responses, Anthropic and Google adapters send the native representation in
   place of the portable message. Sizing now counts the larger once for matching
   native assistant state; unknown protocols/model mismatches retain conservative
   addition. Private native state is still included. This removes a source of
   premature preflight rejection without changing transport validation, actual
   usage accounting, or limits. It does not prove the original Guide closing
   reply or the five failed workers would now succeed.
2. **Give workers the agreed output contracts.** The existing plan execution
   service now includes assignment keys, owners and deliverables from the agreed
   plan. Independent workers can see their peers' filenames/interfaces without
   needing the human to repeat them. Peer instructions, private conversations
   and future results are not included. Actual dependency evidence still follows
   declared dependencies; contracts add no permissions or waiting requirements.

Both changes extend existing owners. There is no new orchestration service,
memory system, execution path, database migration or UI redesign.

## Qualification evidence

Focused inference tests cover all three native continuation protocols, model
mismatch fallback, non-ASCII text, private state, tool results and schemas.
The existing managed two-worker integration test checks that an independent
worker receives its peer's reviewed deliverable but not peer instructions or
future results. The 25 active plan tests pass, including six workers with 20
assignments and the three-slot queued variant. These use scripted providers;
they are not evidence of successful live deliverables.

- Inference library: **157 passed, two existing ignored**.
- Application library: **222 passed, five existing ignored** with four test
  threads. The initial default-concurrency run had one fixture model-discovery
  failure in the foreground/background Guide test; that test passed in isolation
  and in the full rerun. No production timeout or test assertion was relaxed.
- Package quality gate: formatting, workspace/all-target Clippy, architecture
  and static quality passed. CLI build passed.
- The idle engine on port 18741 was restarted with the rebuilt binary. All 23
  existing work IDs/states and five saved agent configuration digests matched
  before and after restart. No additional paid call was made after the failed
  project was stopped. The two corrections have deterministic coverage; a new
  live project has not yet qualified them together.

Source discussion: `0870439e-9711-4d80-a605-1057d7ccda75`.
Execution root: `9a75f811-5d12-4a7e-86c5-676c3b2c6a67`.
Local raw evidence is retained under `.lokai/project-width-87372f80/` as
`budget-live-*.json` and `budget-live-samples.jsonl`. Those ignored records contain
real prompts and tool traces and are not published in Git. Source snapshots have
a retained SHA-256 manifest. The run's output folder was
`verified-kit/preview-0870439e/` within the isolated workspace.

## Remaining product blocker

Workers can still load more source material than their remaining repeated-input
allowance can support. Existing file tools offer line ranges, and workers receive
per-call budget feedback, but model cooperation alone did not prevent this run's
failure. General-worker context management needs a bounded way to consume large
observations while retaining provenance and retrieving omitted content. That
belongs in the existing tool/runtime context path, with explicit incomplete-output
handling; it should not silently truncate evidence, invent completion or bypass
the ledger. Repeating this whole project or merely raising its budget would not
establish that the problem is fixed.
