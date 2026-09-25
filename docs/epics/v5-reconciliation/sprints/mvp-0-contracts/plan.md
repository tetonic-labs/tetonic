# Sprint 0 — Product contracts and characterization

Status: planned. Depends on the preceding MVP sprint; security and bounded admission are enforced incrementally, never deferred until final hardening. Tickets may be split into smaller implementation commits without weakening exit criteria.

## MVP-001 — Set the MVP and deployment contracts

Decide domain vocabulary (Team/Goal/WorkItem versus existing Run/Task/Attempt), identity and knowledge scopes, supported OS/isolation profile, authorization hierarchy, bounded revocation, and production HA/storage topology. Define numeric load/recovery targets before capacity claims. Preserve single-authority local mode while designing replaceable distributed components. Record private-from-peers versus administrator access explicitly.

Acceptance: architecture decisions account for every row of the MVP scope; no model-only enforcement; supported-backend and config schema choices; no universal resume/instant external cancellation claim. Production HA is a release gate, not assumed from replicas.

## MVP-002 — Characterize the existing behavior we must preserve

Capture coding and noncoding/world execution, claim/result integrity, LocalSet hosting, WorkScope quiescence, duplicate delivery, approval waits and legacy history restore. Create the retirement consumer map and explicit failing prototype cases (fake Running, quota fiction, incomplete charter boundaries).

Acceptance: reproducible baseline commands/results and source evidence; behavior tests separated from symbol-presence tests. Planning does not claim these tests already passed. Record no-bypass dependency checks for future composition.

Reuse: REC-001/002. Removal: stop advertising prototype guarantees; no blind deletion before consumers migrate.

Commit discipline: characterization/contract, replacement behavior, caller cutover, then gated removal. Record focused tests and remaining compatibility obligations. No production changes were made by the planning ticket.

