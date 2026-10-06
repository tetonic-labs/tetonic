# October 25 MVP sprint plan

Updated October 5, 2026. The active delivery schedule is the three sprints below, ending with a release decision on October 25. They consolidate remaining V5 integration and product work; they do not start another engine architecture or reset earlier implementation history. Sprint 1 is in progress; sprints 2 and 3 remain planned. See the [baseline implementation evidence](october-1-coherent-workspace/evidence-2026-10-04.md) and [first shaping slice](october-1-coherent-workspace/shaping-evidence-2026-10-05.md). No October P0 ticket has met its complete exit criteria yet.

The release should prove that digital autonomous teams can carry out the person's work while the human shapes outcomes, boundaries and priorities. The operator sees work structure, status, agent participation and actual interactions at a glance, can inspect the details, and receives concise flags when judgment is needed. Tetonic carries orchestration and coordination without unbounded interruption loops. Start with one human owner, one execution machine, a prominent map and composer, and a small tested team. Label this an installable limited MVP preview. Shared human rooms, remote workers and production HA remain later V5 milestones.

## Active schedule

October 5 execution update: [finite agreed-plan execution](october-1-coherent-workspace/plan-execution-evidence-2026-10-05.md)
now connects approved assignments to the existing managed child runtime, shared
usage and current map. A live local model completed two distinct contributions
and a combined result. Useful supplied-document evidence, human flags/steering,
broader capability access and restart reconciliation remain open; no full October
gate closes here.

October 5 follow-on: [bounded human handoff and steering](october-1-coherent-workspace/human-handoff-evidence-2026-10-05.md)
now uses the same plan, runtime, scopes and budgets. Questions and amended
upcoming instructions are durable; waiting remains within a live attempt's
existing deadline. This is partial COORD-B/OCT-202/203 evidence, not durable
park/resume or a completed sprint gate.

October 5 completion follow-up: [completion reliability evidence](october-1-coherent-workspace/completion-reliability-evidence-2026-10-05.md)
fixes the expected-unload response race and delivers newly completed sibling
contributions with dispatch receipts. A real two-agent handoff with an owner
answer and changed upcoming instructions now completed with a combined result
under the original limits: 3101/4096 coordinator tokens, 6107/11096 overall.
This closes that trial's completion gap, not the full sprint's usefulness,
recovery or release-reliability gates.

| Sprint | Dates in 2026 | User outcome | Required gate |
|---|---|---|---|
| [1 Direct and understand a small agent team](october-1-coherent-workspace/plan.md) | October 4 to 10 | Hand over an outcome, observe real team work and shape it in one workspace | Solo baseline plus COORD-A/B/C: governed two-agent collaboration, bounded exchanges, actual work/resource visibility and acknowledged human controls |
| [2 Coordinated work](october-2-coordinated-work/plan.md) | October 11 to 17 | Several responsibilities progress, with useful collaboration and ongoing work | Broaden the team proof, finish shared-limit/fault coverage, independent queues, bounded recurrence and return awareness |
| [3 Release confidence](october-3-release-confidence/plan.md) | October 18 to 24 | Install, understand, operate and recover the product without developer intervention | Clean installation, fresh-user trials, security and recovery checks, multi-day operation and published limitations |

October 25 is the release decision, not another feature-development day. Freeze features on October 18. Reserve October 21 to 24 for release-candidate validation and fixes. The October 6 baseline review must confirm a feasible supported profile and revise sequencing if current failures consume capacity; calendar dates are targets, not measured effort estimates.

The revised [Sprint 1 workspace plan](october-1-coherent-workspace/ui-consolidation-sprint.md) is the product-experience delivery plan: one map, one composer, progressive work inspection and clear human flags backed by bounded coordination. Ten UI children remain: eight P0, one P1 and one P2. UI-008's minimum real team selection/inspection is now required; advanced custom setup stays optional. COORD-A/B/C pull the smallest governed collaboration and activity slices of OCT-201/202/205 into Sprint 1 without duplicating their owners or closing their full acceptance. No fourth sprint is added. Optional customization, filters, shortcuts and motion refinement defer; the second real domain scenario moves from Sprint 1 to Sprint 2 to make room.

## Work levels and tracking

October 4 follow-up: [problem shaping, missing capabilities and skills](october-1-coherent-workspace/shaping-capabilities-and-skills.md)
are required product scope under existing OCT-102/103/104/105. The person can learn
and compare approaches before choosing a plan; agents can request missing access;
and users can create/import skills and use them in governed execution. Parent
ticket counts stay unchanged, but effort has increased. Local exploration and
versioned briefs and finite local plan dispatch now have implementation evidence;
skills and capability requests remain open. Re-estimate at the October 6 review rather
than assuming the expanded scope fits within the dates.

Each sprint has seven tickets: five P0, one P1 and one P2. There are 21 tickets total, including 15 release requirements. Priorities apply to the October profile, not the lifetime importance of a feature.

These counts refer to the OCT parent tickets. UI-001 through UI-010 are their implementation breakdown; priorities and dependencies are recorded in the workspace plan. Eight required UI children and the three COORD slices do not automatically complete their parent acceptance. COORD-A/B/C are named partial gates within existing parents, not new parent tickets.

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
- Simple work starts without team assembly. Unclear problems support persistent exploration, explanation and plan shaping before execution; a huddle can crystallize the chosen approach. Conversations, proposals, assignments, approvals and results remain distinct in the existing resource model.
- Create/edit/export and import agent skills through the declared supported package/source profile; bind exact revisions to actual governed execution. A skill supplies task guidance, not permission. Unsupported script requirements remain visible and cannot bypass the tool profile.
- Missing context, connections and permissions produce actionable work-scoped requests, supported setup paths or alternatives. Resume only after validation; unrelated work can continue. A connector catalog entry alone is not connected access.
- Enforce the declared token, time, task and concurrency limits through delegated work. Financial spend controls appear only if backed by a working cumulative ledger; an estimate is labeled as an estimate.
- Support configurable logging, telemetry and storage for the selected profile. An unsupported backend is rejected explicitly. Preserve scoped knowledge and access controls even with one human owner.

Do not add a second run store, team registry, identity model, approval authority, scheduler authority or budget ledger. Integrate the existing ResourceService, managed runtime, broker, memory and tool enforcement. Preserve useful map, conversation and decision components. Retire obsolete paths after caller and behavior checks, rather than deleting packages by name.

## Dependency and scope decisions

The main dependency chain is OCT-101 baseline, OCT-102 supported execution, OCT-103 identity/work contracts and applicable OCT-105 controls, OCT-201 governed delegation, OCT-202 useful coordination, then OCT-302/304 release evidence. Its first governed team slice now lands in Sprint 1; full parent completion remains in Sprint 2. COORD-C brings the resulting work/activity evidence into OCT-104 without depending on full OCT-205 recurrence coverage. Basic cancellation and permission checks precede real tool use; release testing does not introduce enforcement for the first time.

At the October 6 review, assess actual effort for governed admission, durable exchanges and activity readers against the October 10 team gate. This is a revised target, not a promise that the added integration fits. Defer P2/P1 work before sacrificing the smallest team proof or freeze. At October 10, an incomplete COORD or Sprint 1 P0 gate remains open with explicit schedule impact. At October 17, keep the smallest real team and recurrence proof. If a P0 requirement still fails, record the failure and make an explicit release-scope/date decision; never rename a single-agent build or simulated collaboration as the planned team MVP.

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
