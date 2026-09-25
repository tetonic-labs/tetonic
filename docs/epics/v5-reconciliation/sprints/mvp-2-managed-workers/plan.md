# Sprint 2 — One runtime, real effects and useful first work

Status: planned. Depends on the preceding MVP sprint; security and bounded admission are enforced incrementally, never deferred until final hardening. Tickets may be split into smaller implementation commits without weakening exit criteria.

## MVP-201 — Connect activation to managed local harness execution

Inject through the existing LocalAgentAttemptExecutor boundary; preserve claim, identity, policy, thread-affinity and quiescence semantics. Create durable activations with bounded concurrency, time, tokens and queues. Local worker reports observations; manager validates outcomes. Enforce supported process, credential and egress isolation before running model-requested tools.

Acceptance: the setup UI can activate a real agent and inspect actual events; failed preparation never reports Running; stop reaches owned processes; no uncontrolled direct effect or inference bypass. Exactly one active attempt by default for effectful work.

## MVP-202 — Make coding optional and move the world path onto the runtime

Extract coding roles, prompts, critic/router and toolsets behind a selected harness/capability composition. Move the world loop out of server main/core alternate lifecycle. Route actual Village effects through authorization; distinguish snapshots from durable messages. Prove a noncoding external-tool workload with no repository requirement.

Acceptance: one lifecycle works for both workloads; no mandatory coding identity/repo/tools; idle cancellation works; event receipts survive supported retries; denied effects never reach destinations. Preserve no-omniscience observations. Village game code stays outside this repo.

Reuse: REC-201/202 plus coding extraction from REC-502. Remove D07/D08 only after live parity; retain useful adapters and test fixtures.

Commit discipline: characterization/contract, replacement behavior, caller cutover, then gated removal. Record focused tests and remaining compatibility obligations. No production changes were made by the planning ticket.

## Managed activation boundary (source trace, 2026-09-25)

`identity_job.rs::begin_job_run` composes identity/job admission through `ManagedRunService::admit_with_context`; `AdmissionContext.session_id` is correlation, not an authenticated employee scope. The existing manager pins identity/job bindings and execution rechecks the durable identity revision, input digest, advertised capabilities, policy and claim. Preserve this lifecycle when adding employee activation.

The legacy admission door now rejects persisted private/team and unknown session IDs before identity writes. This is a cutover guard, not scoped execution support. The next activation contract must resolve an organization-owned agent revision, verified initiating principal, information context, resource grants and budgets; persist those bindings with the existing run/task state; inherit them for child work; and revalidate authority before execution and protected effects. Context-bound retrieval alone is insufficient. Scoped activation must not simply pass an authenticated discussion ID into AdmissionContext or bypass the existing claim/finalization manager.
