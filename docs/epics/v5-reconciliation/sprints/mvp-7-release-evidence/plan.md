# Sprint 7 — Production deployment and release evidence

Status: planned. Depends on the preceding MVP sprint; security and bounded admission are enforced incrementally, never deferred until final hardening. Tickets may be split into smaller implementation commits without weakening exit criteria.

## MVP-701 — Deliver the selected production topology and runbooks

Implement the HA/storage decision from MVP-001: replaceable API replicas, fenced controller ownership, supported authoritative storage and accessible artifacts. Provide one primary installation package with preflight, migrations, secret references, logging/telemetry/storage configuration, drain and restore instructions. Standalone remains simpler and explicitly non-HA.

Acceptance: kill an API/controller instance and observe recovery without lost accepted work or competing ownership; quorum loss obeys the documented safety/availability boundary; database backups restore actual work; no hidden shared SQLite deployment. Do not claim HA based on a chart replica count.

## MVP-702 — Prove the MVP scenarios and measured scale

Run the end-to-end team scenario from the product charter with coding and noncoding tools. Exercise private/team isolation, proxy delegation, destructive-action denial, parked work, approval restart, workstation disconnect, stale ownership and exporter/storage outages. Measure activation/action/event rates, concurrency, queue growth, cancellation and recovery under declared hardware/model conditions.

Acceptance: publish the supported envelope and limitations, install/upgrade/restore results and regression commands. Compare useful outcomes and intervention burden in real user trials. If HA gate is unfinished, release only an explicitly limited preview; no production HA label. Resolve all critical privacy/enforcement failures before release.

Exit: coherent MVP with evidence, not broad feature-count completion. Further scale, backends and integrations require measured need.

Commit discipline: characterization/contract, replacement behavior, caller cutover, then gated removal. Record focused tests and remaining compatibility obligations. No production changes were made by the planning ticket.

