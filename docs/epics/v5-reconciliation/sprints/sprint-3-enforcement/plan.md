# Sprint 3 — Execution controls and truthful observability

Historical REC work package. Scheduling and scope are superseded by the [MVP sequence](../README.md); retain applicable technical safeguards as reference.

## REC-301: Durable grants, approvals and budget accounting

Depends on: sprint 2; basic authorization already required in sprint 1. Extend policy/capability/effect gateway with tenant, principal provenance, attempt, action digest and assignment checks. Implement durable approval wait/resolution/revalidation. Move organization resource accounting into reservations/settlement with time windows. Introduce explicit shared-memory grants and bounded retention.

Acceptance: permission revoked during execution blocks the next mediated effect within the stated contract; denied quota does not leak reservation; concurrent requests cannot overspend admitted limits; restart while waiting for approval is recoverable; changed parameters cannot reuse approval; cross-tenant artifacts/memory/streams fail. External harness security profiles describe actual OS/network controls.

Retirement: D03 ad hoc quota/workpad authority and disconnected approval paths once migrated.

## REC-302: Unified event model and operator commands

Depends on: REC-301 and managed lifecycle. Add correlated events for input, model output, tool request/decision/dispatch/result, policy, ownership and terminal outcomes. Lifecycle transitions are durable; large streaming payloads have retention/redaction rules. Views derive from committed events. Stop/steer/resume return requested/accepted/effective state and errors.

Acceptance: reconnect resumes from cursor or reports retention gap; UI cannot display Running from creation alone; trace exposes actual tool failure; unauthorized reader sees no sensitive payload; stream backpressure does not corrupt execution state. Distinguish unavailable model reasoning from observable output.

Retirement: D05 and D10 disconnected control/schema paths; finish D01/D04 only after all OperatorController imports and status consumers switch. Exit: no parallel dashboard state machine. Bounded buffers may remain as projections.

Stop acceptance: request, worker acknowledgment and confirmed quiescence are distinct; a timeout reports unresolved effects and triggers only supported termination. Multi-organization exposure requires full negative tests on every reachable boundary, including queues, traces, error payloads, artifacts and shared memory.
