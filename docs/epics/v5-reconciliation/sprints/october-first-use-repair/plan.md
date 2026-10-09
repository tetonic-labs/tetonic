# First use to useful team work

Created: October 8, 2026. Status: **planned; implementation not started**.
Classification: **additional required product scope; release-blocking corrective sprint**.
Baseline: 113a6a3e and the owner's fresh-workspace walkthrough on October 8.
Release target: October 25. Feature freeze: October 18.

## Outcome

A person opens Tetonic, understands where to begin, explores an uncertain idea or gives a clear request, connects the context it needs, and watches useful work progress. When collaboration helps, they can give an outcome to a visible team and receive combined work without manually directing every agent.

The owner explicitly requires this to be an added layer of scope because the walkthrough exposed a significant product failure. It must not be absorbed into a claim that the original scope or effort is unchanged. This sprint repairs the actual failed first-use journey and establishes an additional release gate.

It preserves the existing map, Tetonic visual identity, agent/team creation foundations, real progress transitions and governed execution. It does not introduce another home screen, planning engine, workflow builder or mandatory tutorial.

## Evidence and scope

The [review baseline](review-baseline.md) distinguishes user observations, verified implementation behavior and remaining uncertainty. The owner encountered two direct-request timeouts, a coding-oriented proposal before their intent was understood, an unusable coordination allowance, an invisible selected team, confusing model discovery and overcrowded work details. These are actual user/model-generated records, not sample content to rewrite.

Eight implementation tickets and one validation ticket define this sprint. **EXP-001 through EXP-009 are P0 for the added product gate and must pass before the October MVP is called ready.** Sizes express scope and uncertainty, not delivery days. Mapping to existing owners below identifies reuse and overlapping implementation; it does not mean the new obligations were already funded or proven. No completion below closes an original October ticket automatically.

| Ticket | Deliverable | Size | Dependencies | Existing owner |
|---|---|---|---|---|
| [EXP-001](tickets.md#exp-001) | One clear entry and natural exploration | M | None | OCT-103/104; product 4.1 |
| [EXP-002](tickets.md#exp-002) | Relevant access and usable execution limits | L | None; early feasibility check | OCT-102/105; product 4.4; FAR-004 |
| [EXP-003](tickets.md#exp-003) | Connect a provider and choose models inside Tetonic | M | None | OCT-102; FAR-001/002 |
| [EXP-004](tickets.md#exp-004) | Useful, executable proposals without budget arithmetic | L | EXP-001 and EXP-002 contracts | OCT-103/202; product 4.1 |
| [EXP-005](tickets.md#exp-005) | Focused work details, readable discussion and compact usage | M | EXP-004 allowance contract | OCT-104/105; product 1.2/1.3/1.4/5.1 |
| [EXP-006](tickets.md#exp-006) | A visible selected team and clear delegation | M | EXP-001; EXP-004 for live proof | OCT-104/202/205; product 7.1 |
| [EXP-007](tickets.md#exp-007) | Strong status signals and actionable failures | M | EXP-002 failure contract | OCT-104/105; product 5.4/7.4 |
| [EXP-008](tickets.md#exp-008) | Archive unwanted work and restore it | M | Existing scoped work identity | OCT-103/104; product 1.1/7.3 |
| [EXP-009](tickets.md#exp-009) | Repeat the first-use journey with real useful work | M | Baseline first; final exit after EXP-001–008 | OCT-101/102/202/303 |

## Added scope, October sequencing and schedule impact

This is an additional corrective sprint and the immediate implementation priority. It supersedes the generic 28-task product backlog's delivery order and the older frontier sprint's immediate-priority wording. Proposed optional backlog items are not automatically included, but the original required MVP commitments remain.

The release now requires the original October gates **and** the new First use to useful team work gate. Existing implementation can satisfy overlapping requirements once it is proven; do not create duplicate systems or count the same engineering work twice. Equally, do not use unchanged OCT ticket counts to imply unchanged scope.

Planning is October 8. Target implementation window is October 9–13, with a feasibility decision on October 10. This is a target, not an assertion that two large integrations and six medium ones have been estimated to fit five days. EXP-002's folder/limit mutation boundary and EXP-004's planner repair boundary require early investigation. New authority must not be improvised to meet the window.

1. Begin EXP-009's baseline and the EXP-002/004 feasibility checks. Implement EXP-001 and EXP-003 to unblock a useful first interaction and a qualified model route. Record whether the October 13 target is credible on October 10.
2. Complete the access, limit and proposal paths in EXP-002/004. Deliver EXP-005/006 against those real contracts. Small status and archive changes in EXP-007/008 may proceed when their dependencies are ready.
3. Repeat the complete journey through the browser and close EXP-009 with actual outputs and owner observations. Do not wait until all UI work finishes to discover model or tool failures.
4. Return to the existing team parking/restoration work under OCT-203, bounded recurrence under OCT-204 and packaged launch/release work. Their requirements remain open and their remaining effort must be re-estimated at the October 10 checkpoint.

The next recovery increment is sequenced after this repair, not declared completed or dropped. Its existing coordinator checkpoints remain intact. Optional motion refinement, expanded portfolio grouping, native vendor harness breadth, broader OAuth/MCP mutations and a skills/MCP storefront receive no new implementation effort during this sprint. This sequencing does not cancel any existing required commitment or silently reclassify it as optional. Existing capabilities and safeguards are retained.

This adds delivery and validation effort and consumes time previously available to the October 10/17 exits. October 25 remains the target, not a renewed feasibility promise. If the added gate and original required work cannot fit before October 18, report the concrete unfinished behavior, revised estimate and scope/date options to the owner. Do not remove either set of requirements, move the date or weaken an exit silently. Do not turn October 18–24 into an unannounced feature sprint.

## Locked interaction decisions

- Keep the map and existing focus/inspector. Begin with one prominent direction entry; no unexplained forced choice between Guide and Local assistant.
- Discussion, clarification and learning are valid activities. A useful answer does not require assignments. Plans appear when there is work worth proposing.
- The Guide can explain actual available capabilities and help form a team. It cannot invent access, team members, completed work or authority.
- Selecting a team must visibly change the recipient context. An idle team has a visible place without fabricated activity.
- Work details lead with the question, proposal, progress, decision or result relevant now. Routine configuration and raw accounting are secondary.
- Permission to share with an inference provider remains visible when needed, even when routine model selection moves into settings.
- Use functional status color with readable text/icons. Retain motion for actual activity; never make it imply success or throughput.
- Provide archive/restore for clutter. Label it honestly; permanent deletion and data erasure are outside this sprint. This is an explicit first answer to the user's removal request, not a claim that archive equals deletion.

## Exit and non-goals

The [validation protocol](validation.md) is the sprint exit. It includes orientation, brainstorming, in-product provider discovery, explicit folder access, a useful tool-backed result, real two-agent collaboration, a second concurrent effort, understandable failure, and archive/restore. A repaired single-agent chat alone cannot pass.

Protect existing scope/privacy, exact approval, budgets, idempotency, stop behavior and truthful state. Do not enlarge every limit blindly, re-run failed effects automatically, prefill fake success, hardcode the review prompts, create a second scheduler or rename a model limitation as a solved product issue.

No new provider adapter, marketplace service, broad connector suite, general live-plan rewriting, arbitrary mid-tool resume, renderer replacement, release-pipeline rebuild or organization administration is included. Actual-model trials use an explicitly bounded supported configuration; fixture tests alone cannot close usefulness.

Implementation evidence goes in this directory and links the owning ticket, revision, checks, artifacts and remaining limitations. The first-use review workspace is retained as evidence and must not be reset without the owner requesting it.
