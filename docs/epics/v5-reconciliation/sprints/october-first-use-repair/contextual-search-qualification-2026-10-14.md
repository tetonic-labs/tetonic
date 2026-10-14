# Contextual source search qualification

Date: October 14, 2026. Follow-up to
[the observation sizing qualification](observation-budget-qualification-2026-10-14.md).

## Product purpose and implemented change

Workers need enough source context to produce and inspect their deliverables.
Returning only matching lines often requires another model request to fetch the
adjacent instructions. The existing granted `grep` tool now accepts
`context_lines` (0–12, default 0). This extends the local source-tool owner; it
adds no execution path, context service, memory store or UI.

- Matches and neighboring lines come from the same authorized live-file read.
- Overlapping ranges merge and keep path/line labels. `:` marks matches and `-`
  marks context. The actual match takes priority over nearby lines when space
  is tight. Small searches without context retain their previous format.
- Context shares the existing byte allowance: 2,048 by default, at most 4,096.
  The core's existing batch sizing still applies. Clipping, omitted matches and
  an early-stopped directory scan are reported as incomplete. A scan stopped at
  exactly the requested match count no longer implies all files were searched.
- `max_results` is validated as 1–1,000 matching anchors (default 100). Context
  may expose additional matches next to those anchors. File scanning can still
  load whole files; these are observation limits, not a total memory bound.
- Saved grants, credential/control-file exclusions, no-follow reads and workspace
  boundaries remain in force. Search still sees live files, not staged edits;
  the tool description directs agents to `read_file` for staged inspection.

## Validation

The local-tool library passed **76 tests**, with one existing ignored test.
Coverage includes adjacent instructions in one observation, merged overlaps,
Unicode/long lines, small byte budgets retaining the match, independent file
locations, result-count omissions, early-stopped scans, argument bounds, private
file exclusions, denied grants and staged-file isolation. The package quality
gate and CLI build passed.

## Hosted trial: failed delivery; efficiency gain not established

Task `902a162d-8b5e-4b92-8db5-ce92402bdb18` used the same saved Setup specialist,
model, assignment, instructions and 65,536-token allowance as the prior successful
trial. Only its output location changed to `verified-kit/context-search/`, and
the engine advertised the new search capability. No completed output was supplied
to the agent. The 384,000-token project ceiling was not increased.

The real model used `context_lines` in five searches. It also requested several
file pages, staged a document, and rewrote it. Admission then rejected the next
model request because estimated context plus minimum output no longer fit the
remaining allowance. It never inspected the draft or called `finish`.

- Seven model calls: 43,787 input + 2,055 output = **45,842 reported tokens**.
- No overrun, pending call, unknown call or remaining hold in this trial.
- The unused 19,694 tokens were released through normal settlement.
- **Zero committed files.** Staged drafts were discarded by normal failure
  handling; they are not delivered work or verification evidence.

This does not establish lower total cost or greater completion reliability. A
smaller token total for a failed attempt is not an efficiency win. The prior
56,503-token successful attempt and this failed attempt are individual model
runs, not a statistical comparison. The final early-scan completeness marker was
added after this hosted trial and verified by local tests; no second paid trial
was run for that metadata correction.

## Remaining acceptance

The six-agent, twelve-assignment project remains unqualified. Contextual search
is now available and tested, but it did not solve cumulative-context exhaustion.
The next investigation should examine delivery/inspection headroom and retained
request size together, without weakening accounting, silently dropping required
evidence or automatically increasing the project budget. Qualification still
requires actual committed artifacts and the independent validation/repair chain.

Raw local trial evidence is retained in the ignored qualification profile at
`.lokai/project-width-87372f80/context-search-*`.
