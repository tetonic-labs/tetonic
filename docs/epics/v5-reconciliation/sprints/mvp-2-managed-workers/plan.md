# Sprint 2 — One runtime, real effects and useful first work

Status: in progress. Registered general jobs now submit through the application run service and existing managed executor, with stored-grant authorization, scoped output/replay and cancellation covered by integration tests. Configured runtime assembly now exercises real workspace reads/writes through existing controls; employee-facing launch transport, cumulative effort budgets and the supported isolation matrix remain unfinished. Depends on the preceding MVP sprint; security and bounded admission are enforced incrementally, never deferred until final hardening. Tickets may be split into smaller implementation commits without weakening exit criteria.

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


## Registered job submission and configured execution (2026-09-25)

`Application::submit_registered_job` now takes a RegisteredAgentJob plus operator-owned RegisteredExecutionSettings, not a caller-built Agent. Settings select an authorized workspace, model, context size, data class, tool ceiling and preparation limits. They are host composition inputs, not an employee-editable permission grant. DefaultRunService resolves registration/revision and current credential/context/stored-grant authority from the existing managed store. The requested tool set must fit the host ceiling; the assembled catalog must match the pinned definition.

| Existing system | Connection now exercised |
|---|---|
| Installed compute plane | Its typed BrokerInferenceProvider, secret scanner, dispatch policy and egress transport are used. No bare-provider fallback. Provider/broker installation is held as one binding. |
| EngineRuntime | Session assembly installs the existing policy/action broker, capability store and approval gate. Interactive actions deny while scoped approvals are unfinished. |
| Tools/Workspace | Existing workspace resolution, sandbox enforcement, capability consumption and broker-gated process adapter. Scoped recall is attached only when requested. |
| StoreAudit / history | Fresh `execution-audit` histories use the existing sessions/messages/tool_calls/events/file_changes tables and immutable context binding. Tool call IDs are namespaced per audit. Audit failure latches into execution authority checks. |
| ManagedRunService | Same LocalSet submission, active registry, execution claim, fresh conversation, hooks, cancellation, finalization and completion receiver. |
| Workspace finalization | Existing ToolsFinalizationDriver and staged-abort hook; successful writes use the manager's commit path. No separate effect owner. |

The returned audit_session_id can be read with the existing authorized transcript interface. Its `audit` status is a history record, not Running/Completed execution state. The run journal remains the lifecycle authority. A managed-binding note correlates audit with run/task/attempt IDs. The raw-agent registered submission helper remains test-only.

Evidence: the original deterministic integration retains credential/grant/conformance/cancellation/output/replay checks. The configured integration uses a scripted local HTTP inference endpoint through the real broker and egress guard. It reads a real file and verifies that its contents reach the next model request and scoped transcript; a separate scenario writes a real file and completes through workspace finalization. It denies a raw provider, a tool request exceeding the host ceiling, and a disallowed egress endpoint. Injected tool-audit storage failure prevents the next inference and accepted output. Wrong-context and legacy history access deny, and no scoped events reach the legacy application sink. This verifies wiring and effects, not model quality or comprehensive cross-platform sandbox guarantees.

Next: durable activation deduplication; time/token/concurrency admission bound to the job; supported product transport; scoped interactive approvals/live streaming; supported provider-profile selection and isolation/recovery evidence. Repeated calls remain separate activations: recovery_id is attribution, not a submission idempotency token. Prepared audit histories may remain if later assembly/admission fails. These records do not imply a run was started. No automatic legacy briefing, global context compiler or conversation reuse is enabled for registered jobs.
