# BASE-004 — Explicit application scope and work-service ownership

Date: October 8, 2026. Status: complete.
Scope: step 4 of the architecture tidy-up, based on `23bf74b5`.

## Delivered

- Extracted work shaping, submission, plans, conversation history, human work
  questions, continuation and inspection into `tetonic-app/src/work/`, owned by
  `WorkService`. Split the former root module into types, submission and inspection.
- Moved agent editing/configuration, provider selection, MCP connections and skill
  operations into scoped `WorkspaceServices` under `src/workspace/`.
- Reduced `local_workspace.rs` to default local identifiers, bootstrap and API
  compatibility reexports. The CLI still uses `LocalWorkspace`, now an alias for
  `WorkService`; existing public payloads and endpoint callers remain compatible.
- Added credential-bound `ApplicationScope` with private principal, organization,
  security-team and participation-context identifiers. Binding verifies the
  credential and team access; service operations retain current resource/store
  checks. The scope value cannot deserialize an untrusted permission decision.
- Removed embedded local owner/org/team identifiers from work and capability
  implementations. Bootstrap chooses local defaults. Work persistence uses the
  existing control store rather than borrowing the provider-key store handle.
- Scoped work notes and presentation metadata by organization/team/work ID in
  schema 70. Added resource authorization, work ownership validation and atomic
  field patches. Product paths propagate metadata failures instead of silently
  presenting empty data or a successful save.
- Added compatibility migration for notes-only and full-field legacy records.
  Only an unambiguous existing work binding is imported. Ambiguous and orphaned
  rows remain preserved in the legacy table, with no guessed team ownership.
- Updated the [architecture entry point](../../../../architecture/README.md),
  [ownership map](../../../../architecture/ownership.md), terminology, host and
  local API documentation. Added the [scoped service contract](../../../../architecture/work-services.md).

## Preserved ownership

The existing `TeamWorkController` remains the scheduler. `WorkService` implements
its host interface and uses the same resource registry, grants, registered job
preparation and managed admission. No second coordinator loop, run journal,
executor or provider-specific tool path was introduced.

Agent revisions, pinned plan inputs, dependencies, budget accounting, parallel
assignment execution, human controls, capability enforcement, broker/egress and
managed finalization keep their existing owners. The application lock covers
admission/configuration changes; it does not serialize model execution.

This is mostly extraction and rewiring of existing code. New behavior is explicit
scope validation and scoped metadata persistence. There are no web changes or new
Cargo dependencies. The live testing engine/database were not restarted or opened.

## Validation

Tests used temporary databases, local fixture servers and the existing lean target
cache. No paid inference or live browser workflow was used.

| Check | Result |
|---|---|
| Full `tetonic-app` library suite | 161 passed, 4 ignored after extraction and metadata error propagation. |
| Final `work::scope_tests` suite | 3 passed, including one additional managed execution journey added after the full run. |
| `tetonic-memory` tests | 184 unit and 3 shared-store concurrency tests passed, including legacy metadata upgrades, ambiguity preservation and schema rollback/crash coverage. |
| `tetonic-cli` tests | 8 unit and 6 separate-process operator tests passed. |
| `scope04_boundaries`, `comp01_pins`, `work02_execution`, `work03_door`, `workfin01_finalization`, `workfin02_terminal` | 93 passed. Source assertions supplement behavioral coverage; they do not replace it. |
| `tetonic-arch-gate verify package` | Passed: formatting, workspace/all-target Clippy with warnings denied, architecture and static quality. |
| Documentation/diff checks | Current architecture/API/ticket local links and heading fragments resolve; `git diff --check` passes; no `web/` diff. |

New scope cases check identical work IDs in different teams, non-default
principals, denied foreign access, credential mismatch and revocation, membership
removal and reopen. The managed execution case uses a different principal/team,
the existing engine with a fixture inference server, and a retried request that
must retain its run without a second model call. Its grant must carry the scoped
principal and context, and the default workspace cannot inspect its work.

The four ignored application cases retain their baseline meaning: three opt-in
local-model/scenario fixtures and a child-process helper exercised by its parent.
The initial new scope fixture omitted team-creation permission; that fixture was
corrected and the suite passed. A test-module placement Clippy warning was fixed
without adding a lint exemption. One attempted rebuild while the Windows test
executable was still running hit a file lock; it was retried after completion.

## Limits

The local bootstrap still creates its existing owner, organization/team and
default agents. It does not implement a multi-user HTTP server or expose arbitrary
scope switching. Provider accounts/defaults remain host/database-wide, and local
owner compatibility methods must not be treated as a complete remote permission
model. The synchronous capability views and background work retain their existing
authorization/lifetime contracts; this does not promise instant recall of admitted
external effects.

Schema 70 uses the existing pre-upgrade backup and transactional migration path.
Old writers must not open the upgraded database. Ambiguous legacy metadata remains
recoverable in the old table but is not shown as another team's data. Legacy raw
store methods remain for compatibility/migration; new product code must use the
scoped resource path.

Distributed control/storage, remote execution, provider-account tenancy, changes
to product behavior and the next numbered architecture step are outside this work.
