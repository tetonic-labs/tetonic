# Sprint 0 — Contracts and characterization

## REC-001: Resolve identities, execution objects and authority

Depends on: audit. Touchpoints: domain identity/run/placement/work-scope contracts, app definition/identity-job, run managed contracts.

Deliver an ADR mapping Organization/Team/Agent/Definition/Activation to existing Identity/Run/Task/Attempt types. Define activation concurrency, definition updates, child execution, current-policy revocation and recovery profiles. Decide initial authentication and supported compatibility matrix. Define the trusted request/execution context and error vocabulary.

Acceptance: every target box has one authority; no second run store; two agents can share a definition without sharing identity; sessionless and conversational work both map; no raw credentials in definitions. Record deferred HA explicitly.

## REC-002: Preserve useful behavior and identify obsolete tests

Depends on: REC-001 for target expectations. Capture deterministic fake-provider coding execution and external-world protocol scenarios, current recovery/quarantine behavior, capability denial, approval and cancellation. Inventory structure-only tests separately from behavior tests. Record current failures without changing production to conceal them.

Acceptance: runnable commands and baseline results; source/caller maps for each D item; CI scope identifies which packages are omitted today. Mark unsupported configuration modes as unimplemented in product documentation. Do not delete protocol/recovery tests merely because they refer to earlier release names.

Commit boundary: contracts/evidence first; characterization changes second. Exit: implementation tickets have unambiguous state and compatibility expectations.
