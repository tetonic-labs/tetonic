# MVP implementation progress

## 2026-09-25 — First persistence and boundary corrections

MVP-001 and MVP-002 are in progress. MVP-101 has a storage prerequisite implemented; neither the resource service nor organization authorization is complete. No existing fleet caller has been cut over, and no retirement gate is closed.

### Implemented

- Reuse the existing SQLite Store, migration transaction and pre-migration backup machinery. Schema 29 adds organizations and organization-scoped teams. Team ownership is a principal reference, not a grant or authentication assertion.
- Require organization plus team ID for team lookup. Exact creation retries succeed; conflicting attributes fail without overwriting ownership. Concurrent creators cannot both replace the same team. Creation does not activate an agent or charge inference.
- Preserve existing identity records across migration. Existing future-schema checks continue to reject unsupported older writers.
- Correct the IntentCharter verb helper: namespace allowlists reject malformed/unscoped verbs, and path/resource boundaries explicitly reject because this helper cannot enforce them. Real effect-boundary policy and budget integration is still required for D13.

### Validation

Commands run from `engine/`:

| Check | Result |
|---|---|
| Baseline: `cargo test -p tetonic-domain -p tetonic-memory -p tetonic-run --lib` | 30 domain, 69 memory, 9 run tests passed before changes |
| Updated domain tests, in the same combined command | 32 passed |
| Final: `cargo test -p tetonic-memory -p tetonic-run --lib` | 74 memory and 9 run tests passed |
| `cargo check -p tetonic-app` | Passed, including downstream compilation against the changed store |
| `cargo run -p tetonic-arch-gate -- arch` | Passed |

The memory tests cover reopen, scoped keys, conflicting retries, concurrent ownership creation, invalid/orphan resources, upgrade preservation and existing migration crash/recovery scenarios. An intermediate run exposed three downgrade fixtures that leave newer tables behind; the migration was made consistent with the existing idempotent table-creation pattern and all memory tests then passed. This is not evidence of authenticated tenant isolation, end-to-end execution, HA, or a production deployment. Full-workspace tests were not run.

### Contracts used for this slice

The local preview uses one local SQLite authority. It must not be deployed as a shared database file across machines. Organization IDs scope stored resources; trusted caller identity and authorization must be established in a service before exposing these methods through an API. Personal/team context separation cannot be inferred from these tables.

New team resources do not mirror or dual-write the prototype FleetManager maps. They remain unused by product entrypoints until the authorized service and consumer migration are ready. A storage prerequisite is not a second lifecycle authority.

### Next integration work

1. Finish MVP-001 decisions: canonical agent identity and immutable definition revisions, principal/membership model, supported OS/isolation profile, production storage/ownership topology, and numeric capacity/recovery targets.
2. Extend MVP-002 characterization to fake Running/quota behavior, coding/world activation, approval waits and history restore before removing their consumers.
3. Build the authorized resource service and definition/membership persistence for MVP-101; migrate fleet callers through that service into the existing managed execution path.
4. Prove private/team context separation under MVP-102 before exposing shared team execution.
5. Cut over callers and delete prototype authorities only after their replacement behavior passes the retirement gates.

The original inventory and validation report are historical audit evidence. Implementation changes intentionally diverge from their file hashes; do not regenerate that baseline to conceal drift.

## 2026-09-25 — Application resource service boundary

MVP-101 now has an application-composed ResourceService for organization/team creation and reads. It reuses the managed service's SharedStore and refuses composition without storage. An explicitly injected ResourceAuthority must authenticate and authorize each exact action before storage access, including retries. Team ownership comes from the resulting principal; it cannot be supplied in the request. No default allow provider, second database or execution manager was added.

The [service contract](sprints/mvp-1-resources-privacy/resource-service-contract.md) records the trust boundary and pending production requirements. The current authority implementation exists only in tests. Credential verification, durable memberships/grants, administrative audit events, transport exposure and fleet caller migration are still unfinished. This is not a claim of production authentication or end-to-end team isolation. The legacy fleet module description now identifies its in-memory prototype status instead of claiming persistent supervision.

The architecture gate (`cargo run --manifest-path engine/Cargo.toml -p tetonic-arch-gate -- arch`) passed after these changes. Documentation checks resolved 78 local links and passed formatting checks.

Validation: `cargo test --manifest-path engine/Cargo.toml -p tetonic-app --lib resources::tests` passed all four tests after a clean rebuild. Tests exercise application/store composition, persistence across recomposition, ownership conflicts, denied scopes/actions, revocation on reads/retries, and refusal of volatile storage. Focused rustfmt and `git diff --check` passed. Compilation initially failed because the disk was full; user-authorized `cargo clean` removed 100.7 GiB of generated artifacts. The successful rebuild disabled incremental compilation and debug symbols through per-command environment settings; no repository build profile was changed.

## 2026-09-25 — Durable membership authority

Schema 30 adds principals, organization roles and explicit team memberships. Control access is decided in one SQL statement using current enabled state, organization membership and team scope. Missing membership denies; organization removal deletes explicit team memberships; principal disablement overrides all metadata permissions. Upgrade from schema 29 preserves resources without creating implicit grants. The schema-29 migration now checks its marker before insertion so later upgrades can safely reuse the migration chain.

Application::membership_resource_service composes this persistent authority with a required CredentialVerifier. The test verifier proves the service uses a verified stable identity rather than the credential text as ownership, and consults current membership on later requests. A real credential provider, administrative mutation APIs/audit, immutable agent revisions and fleet cutover remain outstanding. Permission and revocation semantics are specified in the service contract linked above.

Storage validation: all 77 tetonic-memory library tests passed, including v29 upgrade, scoped permissions, durable revocation and the existing migration failure/crash matrix. Build commands continue using per-command incremental/debug settings to limit artifact growth.

Application validation: all five `resources::tests` passed. The architecture gate passed. After adding administrator/demotion assertions, the three membership storage tests passed again. Production credential verification is not covered by these tests and is not implemented by this slice.

## 2026-09-25 — Local control credentials

Schema 31 stores credential hashes, audience, issuance/expiry times, revocation and transactional lifecycle events. Application::local_credentials supplies a real local verifier to the membership resource service. Issuing a credential requires an existing enabled principal and grants no role. Per-command provisioning stays internal; no unauthenticated issuance endpoint was added. Existing sha2 moved from the application's dev dependencies to runtime dependencies; UUID v4 is reused for random secrets.

The current service contract specifies lifetime limits, hash-only persistence, audience isolation, independent revocation, redacted Debug output and remaining bootstrap/transport/admin-audit work. The v30 migration now checks its marker, preserving the upgrade chain. Older fixture databases explicitly remove the newer credential tables before simulating their historical schema.

Validation so far: all 80 memory library tests passed, covering time boundaries, wrong audience, disabled principals, restart persistence, independent revocation state, v30 migration preservation and atomic rollback when credential-event writes fail. This does not establish an end-user login experience or complete MVP-101.

Final slice validation: all six application resource tests passed, including real local credential issuance through team creation/read, wrong-audience denial, independent revoke and service recomposition. The architecture gate, focused rustfmt, whitespace and documentation-link checks passed. No HTTP endpoint or external identity provider was tested or added.

## 2026-09-25 — Local operator vertical slice

The existing lokai CLI now exposes `control` commands for one-time bootstrap, local credential issuance/revocation and authenticated team creation/inspection. It composes the same resource, credential and membership implementations from a chosen SharedStore without starting Application's coding runtime. LocalControl is a composition facade, not another lifecycle manager or persistence authority. The [runbook](sprints/mvp-1-resources-privacy/local-control-runbook.md) provides a PowerShell walkthrough and trust limits.

Schema 32 adds administrative events. Bootstrap is serialized in one transaction and refuses any already initialized control principal set. The bootstrap event labels the actor as a local operator without pretending an employee identity was authenticated. Audit failure rolls back all initialization; concurrent bootstrap attempts have one winner. Credential issuance is a separate recoverable step. Existing membership/credential APIs remain internal provisioning doors until authenticated administration is added.

Initial validation: `cargo test --manifest-path engine/Cargo.toml -p tetonic-memory -p lokai-cli` passed the CLI unit suite, 82 memory library tests and three store-concurrency integration tests. A separate CLI-process integration test exercises the usable local path. No network endpoint, remote client or agent activation is implied by these commands.

Final validation: the separate-process `control_cli` test passed, including rejection of volatile storage, missing/revoked credentials and repeated bootstrap. The architecture gate passed. The CLI unit suite contained 112 tests: 110 passed and two were ignored. The local runbook demonstrates the path without writing bearer secrets into command arguments. User documents outside this epic were preserved.

## 2026-09-25 — Retired binary branding correction

Per the product correction, the shipped CLI binary is `tetonic` and the existing stdio daemon is `tetonicd`. `tetonic-server` retains its existing name. CLI help/examples, terminal branding, daemon identification, executable locators, installers, release archives and the local-control runbook now use Tetonic. The release workflow no longer emits duplicate Lokai-named archives. No release was published and no installer was executed against the host.

Legacy Cargo package IDs/source directories (`lokai-cli`, `lokaid`), persisted database/config paths, environment compatibility names and enrollment wire prefixes remain unchanged. These are migration concerns rather than public binary names; renaming them blindly would break stored state or source-path checks. Historical audit inventories remain historical. The generated mock LSP server is test tooling rather than a product binary.

Validation: Cargo metadata exposes `tetonic` and `tetonicd`, with no `lokai`/`lokaid` binary targets. Both binaries built; CLI tests passed (110 passed, two existing ignores), daemon tests passed (48), and the control integration test passed with `CARGO_BIN_EXE_tetonic`. The architecture gate, PowerShell installer AST parse, Git Bash installer syntax check and release/installer reference checks passed. Release YAML was inspected but a YAML parser was unavailable. Packaging was not executed on the cross-platform release matrix.

## 2026-09-25 — Source folder and package branding

The naming correction now includes source folders and Cargo package IDs: `engine/litho/tetonic-cli` (package `tetonic-cli`, binary `tetonic`) and `engine/litho/tetonicd` (package/binary `tetonicd`). The daemon smoke script is `engine/scripts/smoke_tetonicd.py`. Build commands, release/CI selectors, source-path tests, architecture checks, generated-protocol comments and active documentation references were migrated. This supersedes the preceding decision to retain legacy package IDs.

Historical audit inventories retain their original baseline paths/hashes; historical prose is not rewritten into evidence of a newer tree. Documentation source links now resolve to the renamed folders. The `.lokai` malicious-project security fixture and persisted user-data/config names remain legacy-format compatibility cases, not product source package names. The user's checkout directory is outside the versioned source rename.

Validation: renamed CLI/daemon suites, CLI process integration and all 89 architecture-checker unit tests passed; the live architecture gate and epic documentation links passed. All six selected application integration suites that pin CLI, composition, gate, portal and work source paths passed against the renamed folders.

## 2026-09-25 — Authenticated organization membership administration

MVP-101 now exposes organization role grants/removal through the resource service and local `tetonic control set-member` / `remove-member` commands. The actor comes from verified credentials; durable enabled administrator membership is checked again inside the transaction that changes membership and records actor, subject, scope and requested role. Last-enabled-administrator removal/demotion is rejected. Team-creator and platform-only identities cannot administer organization membership. This does not grant execution, tool or private knowledge access.

Trusted local `register-principal` provisions an unprivileged identity with an audit record, without allowing retries to alter an existing principal's enabled or platform-admin fields. The schema remains version 32. Credential revocation remains admission-time: the transaction rechecks actor membership, not the already-verified credential. Remote transport, team-specific membership administration, audit browsing, operator identity/recovery, immutable agent revisions and privacy enforcement remain unfinished.

Validation: 85 memory-store tests, six application resource tests, two separate-process CLI scenarios and the architecture gate passed. Added evidence covers self-escalation/cross-org denial, revoked administrators, last-admin protection, concurrent cross-removal, rollback on audit failure, and grant/create/revoke/deny across CLI processes. All test databases are temporary.

## 2026-09-25 — Explicit team membership administration

Added authenticated `add-team-member` / `remove-team-member` CLI and resource-service operations. Current team owners and organization administrators may manage explicit team metadata membership; ordinary members cannot grant themselves access, and recipients must already belong to the organization. Transactional authority rechecks and audit writes follow the organization administration pattern. Removing explicit membership does not remove independent owner/admin privileges.

Schema 33 adds `team_id` to administrative audit events while preserving earlier records. The previous migration now skips an already-applied marker; legacy downgrade test fixtures remove newer schema artifacts before replay. Existing automatic pre-migration backup, rollback and crash-recovery machinery remains in use.

Validation: all 87 memory tests, six application resource tests, both separate-process CLI scenarios and the architecture gate passed. Evidence covers team scope, nonmember denial, owner removal, transactional audit failure, version-32 upgrade preservation, and explicit grant/read/revoke/deny through real CLI processes. MVP-102 context isolation remains unimplemented; these metadata grants must not be reused as implicit authority to personal knowledge or execution.

## 2026-09-25 — Preserve admitted identity revisions

Managed admission previously overwrote one identity row, while execution compared its admitted identity to that latest row. A later definition revision could invalidate previously admitted work. Schema 34 now retains immutable identity snapshots keyed by identity ID and definition digest, backfills the existing row, and updates the latest projection atomically. Reusing the same key with different attributes is rejected. Managed execution resolves its job-bound revision instead of the latest projection.

This is a migration of the existing managed execution path, not a disconnected resource API. The new application regression admits revision one, persists revision two, verifies revision one reaches inference, then cancels it. Updating a definition is not cancellation; use the explicit cancellation path. Existing execution policy checks remain in place.

This is only part of MVP-101: snapshots are not complete serialized harness/definition payloads, digest generation is still caller-owned, and organization-scoped agent creation, revision selection controls and private/team context isolation remain pending. Migration can preserve only the latest pre-upgrade identity row; older overwritten revisions cannot be reconstructed from this table.

Validation: 88 memory tests (including upgrade/crash/backup checks), all 11 managed-service integration tests, all three application execution regressions and the architecture gate passed. The missing-identity regression initially waited on inference because it still deleted the old projection; it now removes the authoritative revision table and uses a bounded timeout. The regression verifies missing revisions deny before policy/inference, and explicit cancellation still stops work after a newer revision is published.

## 2026-09-25 — Trace privacy cutover across existing context paths

Added the MVP-102 [context isolation cutover](sprints/mvp-1-resources-privacy/context-isolation-cutover.md) from current source inspection. Resume already checks workspace ownership, while recall, briefing summaries and project consolidation use workspace/project boundaries rather than employee/team grants. Live-session lookup, artifact storage and event fanout are trusted internal APIs, not authenticated employee interfaces. The context cache already hashes the full request; its missing privacy input is trusted context/grant state, not merely a session cache key.

The plan now specifies immutable session-context binding, explicit legacy-local migration, scoped retrieval/consolidation, current authorization before cache use, artifact/event controls and adversarial end-to-end canary evidence. It rejects silently turning metadata membership into private-content access. All referenced source file paths were checked. This commit changes the implementation contract only; it does not claim runtime privacy or enable remote access. Provider-specific cached conversation semantics still need a dedicated trace during compiler cutover.

## 2026-09-25 — Durable session context bindings and legacy read isolation

Schema 35 adds information-context records (legacy-local, organization-private and team) and immutable `sessions.context_id`. Existing sessions and new sessions created through the existing local API use `legacy-local`; no employee ownership is guessed. The database rejects unknown context IDs, session rebinding, context mutation and deletion of an in-use context. Existing context ownership references organizations/principals/teams, but record existence is not an authorization grant.

Legacy transcript, resume, recovery-payload and history-list/latest APIs now deny or exclude nonlegacy sessions. Recall FTS and recent finish summaries filter to legacy sessions before returning content. Project consolidation rejects scoped input so future private sessions cannot flow into the existing shared-by-workspace digest. The migration reuses backup/crash/recovery machinery, and older downgrade fixtures now explicitly remove context schema artifacts.

Validation: all 90 memory tests, all 118 application unit tests and the architecture gate passed. A private canary fixture verifies transcript/resume/recall/summary/consolidation exclusion and immutable context binding; upgrade/reopen preserves legacy history. Private fixture construction is trusted test SQL: no public scoped activation API is exposed yet. This is the first connected MVP-102 storage/read-path change, not full privacy: scoped authorization/retrieval, project-knowledge ownership, live sessions, caches, artifacts, streams and publication still require cutover before multiuser execution is enabled.

## 2026-09-25 — Authenticated information-context access

The application now composes `ContextService` from the existing SharedStore and CredentialVerifier; LocalControl exposes the same service. Context creation derives private ownership from the authenticated principal, verifies current enabled organization membership, and permits team contexts only for current team owners/explicit members. Metadata administrator status alone confers no content access. Idempotent creation cannot replace an existing context or reveal an inaccessible conflicting context.

Scoped transcript retrieval verifies current membership and exact session/context binding in one database read transaction. It returns at most 200 messages, preserves chronological order and excludes rolled-back spawn branches. Legacy-local contexts cannot be read through this employee-authorized service. Credential checks occur at admission; membership/content reads share a snapshot, so revocation after the snapshot does not retroactively cancel that already-admitted read.

Validation: focused storage authorization tests, all seven application resource tests, and the architecture gate passed. Tests cover private canary denial for administrators/other employees, explicit team grant/revocation, organization removal, guessed session IDs, rollback exclusion, forged credentials, credential revocation and conflicting context creation. No public scoped session activation or inference path is enabled yet. Session lifecycle, tools/recall, summaries, caches, artifacts and streams still require authorization cutover; this service uses existing history tables rather than a second history store.

## 2026-09-25 — Authenticated scoped discussion history

`ContextService` can now open durable discussion histories and append human messages through the existing sessions/messages tables. Context authorization and mutation share a write transaction. Caller-supplied session IDs cannot rebind an existing session; message retries use a session-scoped client ID and only succeed for identical author/content. Permission is rechecked even on retries. Messages are restricted to user role, 64 KiB, and verified human author attribution rather than an invented agent identity.

Schema 36 adds nullable human-author and client-message identifiers to existing messages, plus a unique session/client-message index. Old messages retain their existing content and unknown historical author; no identity is inferred. Discussions have `mode=discussion`, `status=open`, no model and no workspace grant. Creating one does not activate agents or emit a Running status. Legacy transcript/recall/summary fences continue to apply.

Validation: all 93 memory tests, seven application resource tests and the architecture gate passed. Tests exercise authenticated create/open/append/read, idempotent retries and conflicts, denial for another principal, access removal across reopen, correct human attribution, and rollback of message plus recall indexing when attribution fails. Scoped runtime activation, UI/CLI exposure, close/archive behavior, context-aware tools and end-to-end inference isolation remain outstanding.

## 2026-09-25 — Exercise scoped discussions through the product CLI

`tetonic control context` now exposes private/team context creation, discussion opening, human message submission from a bounded UTF-8 file, and JSON history. Every operation supplies the stdin credential to the same ContextService; no duplicate storage/authorization implementation is introduced. Open/send explicitly report that no agent was activated. History limits are constrained to 1–200 messages.

The local control runbook includes a usable walkthrough and separates discussion history from agent execution. Local database operators remain trusted. Validation: all three CLI integration scenarios and the architecture gate passed. The new separate-process scenario verifies persisted private history, idempotent message retries, no credential/canary disclosure on denied reads, missing credentials, oversized files, organization revocation, team membership requirements and denial when a team context is paired with a private session. Scoped runtime activation and the rest of MVP-102 remain incomplete.

## 2026-09-25 — Authorized context message recall

Added context-scoped message recall in the existing memory store, composed through ContextService and `tetonic control context recall`. Authorization and retrieval share a read snapshot. SQL restricts to the requested context before returning snippets/limits, excludes system messages, and checks the indexed body/role against an existing non-rolled-back source message. The existing index lacks message IDs; this source revalidation prevents deleted/rolled-back entries from being returned without introducing another index. Scoped results do not use global BM25 ranking statistics.

Validation: the focused recall test, all three CLI integration scenarios and the architecture gate passed. Evidence covers private/team canary separation, filtering before a one-result limit, stale-index and rollback exclusion, membership revocation and real CLI search. Query syntax is normalized by the existing FTS tokenizer and input is capped at 4096 bytes; results are capped at 30. The agent tool adapter is not yet switched over, and tool-result recall/project digests are intentionally not represented as authorized by this message-only API. Full scoped execution remains gated on the remaining MVP-102 cutover.

## 2026-09-25 — Bind agent recall tools to authenticated contexts

ContextService can now bind an existing Tools instance to a verified principal/context and its authoritative store. The actual `recall` dispatcher uses scoped message retrieval for that binding, rechecking membership on every invocation. Clones retain the binding, and later legacy `with_memory` configuration cannot replace its database or downgrade it to workspace recall. Model arguments cannot set a principal/context: unknown recall arguments are rejected. Tool descriptions accurately identify scoped message-only recall; returned snippets are marked untrusted and scoped storage errors are sanitized.

This is trusted host composition, not a new model-callable authority. Credential verification is admission-time; the binding checks durable membership thereafter, so credential expiry/revocation does not itself cancel an already-bound execution. Explicit execution lifetime/revocation integration remains pending. Binding recall grants no file/tool capability and does not by itself start scoped inference.

Validation: seven resource tests passed before the final module extraction; the authenticated tool-dispatch test passed again afterward. It exercises real Tools dispatch, model scope spoofing rejection, retained scope through cloning/legacy configuration, and membership removal after binding. The architecture gate initially rejected growth of tools/lib.rs; memory configuration, read-only connection caching and recall dispatch were extracted together into memory.rs, and the gate now passes. Remaining compiler, briefing, artifacts and stream paths are still required before full scoped activation.
