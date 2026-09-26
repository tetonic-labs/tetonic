# Sprint 1 — Durable teams, identities and information boundaries

Status: **exited (local MVP preview).** Durable resources, local credentials and the local operator CLI are implemented; organization and team membership, organization-owned agent registration and immutable definition revisions are implemented. A verified backup restores teams, membership, and private history without granting that history to another member. Creating a team or adding a member creates that principal's empty private working context; other members cannot read it; `tetonic job run --team` selects it. Unpublished private history does not enter team execution inputs. Metadata registration does not invent a token charge or Running. Employee-visible failures do not repeat store/tool bodies. The unauthenticated daemon does not snapshot, replay, or cancel a scoped run.

Deferred (not required to exit this sprint; later MVP stages own them): remote multi-user authentication/UI; nonlocal multi-tenant exposure; deletion of the legacy fleet prototype (D01) and the legacy coding-session identity pin (D11) after product cutover; threat model against infrastructure admins who can read the database file. See [implementation progress](../../progress.md) and the [local control runbook](local-control-runbook.md).

## MVP-101 — Persist and authorize organization, team and agent resources

Implement immutable definition revisions, distinct agent identities, team membership/roles and trusted request context. Build authenticated CRUD with idempotency and a thin team-creation interface early. Separate employee identities from devices and agents. Migrate existing histories into an explicit legacy scope, preserving IDs and admission snapshots.

Acceptance: duplicate creation is harmless; restart retains resources; roles only grant explicit capabilities; definition updates do not silently rebind queued work; no invented token charge or Running status. Test backup/restore and reject unsupported old writers.

**Exit evidence:** local control CLI bootstrap/create/membership; durable store + backup restore; D02 charge/Running removed from prototype registration; registered org agents use distinct identities and immutable revisions.

## MVP-102 — Implement scoped context and explicit knowledge sharing

Implementation sequence and current source boundaries: [context isolation cutover](context-isolation-cutover.md).

Provide private and team history/knowledge/artifact grants. Retrieval, context assembly, caches, model-session handles, streams and errors all obey the execution scope. Personal-agent team participation creates a separate authorized working context. Explicit publication includes provenance and destination authorization.

Acceptance: place a unique secret in private history; team execution inputs, tools, logs and outputs cannot retrieve it without authorized publication. Membership revocation blocks future retrieval. Guessed IDs and summaries cannot bypass scope. Do not promise privacy against infrastructure administrators without a separate threat model.

**Exit evidence:** `team_execution_cannot_retrieve_unpublished_private_history`; participation working context; authorized publication; membership-checked open_live/poll_run; FanoutEventSink and SessionService are not employee APIs.

Reuse: identity store, artifact/context interfaces, provenance patterns. Nonlocal multi-tenant exposure remains gated. Prototype map deletion (D01) and coding-identity pin removal (D11) wait for later cutover.

Commit discipline: characterization/contract, replacement behavior, caller cutover, then gated removal. Record focused tests and remaining compatibility obligations.
