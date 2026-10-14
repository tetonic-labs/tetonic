# Bounded source reading and live project retest

**The reader defect is corrected; large-project acceptance still fails.**
Two real hosted-model reruns produced no completed assignments and no committed
deliverables. Parallel workers are not evidence of useful project completion.

## Implementation

The existing `tetonic-tools` file reader now returns bounded, explicit source
observations: 2,048 source bytes by default, with a firm 4,096-byte maximum even
when a model requests more. Large reads include byte coordinates, a SHA-256,
exact continuation arguments and an explicit warning that omitted content is
not evidence. Continuations can reject a changed source using `expected_sha256`.
Byte cursors remain valid through Unicode and very long individual lines.
Existing line ranges remain supported; small reads preserve their prior format.

Each page uses the existing authorized live/staged read path. Workspace,
credential, symlink and transaction boundaries are not bypassed by a cursor.
Private staged changes remain visible only to their owning tool instance.
The generic runtime receives observation completeness through `ToolOutcome`;
partial pages no longer count as whole-file reads, and distinct cursors count
as reading progress. No new executor, storage table, ledger or UI was introduced.

This bounds individual file observations, **not** all observations in a tool
batch, total retained history, or cumulative input usage. The implementation
still loads and hashes the authorized file in memory. It does not paginate
MCP/shell output or implement semantic memory/compaction.

An attempted change that reduced retained conversation history with remaining
tokens was rejected after the first rerun: it lost useful history and contributed
to repeated reads. It is not included in the final code.

## Actual reruns

Both used the preserved isolated profile on port 18740, six saved OpenAI
`gpt-6.1-sol` specialists, twelve assignments, existing granted tools and normal
proposal/continuation/start APIs. No completed output or fabricated tool result
was inserted into the database. The independent reviewer retained its governed
shell grant; no validation command was reached.

| Observation | First rerun | Final hard-cap rerun |
|---|---:|---:|
| Reviewed project allowance | 335,872 | 378,304 |
| Worker assignments | 12 | 12 |
| Failed initial assignments | 6 | 6 |
| Dependency-gated assignments never started | 6 | 6 |
| Completed assignments | 0 | 0 |
| Committed deliverable files | 0 | 0 |
| Execution model calls, including coordinator | 25 | 21 |
| Reported execution input + output tokens | 101,970 | 95,173 |
| Remaining holds / pending / unknown calls after stop | 0 / 0 / 0 | 0 / 0 / 0 |
| Reported allowance overruns | 0 | 0 |

The first run retained the previous scope and allocations, with the initial
2 KB default / 16 KB maximum implementation. Models explicitly requested larger
pages, defeating the smaller default. A worker also repeated reads after the
experimental history reduction. Both issues informed the final implementation.

The final run used the firm 4 KB cap and original history retention. It was a
**revised-scope qualification**, not a controlled reproduction of the old plan:
guides were shortened from roughly 700–1,300 to 250–350 words, while retaining
six initial contributions, exercises, readiness JSON, HTML integration,
independent validation, repair and revalidation. The Guide authored the new
proposal through the existing preparation API. This planning call used another
6,590 reported tokens, separate from the table's execution totals.

Owner review corrected the Guide's mistaken interpretation of a 32,768-token
per-run ceiling as a combined ceiling across an agent's separate assignments.
Through the normal revision API, allocations became 28,000 per assignment and
32,768 for each independent validation assignment: 345,536 worker tokens plus
32,768 coordination tokens. Task instructions, dependencies, tools and output
contracts remained model-authored. The workspace's 384,000 ceiling was unchanged.

The source fixture also needed a correction: all thirteen manifest hashes and
byte counts had been computed before Windows newline conversion. They were
recomputed over the actual supplied file bytes; source contents were unchanged.
The original and corrected manifests are retained in local evidence.

## What failed, specifically

The final trace confirms explicit bounded pages and hash-bearing continuation
calls worked. However, several workers requested multiple pages per model turn.
Those results accumulated in history and made the next request unaffordable
under conservative admission, before the workers wrote their documents.

For example, the setup worker used 12,863 reported tokens over three calls of
its 28,000 allocation. It then held roughly 28 KB of source observations plus
its assignment and tool context; the next estimated input and minimum output
did not fit the remaining allowance. The request was correctly withheld rather
than weakening accounting or silently increasing its budget.

The HTML worker went further: it generated and staged `index-shell.html` and
`html-contract.json`. After four calls and 20,989 reported tokens, it could not
afford its next call. The failed attempt aborted its uncommitted transaction;
neither file became a delivered artifact. We did not restore those drafts by
hand or count them as success. The operator-facing budget error now distinguishes
completed work from uncommitted changes that may have been discarded.

Both stranded roots were canceled through normal controls after their workers
stopped. Independent checking, integration and repairs never executed. There is
no evidence supporting a successful end-to-end large-project claim.

## Validation and remaining acceptance

- Application library: 222 passed, five existing ignored.
- Generic core: 50 passed. Local tools: 65 passed, one existing ignored.
- Source tests cover Unicode/long-line reconstruction, changed-source rejection,
  clamped oversized requests, line-number expansion, invalid cursors, staged
  isolation and authorization checks on continuation.
- The progress-monitor regression distinguishes eight successive pages from a
  repeated whole-file read.
- The application test HTTP fixture now survives an empty canceled connection
  instead of terminating its listener and breaking a separate foreground test.
- Package quality gate and CLI build pass. No production timeout, authorization
  check, usage ceiling or acceptance assertion was relaxed.
- Both idle engines were restarted with the final binary. Port 18741 preserved
  all 23 work IDs/states and five saved-agent digests; port 18740 preserved all
  110 work IDs/states and nine saved-agent digests. Neither restart dispatched
  more model work.

The next blocker is **bounded source consumption over an entire assignment**:
budget for repeated input, output creation and a confirmation/handoff call;
avoid uncontrolled read batches; retain usable evidence rather than silently
dropping history and provoking rereads. The planner also needs realistic
per-assignment allowance guidance. Increasing a single page cap or relying on
an instruction to “read less” is not an adequate resolution. Qualify a bounded
read → write → inspect/verify → finish attempt before paying for another full
project, then rerun the twelve-assignment review/repair chain. That live
acceptance remains open.

First source/root: `e8672c0f-4fd5-4c06-9d0d-50b95b627870` /
`fe80169e-8179-480b-99ca-16192a464d2d`.
Final source/root: `90acfbea-93d5-4716-a41e-b64994b4ea5e` /
`2e29c4de-1e15-439d-9a8e-acb3e4a661bb`.
Raw prompts, task traces, manifests, revisions and settlement snapshots remain
in ignored `.lokai/project-width-87372f80/source-read-*` files. They are not
published in Git.
