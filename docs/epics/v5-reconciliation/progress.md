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
