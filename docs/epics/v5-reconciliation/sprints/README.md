# October 25 MVP sprint plan

Updated October 4, 2026. The active delivery schedule is the three sprints below, ending with a release decision on October 25. They consolidate remaining V5 integration and product work; they do not start another engine architecture or reset earlier implementation history. Sprint 1 is in progress; sprints 2 and 3 remain planned. See the [current implementation evidence](october-1-coherent-workspace/evidence-2026-10-04.md). No October P0 ticket has met its complete exit criteria yet.

The release should prove that a person can give Tetonic several different kinds of work and leave them progressing without coordinating every agent. Start with one human owner, one execution machine, a prominent map and composer, and a small tested team. Label this an installable limited MVP preview. Shared human rooms, remote workers and production HA remain later V5 milestones with their existing security and failure gates.

## Active schedule

| Sprint | Dates in 2026 | User outcome | Required gate |
|---|---|---|---|
| [1 Coherent workspace](october-1-coherent-workspace/plan.md) | October 4 to 10 | Give real work, inspect the result, and redirect it in one workspace | One complete execution path with truthful state, bounded tools, durable conversation and acknowledged controls |
| [2 Coordinated work](october-2-coordinated-work/plan.md) | October 11 to 17 | Several responsibilities progress, with useful small-team collaboration | Real delegation under inherited limits, independent queues, bounded recurrence and evidence-driven map activity |
| [3 Release confidence](october-3-release-confidence/plan.md) | October 18 to 24 | Install, understand, operate and recover the product without developer intervention | Clean installation, fresh-user trials, security and recovery checks, multi-day operation and published limitations |

October 25 is the release decision, not another feature-development day. Freeze features on October 18. Reserve October 21 to 24 for release-candidate validation and fixes. The October 6 baseline review must confirm a feasible supported profile and revise sequencing if current failures consume capacity; calendar dates are targets, not measured effort estimates.

The [Sprint 1 UI consolidation plan](october-1-coherent-workspace/ui-consolidation-sprint.md) is the detailed product-experience delivery plan: a human, intuitive workspace with one map, one composer and coherent work/decision handling. It contains ten child tickets under the existing October parents: seven P0, two P1 and one P2. These are not a fourth sprint or additional top-level release gates. OCT-104 is now sized L to reflect the complete consolidation; optional refinements defer before release validation time is consumed.

## Work levels and tracking

Each sprint has seven tickets: five P0, one P1 and one P2. There are 21 tickets total, including 15 release requirements. Priorities apply to the October profile, not the lifetime importance of a feature.

These counts refer to the OCT parent tickets. UI-001 through UI-010 are their implementation breakdown; their priorities and dependencies are recorded in the consolidation plan. Seven required UI children do not imply that their parent engine/product acceptance is automatically complete.

| Level | Meaning | Handling |
|---|---|---|
| P0 Required | Necessary to deliver the stated experience or enforce a reachable boundary | Must pass before claiming the release scope; never waive privacy, authorization, cancellation truth or durability to meet the date |
| P1 Valuable | Improves the experience or maintainability beyond the required baseline | Pull in after required work is secure; may move past October 25 |
| P2 Stretch | Optional convenience or refinement | Start only if all required dependencies pass and release validation time remains protected |

Size describes uncertainty and breadth: **S** is a localized change; **M** spans a component or service boundary; **L** spans multiple layers or a distributed-systems contract such as ownership or admission. Sizes are provisional and are not day estimates. Split L tickets into reviewable commits without splitting their safety guarantees. Track states as planned, in progress, blocked, verified or deferred, with evidence beside any verified status. No individual implementers or capacity assumptions are assigned here.

## Supported release boundary

- One human owner, one execution host, durable single-authority storage, loopback access by default, and one primary OS/package selected and proven by October 6. No public unauthenticated listener or shared database across machines.
- One complete provider, built-in harness and tool combination proven against actual model execution. Hide unsupported combinations. Hosted file disclosure requires the existing egress and permission path; never loosen it simply to make a provider work.
- Proposed validation target: three independent work items, two collaborating agents, and one bounded recurring responsibility on declared hardware. Final supported concurrency comes from measurement, not this target.
- Simple work starts without team assembly. An optional short huddle makes larger work understandable. Conversations, assignments, approvals and results retain distinct meanings in the existing resource model.
- Enforce the declared token, time, task and concurrency limits through delegated work. Financial spend controls appear only if backed by a working cumulative ledger; an estimate is labeled as an estimate.
- Support configurable logging, telemetry and storage for the selected profile. An unsupported backend is rejected explicitly. Preserve scoped knowledge and access controls even with one human owner.

Do not add a second run store, team registry, identity model, approval authority, scheduler authority or budget ledger. Integrate the existing ResourceService, managed runtime, broker, memory and tool enforcement. Preserve useful map, conversation and decision components. Retire obsolete paths after caller and behavior checks, rather than deleting packages by name.

## Dependency and scope decisions

The main dependency chain is OCT-101 baseline, OCT-102 supported execution, OCT-103 durable work lifecycle, OCT-201 governed delegation, OCT-202 useful coordination, then OCT-302 and OCT-304 release evidence. Product simplification and packaging can advance once their contracts are stable. Basic cancellation and permission checks precede real tool use; sprint 3 proves recovery and faults rather than introducing enforcement for the first time.

At the October 10 gate, an incomplete single-work journey takes priority over optional team controls and visual refinement. At the October 17 gate, cut P2 then P1 work before reducing the supported provider/OS/concurrency envelope. Keep the smallest real team and recurrence proof. If a P0 requirement still fails, record the failure and make an explicit release-scope/date decision; do not quietly ship simulated collaboration or describe a single-agent build as the planned team MVP.

## Product evidence

Use the same engine and user journey for these scenarios, with real inputs and model calls:

1. Compare supplied research documents and produce a recommendation with source evidence.
2. Investigate a repository issue and prepare a bounded change or proposal with actual validation results.
3. Revisit changing documents on a schedule, report material changes, and wait when there is nothing to do.

Provide task-local tools and context; do not require a source repository for noncoding work. Do not bake a fixed task breakdown, predetermined outcome or fake test result into a scenario. A run completing, evidence being verified, and a user accepting the result are separate states.

Measure first useful outcome, human interventions and active supervision time per accepted result, return-to-understanding time, failures and resource use. Compare comparable solo and small-team work without assuming teams always win. Recruit fresh users during sprint 1 so sprint 3 has actual sessions. Proposed usability targets are next-action comprehension within 30 seconds and return comprehension within 60 seconds; publish observations rather than treating targets as measured facts.

Evidence entries record the ticket, source revision and dirty-tree state, configuration/model, commands or human scenario, actual result, artifacts and unresolved limitations. Older passing tests do not verify today's modified and untracked source. Commit only reviewed work; do not sweep another contributor's unfinished changes into a planning or implementation commit.

## Relationship to the existing V5 work

| October sprint | Existing V5 tickets it completes or adapts |
|---|---|
| 1 | MVP-101/102 resources and privacy; MVP-201/202 managed execution; MVP-401/402 controls; MVP-601 coherent product |
| 2 | MVP-301/302 team work and activation; MVP-401/402 inherited controls; MVP-601 map and oversight |
| 3 | MVP-602 cutover; the standalone portion of MVP-701; applicable MVP-702 release evidence |

The broader MVP-701 production topology and remote-placement gates are deferred for the explicitly limited October preview, not marked complete. Architecture-stage exits below describe their recorded local-preview evidence; they are not proof that the current browser product or modified worktree is release-ready. REC-001–502 tickets remain technical source material, not an independent active backlog.

## V5 foundation stages and recorded exits

| Sprint | Tickets | Exit evidence | Retirements |
|---|---|---|---|
| [0 — Contracts](mvp-0-contracts/plan.md) | MVP-001/002 | Product, privacy, deployment and failure contracts; baseline scenarios | REC-001/002; no cosmetic rewrite |
| [1 — Resources and privacy](mvp-1-resources-privacy/plan.md) | MVP-101/102 | Create a team; durable identities and private/team contexts | **Exited** (local preview); remote UI / D01/D11 deletion deferred |
| [2 — Managed workers](mvp-2-managed-workers/plan.md) | MVP-201/202 | Controlled coding and noncoding execution through one runtime | **Exited** (local preview); remote setup UI, spend ledger, D07/D08 deferred |
| [3 — Team work](mvp-3-team-work/plan.md) | MVP-301/302 | Huddles, backlog, delegation, budgets, autonomous activation and overlap | **Exited** (local preview, 2nd pass); managed child admission, spend ledger, D03 deferred to 4+ |
| [4 — Human controls](mvp-4-human-controls/plan.md) | MVP-401/402 | Bound approvals, hierarchical stop, accounting and truthful inspection | **Exited** (local preview); D01/D04/D05/D10 deletion and remote stop deferred |
| [5 — Placement](mvp-5-placement/plan.md) | MVP-501/502 | Enrolled workstation and remote execution with grants/fencing | **Exited** (local preview); fabric harness cutover, D06 deletion, HA deferred |
| [6 — Product cutover](mvp-6-product-cutover/plan.md) | MVP-601/602 | Coherent team interface and removal of superseded paths | **Exited** (local preview); D01/D04/D05/D06/D10 deprecated, arch gate clean |
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
