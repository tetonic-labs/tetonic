# Active MVP implementation sequence

This is the authoritative proposed sequence after product discovery. All tickets are planned. Existing REC-001–502 tickets remain technical source material; they are absorbed below rather than executed as an independent backlog. Eight dependency stages are not eight calendar weeks. Retirement requires evidence, not just arrival at a sprint number.

| Sprint | Tickets | Exit evidence | Retirements |
|---|---|---|---|
| [0 — Contracts](mvp-0-contracts/plan.md) | MVP-001/002 | Product, privacy, deployment and failure contracts; baseline scenarios | REC-001/002; no cosmetic rewrite |
| [1 — Resources and privacy](mvp-1-resources-privacy/plan.md) | MVP-101/102 | Create a team; durable identities and private/team contexts | REC-101/102; D02/D11; start D01/D15 |
| [2 — Managed workers](mvp-2-managed-workers/plan.md) | MVP-201/202 | Controlled coding and noncoding execution through one runtime | REC-201/202; D07/D08 |
| [3 — Team work](mvp-3-team-work/plan.md) | MVP-301/302 | Huddles, backlog, delegation, budgets, autonomous activation and overlap | DAG/spawn reuse; D03/D14 |
| [4 — Human controls](mvp-4-human-controls/plan.md) | MVP-401/402 | Bound approvals, hierarchical stop, accounting and truthful inspection | REC-301/302; close D01/D04/D05/D10 |
| [5 — Placement](mvp-5-placement/plan.md) | MVP-501/502 | Enrolled workstation and remote execution with grants/fencing | REC-401/402; D06 |
| [6 — Product cutover](mvp-6-product-cutover/plan.md) | MVP-601/602 | Coherent team interface and removal of superseded paths | REC-501/502; D09/D12 and removal audit |
| [7 — Release evidence](mvp-7-release-evidence/plan.md) | MVP-701/702 | Supported installation, HA evidence and measured capacity | Config and fault tests |

Authentication, private/team context separation and baseline enforcement precede collaboration and actual tool use. Sprint 4 extends controls to persisted hierarchy and recovery; it does not retroactively secure unrestricted earlier execution. The thin UI starts in sprint 1 and real work in sprint 2; sprint 6 polishes it.

Release gates: restrict early deployment until every reachable boundary has tenant enforcement. No untrusted executable harness before isolation. Bound admission before running work; delegated accounting is mandatory when delegation appears. One mutable authority during expand/migrate/switch/contract. Workstation consent is separate from enrollment. Workers use direct authorized data paths. Production HA must pass sprint-7 evidence; standalone preview does not imply HA.

Cross-cutting acceptance: no fabricated Running/Applied states; no silent ephemeral fallback in production; no unsupported auto-resume; no uncontrolled world action; no cross-tenant reads; no historical database deletion; no duplicate mutable authority. New workload-specific logic belongs in harnesses/integrations.

## Product requirement traceability

| Requirement | Owning tickets |
|---|---|
| Small-team first-use experience | MVP-101, 201, 601 |
| Autonomous ongoing responsibility | MVP-301/302 |
| Huddles and flexible breakdown | MVP-301, 601 |
| Personal agents without private-history contamination | MVP-102, 302, 402 |
| Roles, tool packs/MCPs and top-down permissions | MVP-101, 201, 302 |
| Park/reprioritize while other work continues | MVP-301, 401 |
| Cross-team overlap and collaboration | MVP-302 |
| Effort budgets without proxy reset | MVP-201, 302, 402 |
| Proposals and uncertainty escalation | MVP-301, 401, 601 |
| Stop processes/subagents and resume honestly | MVP-201, 401, 502 |
| Workstation and cluster placement | MVP-501/502 |
| Configurable telemetry/logging/storage | MVP-001, 101, 402, 602, 701 |
| Large-team visibility and measured scale | MVP-402, 601, 702 |
| Cut bloat and preserve safeguards | Every replacement; MVP-602 closure |
