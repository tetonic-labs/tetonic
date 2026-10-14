# Observation sizing and delivery qualification

Date: October 14, 2026. Follow-up to
[the failed source-reading project retest](source-reading-project-retest-2026-10-14.md).

The acceptance target remains real production and verification of deliverables
by the six-agent, twelve-assignment project. Starting workers concurrently,
staging a write, or passing a scripted boundary test does not meet that target.

## Implemented boundaries

- `tetonic-core` sizes a batch through the optional domain
  `ToolHost::limit_observation` hook. Local file reads and searches opt in;
  composite application/MCP/skill hosts forward local tools without treating
  external tools as pageable. Scope, argument validation, authorization and
  execution still use their existing owners.
- The nominal batch allowance is 6,144 bytes with estimated per-result framing
  and a useful small-page floor. Individual file/search limits still apply.
  This does not cap complete conversation size, all tool output, or memory use.
- File pages retain byte/hash continuation. A short heading before a long
  paragraph no longer wastes most of the requested page.
- Search returns bounded excerpts around a match, with path and line number.
  `max_bytes` defaults to 2,048, caps at 4,096, and requires at least 256.
  Clipping/omission is explicit. Small results retain their existing format;
  callers can narrow the query and read the relevant source range.
- Post-edit previews have a 1,024-byte body limit, including long Unicode lines.
  A preview is not independent verification of the generated artifact.
- Short per-call budget feedback includes approximate remaining full-context
  calls. The planner distinguishes a per-run ceiling from the aggregate of
  multiple assignments on one agent. Neither changes actual ledger limits.

The implementation keeps observations in ordinary history and preserves native
provider call/result pairing. It introduces no second executor, usage ledger,
permission path, compaction store or UI.

## Rejected approaches and observed failures

An initial budget-aware policy refused reads when estimated completion headroom
was small. The model retried, spending more tokens than a small observation
would have cost; it also broke low-budget integration fixtures. That refusal
was removed. A second variant progressively shrank pages with remaining tokens.
Live traces showed extra calls reconstructing paragraphs. It was also removed:
the final batch sizing is stable and independent of remaining token allowance.

The test specialists initially lacked source search. The existing `grep` grant
was added through the ordinary agent-edit API for their existing test folder.
One search then returned about 20 KB of matching paragraphs, motivating the
bounded search formatter. This was an explicit test configuration change, not
an implicit grant for everyone who has `read_file`.

| Trial | Allowance | Calls | Reported tokens | Outcome |
|---|---:|---:|---:|---|
| Read refusal | 32,768 | 5 | 23,735 | Staged a document; failed before inspection/finalization |
| Small-page floor | 32,768 | 5 | 24,054 | Research exhausted usable allowance |
| Search granted, old formatter | 32,768 | 3 | 6,060 | Hosted stream transport failure; one unknown call |
| Bounded search, shrinking pages | 32,768 | 5 | 24,235 | Research exhausted usable allowance |
| Same policy, reviewed larger single-task allowance | 65,536 | 8 | 54,842 | Staged a document; failed before inspection/finalization |

These are successive diagnostic configurations, not a controlled benchmark.
None delivered a committed file. The unknown provider call retains 26,708 tokens
for review; its cost was not treated as zero or manually released. All other
listed attempts settled with no remaining holds and no reported overruns.

The 65,536-token single-task allowance was an explicit change within the existing
384,000 host ceiling. It does not demonstrate completion within the earlier
28,000-token plan allocation. The larger project allowance has not been silently
increased. A budget-sizing decision for the next full qualification was requested.

## Validation

- Application library: 222 passed, five existing ignored.
- Core library: 51 passed, including native batch integrity, preservation of
  task/evidence history, and observation sizing that leaves delivery available.
- Local tools: 70 passed, one existing ignored. Coverage includes Unicode,
  long search hits, small search budgets, actionable locations, partial-result
  markers, invalid arguments, denied scopes and ungranted tools.
- Package gate: formatting, Clippy, architecture and static quality passed.
- Generated package build artifacts were cleaned after a disk-space build
  failure; workspace data, saved agents and credentials were preserved.

## Final single-agent qualification: passed

With stable page sizing, the same saved Setup specialist completed the same
assignment at the explicit 65,536-token allowance. It searched and read real
source excerpts, wrote `verified-kit/observation-stable/setup.md`, inspected the
entire staged document with `read_file` (lines 1–41), and then called `finish`.
The normal transaction finalization delivered the file into the workspace.

- Task: `b1f7abca-e10c-4c61-80e4-8ce32b7bdaf7`.
- Eight model calls; 54,938 input + 1,565 output = **56,503 reported tokens**.
- Zero pending/unknown calls, zero remaining holds, no reported overrun.
- Delivered document: 328 words; four local source links, all resolving.
- The agent explicitly distinguished inspected documentation from installation,
  browser, live engine and provider acceptance tests it had not performed.
- The test operator independently checked that the committed file exists,
  recorded its hash and checked the links. This is documentation inspection,
  not browser acceptance or proof that installation instructions execute.

This is one observed success, not a reliability benchmark. It does not prove
completion under the former 28,000-token assignment allocation. The final stable
policy was tested at 65,536; no claim is made that it succeeds at 32,768.

The full twelve-assignment acceptance remains open. No larger-budget project
has been dispatched while its budget choice is pending. Its acceptance still
requires real artifacts, independent validation and the repair/revalidation
chain. The historical failed project runs remain failed.

Raw trial task/usage snapshots are retained under the ignored local profile
`.lokai/project-width-87372f80/observation-*`. They are not published in Git.
