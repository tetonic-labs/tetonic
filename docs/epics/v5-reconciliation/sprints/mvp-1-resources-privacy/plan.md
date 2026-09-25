# Sprint 1 — Durable teams, identities and information boundaries

Status: durable resources, local credentials and the first local operator CLI are implemented; organization and team metadata membership administration, organization-owned agent registration and immutable definition revisions are implemented. Remote authentication/UI and complete execution-context privacy remain pending. Depends on the preceding MVP sprint's applicable contracts; security and bounded admission are enforced incrementally, never deferred until final hardening. Tickets may be split into smaller implementation commits without weakening exit criteria. See [implementation progress](../../progress.md) and the [local control runbook](local-control-runbook.md).

## MVP-101 — Persist and authorize organization, team and agent resources

Implement immutable definition revisions, distinct agent identities, team membership/roles and trusted request context. Build authenticated CRUD with idempotency and a thin team-creation interface early. Separate employee identities from devices and agents. Migrate existing histories into an explicit legacy scope, preserving IDs and admission snapshots.

Acceptance: duplicate creation is harmless; restart retains resources; roles only grant explicit capabilities; definition updates do not silently rebind queued work; no invented token charge or Running status. Test backup/restore and reject unsupported old writers.

## MVP-102 — Implement scoped context and explicit knowledge sharing

Implementation sequence and current source boundaries: [context isolation cutover](context-isolation-cutover.md).

Provide private and team history/knowledge/artifact grants. Retrieval, context assembly, caches, model-session handles, streams and errors all obey the execution scope. Personal-agent team participation creates a separate authorized working context. Explicit publication includes provenance and destination authorization.

Acceptance: place a unique secret in private history; team execution inputs, tools, logs and outputs cannot retrieve it without authorized publication. Membership revocation blocks future retrieval. Guessed IDs and summaries cannot bypass scope. Do not promise privacy against infrastructure administrators without a separate threat model.

Reuse: identity store, artifact/context interfaces, provenance patterns. Retire D02/D11 after cutover; start D01/D15 replacement. Nonlocal multi-tenant exposure remains gated on complete boundary tests.

Commit discipline: characterization/contract, replacement behavior, caller cutover, then gated removal. Record focused tests and remaining compatibility obligations. No production changes were made by the planning ticket.
