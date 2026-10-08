# BASE-006 — Durable state, budgets and public contracts

Date: October 8, 2026. Status: complete.
Scope: step 6 of the architecture tidy-up, based on `539c90b6`.

## Delivered

- Organized the existing `tetonic-memory` implementation into `control`,
  `execution`, `context`, `usage` and `artifacts`. Moved 63 root modules and seven
  child files; retained public root type exports and store methods. Mechanical
  comparisons of all moved implementations found only module qualification,
  equivalent crate visibility and formatting changes. SQL, schema version and
  transaction boundaries are unchanged.
- Documented the authoritative records and cross-area atomic operations. Traced
  funding, delegation, reservation, provider enforcement/reporting, settlement,
  cancellation holds and resumption fences through the existing implementation.
  Replaced the stale storage README and updated current architecture links.
- Added one typed candidate-artifact v1 contract shared by the existing two
  encoders and the authorized result reader. Preserved historical payload bytes
  and content digests; reject ambiguous/unsupported payloads. Managed acceptance,
  context authorization and digest verification remain prerequisites to display.
- Added a versioned safe failure envelope with stable categories and recovery
  guidance. Preserved the existing `error` field. Resource conflicts/denials and
  storage failures keep their categories across the application boundary;
  agent/team/brief/plan edits retain their specific conflict messages.
- The local HTTP adapter maps application categories to appropriate statuses
  instead of returning 400 for everything. Added an optional v1 request header
  and response version header; incompatible/duplicate versions fail before a
  command runs. Updated the existing web client to retain error metadata and
  handle malformed/legacy/version-mismatched bodies without automatic replay.
- Budget-setting requests now require an explicit `token_limit`: omission cannot
  silently clear a saved limit; `null` remains the explicit reset operation.
- Bound work-list entries use the existing authorized task projection for actual
  execution status and agent assignment. Saved presentation edits cannot override
  those facts. The summary path skips transcript/artifact payload reads; unexecuted
  work retains its manual presentation status.
- Updated architecture gates for moved source paths and added a regression test
  proving other persistence areas cannot directly mutate the run journal.

The code-backed guide is
[Durable state, budgets and public contracts](../../../../architecture/durable-state-and-contracts.md).

## Validation

| Check | Result |
|---|---|
| `tetonic-domain` | 29 passed, including candidate payload compatibility and strict decoding |
| `tetonic-memory` | 184 unit + 3 concurrency tests passed, including every migration marker, abrupt process exit, competing reservations and resume accounting |
| `tetonic-app` library | 164 passed; 4 existing cases ignored, including opt-in fixtures and a child-process helper exercised by its parent |
| Application integration | 78 passed across `work02_execution`, `work03_door`, `workfin01_finalization`, `workfin02_terminal`, `obs02_envelope`, `obs02_finalization` |
| Managed execution | 47 managed-service cases + 1 execution-claim case passed |
| CLI | 10 unit + 6 control integration cases passed, including required budget fields, failure categories and version guards |
| Architecture gate | 92 passed, including the new grouped-journal boundary case |
| Web client | TypeScript check passed; 4 client cases passed, including conflict metadata and no mutation replay |
| `tetonic-arch-gate verify package` | Passed formatting, workspace/all-target Clippy with warnings denied, architecture and static quality checks |
| Diff and documentation | Current architecture/crate entry links resolve; moved implementations compared against baseline; `git diff --check` passed |

Total: 618 passing tests in the listed suites. This is not a full workspace or
full browser test run. Tests use local fixtures and temporary stores; no paid
inference or live database was used. The live engine was not restarted.

## Preserved limits and next step

This is a storage organization and contract improvement, not a new database,
ledger, execution service or UI redesign. No dependencies or schema migrations
were added. SQLite remains local durable storage. Token accounting remains a
reported-usage guardrail; unknown usage is held, and automatic reconciliation,
monetary budgets and distributed accounting remain future work.

Public failures and candidate artifacts now have explicit contracts. This does
not claim that every historical JSON body or string-valued projection has been
converted. Work shaping, run ownership, effect governance and telemetry retain
their existing authorities. Historical audit documents retain dated source paths.

Step 7 is contributor navigation and enforcement across the remaining package
and frontend boundaries. It has not been started in this delivery.
