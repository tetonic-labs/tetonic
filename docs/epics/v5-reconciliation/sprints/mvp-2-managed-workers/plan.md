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

Next: cumulative token/effort and concurrency admission bound to the job; supported product transport; scoped interactive approvals/live streaming; supported provider-profile selection and isolation/recovery evidence. Registered submission retries now use the explicit request_id described below; recovery_id remains job attribution. Prepared audit histories may remain if later assembly/admission fails or another concurrent request wins. These records do not imply a run was started. No automatic legacy briefing, global context compiler or conversation reuse is enabled for registered jobs.

## Bounded registered execution (2026-09-25)

RegisteredExecutionSettings now requires an operator-selected max_elapsed_seconds from 1 to 86,400. The clock starts at application submission, and the existing TaskInputBinding.deadline and run deadline projection persist the absolute bound at admission. Unix-second precision can shorten the requested interval by less than one second. Legacy callers retain their previous optional-deadline behavior. Managed children inherit the earlier of their own and their parent's deadline; governed delegation remains disabled pending its full grant/budget contract.

The manager combines the persisted deadline with a monotonic limit captured at admission, preventing backward clock adjustments during execution from extending the bound. Existing execution gates recheck it before new inference/tools; the existing cancellation poll interrupts hung inference and closes WorkScope. Finalization signals cooperative verification at expiry and retains ownership of any blocking worker until it returns. Expired jobs record the existing TimedOut failure class and task timeout kind, fail the run and accept no new output. Durable finalization claims and result acceptance reject at the exact deadline. Results completed before the deadline may still finish artifact publication afterward; publication failures retain their recovery state.

This is a local per-job host ceiling, not cumulative team effort accounting, hard real-time termination, a process-kill guarantee, or distributed recovery/resume. A noncooperative effect may exceed the deadline while draining, and completed effects cannot be undone. Synchronous or unavailable host/storage operations can also delay completion. Evidence covers hung brokered HTTP inference, cooperative verification, a noncooperative commit that retains ownership until released, deadline persistence/reopening, child inheritance and exact-boundary result rejection. Remaining exit criteria above still apply.

## Durable activation retries (2026-09-25)

RegisteredAgentJob now requires request_id: 1–128 ASCII letters, digits, periods, underscores or hyphens. A client keeps this key for retries and uses a new key for intentionally distinct work. The manager derives a stable run ID from the verified organization/principal and request ID. The root TaskInputBinding stores the key, a host-computed request fingerprint and the winning audit history locator. Fingerprints bind the exact job, current scope/grant and effective workspace/model/tool/context/preparation/time settings. Changed requests under the same key conflict. The original deadline is never renewed by a retry.

This extends the existing run journal and its atomic event/projection transaction, with no activation database, schema migration or independent lifecycle state. Concurrent managers on the same local SQLite store rely on that transaction and command deduplication: only the creator can proceed into attempt admission. The run-command transaction now acquires its write lock before reading projection metadata to avoid deferred WAL transaction upgrade races. Retried receipts are reauthorized before and after reading, including terminal jobs; revoked grants deny. The application returns run/task/audit locators and an optional execution handle. That handle appears only on the winning launch. A receipt alone does not claim Running or grant execution; use current scoped inspection/replay for state and results.

The existing managed submission owner now spans admission through finalization, so dropping the launch caller while admission is in progress does not leave a locally admitted job without an executor. A process crash or partial durable admission remains bound to its original key. Retrying returns that existing run without replaying effects or automatically resuming it. Recovery/resume, multi-node ownership and retention/tombstone policy remain separate work. Deduplication is valid while the authoritative run record is retained; an operator deleting it also deletes its retry history. Concurrent preparation can leave an unused empty audit history, as can a later preparation failure.

Verification includes a forced two-manager creation race, SQLite failure after CreateRun, database reopening, conflicting requests, grant revocation, intentional new keys, loss of the admission response, and concurrent/terminal retries through the real configured runtime, compute broker and HTTP test endpoint.
