# Reconciliation implementation sequence

These are dependency-ordered work packages, not calendar promises. All are planned, not implemented by this audit. Each ticket requires a focused commit for the new path and a separate or clearly bounded removal commit after parity. Do not defer retirement indefinitely after replacement ships.

| Sprint | Tickets | Exit evidence | Retirements |
|---|---|---|---|
| [0: Contracts and baseline](sprint-0-contracts/plan.md) | REC-001, REC-002 | Canonical objects/authority and runnable characterization scenarios | Unsupported claims/config advertising |
| [1: Durable control plane](sprint-1-control-plane/plan.md) | REC-101, REC-102 | Authenticated creation and restart-safe tenant resources | D02, D11; D01 replacement starts |
| [2: Unified local execution](sprint-2-local-runtime/plan.md) | REC-201, REC-202 | Coding and Village execute through same managed lifecycle | D07, D08; D01/D04 consumer migration starts |
| [3: Enforcement and observability](sprint-3-enforcement/plan.md) | REC-301, REC-302 | Revocation/approvals/budgets and truthful replay | D01/D04 close after operator cutover; D03, D05, D10 |
| [4: Remote ownership](sprint-4-remote-workers/plan.md) | REC-401, REC-402 | Worker loss/stale generations/unknown effects tested | D06 |
| [5: Product and deletion cutover](sprint-5-cutover/plan.md) | REC-501, REC-502 | Unified config, packaging, optional coding pack, removal report | D09, D12 and remaining gated removals |

Authentication and basic authorization are prerequisites from sprint 1 onward; sprint 3 strengthens full execution enforcement rather than introducing security after an exposed server. Sprint 2 must preserve existing checks during harness migration. Begin tenant migration in sprint 1 and require negative isolation tests at each added boundary.

Release gates: sprints 1–2 are restricted to a single-organization development profile until every reachable data/effect boundary has tenant enforcement. No untrusted executable harness before its isolation profile. No unbounded activations pending sprint-3 accounting. Every migration follows expand/migrate/switch/contract with a tested restore path and one mutable authority. Sprint 4 is a separate deployment milestone, not a prerequisite to proving the local product.

Cross-cutting acceptance: no fabricated Running/Applied states; no silent ephemeral fallback in production; no unsupported auto-resume; no uncontrolled world action; no cross-tenant reads; no historical database deletion; no duplicate mutable authority. New workload-specific logic belongs in harnesses/integrations.
