# Sprint 1 — Durable resources and authenticated control

## REC-101: Reconcile fleet into persistent identities and definitions

Depends on: REC-001/002. Touchpoints: fleet_api, fleet types, identity store, coding definition compiler, database migrations.

Implement organization/team/agent/revision repositories. Add explicit legacy-organization migration, owner/grant references, per-agent identities and immutable definition digests. Desired and observed states are separate. Remove arbitrary creation token charge and Running-on-create. Persist activation intent with a recoverable dispatch record.

Acceptance: restart retains resources; duplicate requests are idempotent; two organizations cannot access each other's resources; two coding agents have different identities; migration preserves existing identity/run histories and has backup/export instructions. No blanket reassignment of old records without recorded mapping.

Retirement: D02 and D11 after caller migration. D01 replacement starts here; final deletion waits for sprint-2 execution proof and sprint-3 operator consumer cutover. Preserve temporarily required request DTOs as adapters only; the new path has no dual-write fleet authority.

Migration acceptance: rehearse backup/restore; preserve immutable admitted revisions during updates to the agent's default definition; retain original history ownership mapping. Define supported rollback versions and reject incompatible old writers. Test update while work is queued, running and awaiting approval.

## REC-102: Expose common control services through one API

Depends on: REC-101. Add authenticated create/inspect/update/activate/cancel and definition APIs with tenant context, bounded requests, versioning and idempotency. Establish local bootstrap trust and refuse unauthenticated remote access. CLI becomes an early API consumer; UI/MCP need no privileged alternate door.

Acceptance: an unauthenticated request fails; cross-tenant IDs fail; activation returns a durable ID and pending state; request retry does not create two activations. Persist state before acknowledgment. API request tests invoke real repositories rather than only mocking FleetManager.

Keep a restricted single-organization development exposure profile until full boundary isolation passes. Bound pending activations, request sizes and controller retries; reject duplicate control-plane ownership of a store. Authentication is not evidence that downstream tools and artifacts are tenant-isolated.

Commit boundary: migration/repositories; service/API; old dispatcher removal. Exit: real durable control plane, with execution pending until sprint 2.
