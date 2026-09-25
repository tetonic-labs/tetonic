# Sprint 4 — Approvals, hierarchical controls and operational truth

Status: planned. Depends on the preceding MVP sprint; security and bounded admission are enforced incrementally, never deferred until final hardening. Tickets may be split into smaller implementation commits without weakening exit criteria.

## MVP-401 — Bind decisions and stops to actual execution effects

Allow flexible proposal presentations backed by exact action/plan scope, parameters, approval identity, expiry and evidence. Implement org/team/goal/agent pause, cancellation and emergency stop across persisted descendants and cooperating peers. Keep baseline stop enforcement from sprint 2; extend hierarchy, restart and remote-ready propagation here.

Acceptance: rejected/expired approvals never dispatch; changed proposals need revalidation; new descendants cannot start after a parent stop; a shell process tree is stopped within the documented supported bound. Unreachable/noncancelable effects remain visibly unresolved. Paused work resumes only after state inspection.

## MVP-402 — Consolidate inspection and resource accounting

Provide one correlated event vocabulary and projections for inputs, work, actions, approvals, spend/effort, knowledge provenance and actual outcomes. Export telemetry directly; durable execution state is separate. Add threshold escalation and durable team/origin accounting; missing usage data is not zero. Support bounded replay and retention gaps.

Acceptance: one view explains a team's real work without one chat tab per agent; no private context leakage through traces; exporter outage has bounded impact; storage failure is explicit. Live views cannot override lifecycle authority.

Reuse: REC-301/302. Complete D01/D04/D05 only after all operator consumers switch; consolidate D10 streams. Preserve useful bounded buffers and security auditing.

Commit discipline: characterization/contract, replacement behavior, caller cutover, then gated removal. Record focused tests and remaining compatibility obligations. No production changes were made by the planning ticket.

