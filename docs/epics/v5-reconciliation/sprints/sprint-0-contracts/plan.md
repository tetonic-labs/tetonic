# Sprint 0 — Contracts and characterization

Historical REC work package. Scheduling and scope are superseded by the [MVP sequence](../README.md); retain applicable technical safeguards as reference.

## REC-001: Resolve identities, execution objects and authority

Depends on: audit. Touchpoints: domain identity/run/placement/work-scope contracts, app definition/identity-job, run managed contracts.

Deliver an ADR mapping Organization/Team/Agent/Definition/Activation to existing Identity/Run/Task/Attempt types. Define activation concurrency, definition updates, child execution, current-policy revocation and recovery profiles. Decide initial authentication and supported compatibility matrix. Define the trusted request/execution context and error vocabulary.

Acceptance: every target box has one authority; no second run store; two agents can share a definition without sharing identity; sessionless and conversational work both map; no raw credentials in definitions. Record deferred HA explicitly.

Resolve executor thread affinity/LocalSet hosting, revision-update behavior under existing identity equality checks, single-writer enforcement, bounded activation admission, schedule catch-up, and speculation defaults. Generic effects default to no concurrent speculation. Define cancellation acknowledgment versus quiescence and escalation before changing execution interfaces.

Define the versioned operator configuration contract now: telemetry, logging and storage are mandatory configurable surfaces. Specify supported backends, config precedence/validation, secret references, reload versus restart, dependency-failure behavior and distributed topology constraints. Follow [operator requirements](../../operator-configuration.md); implement settings with their owning subsystem rather than waiting for sprint 5.

## REC-002: Preserve useful behavior and identify obsolete tests

Depends on: REC-001 for target expectations. Capture deterministic fake-provider coding execution and external-world protocol scenarios, current recovery/quarantine behavior, capability denial, approval and cancellation. Inventory structure-only tests separately from behavior tests. Record current failures without changing production to conceal them.

Acceptance: runnable commands and baseline results; source/caller maps for each D item; CI scope identifies which packages are omitted today. Mark unsupported configuration modes as unimplemented in product documentation. Do not delete protocol/recovery tests merely because they refer to earlier release names.

Capture manager-owned completion claim/result sealing and cancellation quiescence as explicit preservation tests. Characterize schema upgrade/restore and definition-update races before implementing their replacements.

Commit boundary: contracts/evidence first; characterization changes second. Exit: implementation tickets have unambiguous state and compatibility expectations.
