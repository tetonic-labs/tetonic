# Sprint 4 — Approvals, hierarchical controls and operational truth

Status: **exited** (local MVP preview). Schema 49 adds hierarchical control stops (org/team/goal/work/agent × pause/cancel/estop), durable effect approvals with digest/expiry, unresolved stop effects, and team effort entries where missing usage is `unknown` not zero. ResourceService exposes stop/clear, propose/resolve/dispatch-check, effort recording and `inspect_team_work` (one team view without private bodies). `activate_team_work` denies under an active ancestor stop. `Application::apply_control_stop` parks matching work and cancels known managed runs, recording unresolved targets. CLI: `tetonic control work stop|clear-stop|propose-approval|resolve-approval|inspect-team`.

Deferred (later stages): full fleet/operator prototype deletion (D01/D04/D05); ThoughtStream consolidation (D10); remote-ready stop propagation; exporter outage soak; spend rate cards / money ledgers; governed `parent_attempt` admission receipts. Depends on sprints 1–3. See [implementation progress](../../progress.md).

## MVP-401 — Bind decisions and stops to actual execution effects

Allow flexible proposal presentations backed by exact action/plan scope, parameters, approval identity, expiry and evidence. Implement org/team/goal/agent pause, cancellation and emergency stop across persisted descendants and cooperating peers. Keep baseline stop enforcement from sprint 2; extend hierarchy, restart and remote-ready propagation here.

Acceptance: rejected/expired approvals never dispatch; changed proposals need revalidation; new descendants cannot start after a parent stop; a shell process tree is stopped within the documented supported bound. Unreachable/noncancelable effects remain visibly unresolved. Paused work resumes only after state inspection.

## MVP-402 — Consolidate inspection and resource accounting

Provide one correlated event vocabulary and projections for inputs, work, actions, approvals, spend/effort, knowledge provenance and actual outcomes. Export telemetry directly; durable execution state is separate. Add threshold escalation and durable team/origin accounting; missing usage data is not zero. Support bounded replay and retention gaps.

Acceptance: one view explains a team's real work without one chat tab per agent; no private context leakage through traces; exporter outage has bounded impact; storage failure is explicit. Live views cannot override lifecycle authority.

Reuse: REC-301/302. Complete D01/D04/D05 only after all operator consumers switch; consolidate D10 streams. Preserve useful bounded buffers and security auditing.

Commit discipline: characterization/contract, replacement behavior, caller cutover, then gated removal. Record focused tests and remaining compatibility obligations.
