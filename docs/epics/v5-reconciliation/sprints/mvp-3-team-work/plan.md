# Sprint 3 — Persistent goals, huddles and autonomous work

Status: **exited** (local MVP preview, second pass). Schema 46–48 store team goals, work items, huddle proposals, attempt/run bindings, event/schedule cursors and bounded delegations. ResourceService exposes create/list/park/resume/activate, huddle accept, cursor activation and delegation. `Application::activate_team_work` / `launch_team_work` / `tetonic job run --team --work` submit through the **same** `submit_registered_job` managed path and bind the resulting run/attempt onto the work item. Child activations inherit a reported-token ceiling from the stored delegation budget and are denied while the parent is parked. CLI: `tetonic control work` for goals/work/huddle/park/resume.

Deferred (later stages): full governed `parent_attempt` admission inside ManagedRunService (still explicitly denied until grant/budget receipts land in sprint 4); cumulative spend ledgers and waiting/fair queues (MVP-402); write-claim conflict negotiation UI; permanent team-change proposals; D03 deletion. Depends on sprints 1–2 (exited local preview). See [implementation progress](../../progress.md).

## MVP-301 — Create durable goals, huddles and team queues

Add team responsibilities, work items, versioned huddle proposals, dependencies and task ownership above bounded executions. Reuse DAG helpers instead of introducing a second execution state machine. Support manual assignment plus one event and schedule activation path, persistent cursors, bounded catch-up, idle waiting, parking and reprioritization.

Acceptance: an accepted huddle creates actual work idempotently; independent tasks run while one waits; parked work survives restart and resumes with context revalidation. Quick tasks do not require a huddle. Scheduled/event duplicates do not generate infinite backlogs.

## MVP-302 — Delegate within limits and coordinate overlapping work

Implement authorized temporary children and requests to existing peers with inherited goal, payer and stop lineage. Reuse SpawnBudgetGate/ledger contracts, persist attribution beyond sessions, enforce fan-out/depth/concurrency/task limits. Add declared resource claims, duplicate-task references and scoped conflict notifications. Permanent team changes are proposals unless explicitly granted.

Acceptance: asking a peer cannot reset the originating budget or escape stop scope; conflicting write claims are detected; denied cross-team requests disclose no private context; bounded negotiation escalates unresolved conflict. Goal management does not force a fixed workflow sequence.

Retire D03 budget/workpad authority only when replacements work; D14 session-only lineage replaced. Preserve generic spawn safeguards, not coding-specialist policy as the platform default.

Commit discipline: characterization/contract, replacement behavior, caller cutover, then gated removal. Record focused tests and remaining compatibility obligations.
