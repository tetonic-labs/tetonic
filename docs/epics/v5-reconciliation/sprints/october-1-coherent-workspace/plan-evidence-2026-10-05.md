# Work-plan review evidence — October 5, 2026

This delivers the saved-brief-to-proposal part of OCT-103 in the sole team
workspace. It does not close OCT-103, COORD-A/B/C, or Sprint 1.

## Product path

Open an existing shaping discussion, save a Working brief, then choose Work
plan. The Guide proposes assignments from that explicit brief and the registered
working-agent roster. Review the actual result, adjust its approach, assignments,
owners, dependencies, requested tools and proposed token allocations, then save
a revision or agree to the direction. Agreement records intent; it starts no
workers, reserves no budget and grants no tools.

The map keeps the real planning run attached to its originating work item.
Blackboard and the plan inspector retain the original model reply. Failed or
malformed output stays visible instead of becoming a fabricated plan. The UI
preserves edits and uncertain command identities, detects revision conflicts,
and requires explicit review before rebasing an edit. There is no alternate
planning screen or sample controller.

## Integration and authority

- Schema v54 extends the existing `huddle_proposals` table; legacy huddle data
  survives migration. Scoped reads and mutations use ResourceService and the
  existing store's membership checks.
- Generation uses the registered Guide, existing execution grants, managed
  run admission, broker inference, audit and artifacts. Only the pinned brief
  and bounded agent descriptions enter the request; private shaping transcripts
  are excluded. The request identity and composed prompt are durable.
- The existing inference response-format capability is now reachable through
  a host-selected runtime setting. Structured-answer runs advertise no tools
  and refuse unexpected tool calls before dispatch. Ordinary runtime activation
  fingerprints are unchanged when no response schema is supplied.
- Capture requires the exact recorded generation, completed managed result,
  Guide identity, exploration purpose and no conversation parent. It validates
  the strict schema, current agent references, unique keys, dependency graph,
  text bounds and proposed allocation sum. Model output never grants authority.
- Revisions use compare-and-save semantics. Agreement requires the latest draft
  and current brief. Reads and agreement cannot launch work. The legacy title-only
  huddle acceptance path rejects these structured proposals.
- A discovered delegated-work fallback could submit a child as an independent
  root run. That path now refuses delegated activation. Root execution remains
  available; governed children remain gated on COORD-A.

## Automated evidence

- Memory: 134 library tests passed, including huddle persistence, scope checks,
  migration, invalid DAGs, budgets, changed retries, stale briefs and agreement
  without dispatch. The three huddle tests also passed after the generation-to-work
  mapping change.
- Application: all 189 library tests passed. The planning test exercises real
  managed admission and broker transport with a protocol fixture, asserts the
  JSON schema and empty tool advertisement, excludes a private-history canary,
  captures output, agrees without worker dispatch and restores after reopening.
  The activation regression proves a delegated child cannot use root fallback.
- Core: all 47 library tests passed. Structured-answer coverage verifies provider
  formatting and rejects an unexpected tool call without producing a tool result.
- Inference: 134 library tests passed; two performance benchmarks were ignored.
  The managed planning integration also verifies the Ollama request's explicit
  `think: false` default for a tool-free structured answer.
- Local HTTP adapter: all five targeted tests passed, including strict plan
  command rejection of execution authority and dispatch fields.
- Web: 98 tests across 20 files and the TypeScript/production build passed.
  Plan tests cover proposal/review/agreement, retained retry identity, mismatched
  receipts, stale edits and absence of generation on reads. Projection coverage
  attaches real planning activity to its source without invented worker edges.
- CLI development build and `git diff --check` passed. No claim is made here
  about repository-wide release gates or fresh-user comprehension testing.

## Live verification and remaining work

Live verification uses the existing local database and qwen3.5:latest through
the normal team workspace. The first proposal returned prose instead of a plan;
capture rejected it and preserved the original reply. That exposed the need to
connect the existing provider response-format contract rather than rely on prompt
wording alone. Two subsequent attempts hit the existing 120-second deadline;
these remained failed and did not produce plans. Shape-only provider constraints
avoid a needlessly large decoding grammar, while all bounds remain checked in
the engine. A local diagnostic also showed the model's default thinking consumed
a 64-token response without answer content. Tool-free structured requests now
default to thinking disabled unless the host explicitly configured otherwise.

The fourth proposal completed through the ordinary registered runtime. It used
saved brief revision 4 for Copper Lantern and produced three real assignments,
dependencies and four unresolved data questions. Capture validated and stored
plan revision 4; the UI shows **Proposed · not started**. The proposal was left
unagreed. No worker assignments were launched. The developer verified the
contract and rendering, not the quality of this proposed research approach or
its execution. The model proposes another preliminary planning step and a
data-dependent review; those choices are editable model output, not canned logic.

Screenshot: `.lokai/ui-consolidation-check/team-work-plan-2026-10-05.png`.
Reopening reads the stored proposal without generating another run. The product
keeps long rationale behind Approach and detailed instructions behind each
assignment, retaining the existing map and brand.

Still open: governed plan dispatch, shared effort reservations, inherited grants,
parent stop propagation, recovery reconciliation, dependency scheduling, bounded
agent help/interruptions, actual worker contribution/resource activity on the
map, executable skills and durable missing-capability resolution. Proposed
token amounts are not enforcement. The current local profile is single-owner;
it is not proof of distributed organization operation. Plan history and local
receipt recovery cover the latest 50 revisions; older retries fail closed.

Changes remain local in the existing mixed worktree. No unrelated changes were
committed or pushed as part of this slice.
