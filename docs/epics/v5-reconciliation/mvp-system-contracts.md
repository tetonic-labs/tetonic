# MVP design constraints

These are target contracts, not an assertion of current implementation. They refine the earlier reconciliation architecture according to the product discussion.

## Control decisions versus execution data

The logical control plane owns definitions, grants, activation and accepted lifecycle state. Workers execute harnesses and enforce bounded capabilities locally. They connect directly to authorized inference, tool and scoped data services; the central API is not a proxy for every token, tool payload or telemetry event. Logs/metrics/traces export directly to configured collectors with bounded failure behavior.

One authority per record/assignment does not require one irreplaceable process. Initial local composition can embed services; the production design must specify replaceable API instances, partitionable controller ownership, durable assignment transactions and store failure behavior. Do not add a standalone consensus service without a demonstrated requirement. A central database or effect service still needs capacity/failure testing; moving arrows does not prove scale.

## Agents, teams and knowledge

An identity may participate in several teams, but each execution has an authorized information scope. Team runs must not reuse private conversation buffers, retrieved text, cached model sessions or private summaries. Destination grants constrain input assembly before inference, not only output filtering. Sharing creates an explicit published record with source/provenance and authorized content; an agent cannot launder private data by handing a summary to another agent.

Private/team knowledge can begin as scoped records, references and optional relationships. No separate graph database is required for MVP. Shared statements preserve origin, confidence/verification status and correction history. Deletion/retention and membership removal invalidate future retrieval and caches; they cannot retract content someone already legitimately received. Admin access is a separate documented policy, not an implied end-to-end confidentiality promise.

Cross-team messages have authenticated sender, recipient, purpose, resource grants and origin work lineage. Model-generated instructions from tools or other agents are untrusted content; they cannot elevate authority. Requests may be rejected, negotiated or escalated. Unsupported cross-scope transfer fails rather than falling back to an unrestricted conversation.

## Persistent responsibilities and huddles

A team charter is ongoing intent, not a continuously open execution. Goals and work items persist independently of worker lifetime. An activation creates bounded attempts against selected work. Event cursors, timers and queue changes wake eligible work; idle teams do not need constant inference.

Huddles propose versioned work breakdowns with dependencies, ownership and budget allocations. They may proceed automatically within preauthorized scope; human approval is conditional on policy/risk. Recording a plan does not itself grant action permission. Replanning retains task history and prevents duplicate activation through optimistic versions/idempotency.

Parked and approval-waiting work does not block unrelated eligible work. Resume checks resource/definition/policy changes and previous effects before continuing. An agent can propose additional tasks but cannot silently expand a team's charter or permanent membership. Leadership permissions are explicit, and human overrides follow permission rules.

## Budget, overlap and execution lineage

Track origin organization/team/goal, parent request, payer allocation and stop scope across both new subagents and requests to existing peers. Reserve before dispatch, settle measured usage, and release on terminal/quiescent state as appropriate. Denial cannot be bypassed by asking another team to execute; either an explicit authorized transfer is recorded or the request remains charged/limited under its origin. Cross-organization delegation is out of the initial supported profile.

MVP dimensions are bounded time, tokens, concurrency and task count; spend can use a declared rate estimate plus limits on enforceable provider usage. Missing price or usage data is unknown, not zero. Delegation depth/fan-out and queue limits prevent unbounded work explosions. Default no speculative duplicate effectful attempts.

Overlap coordination uses declared resources and task references, with explicit write claims or isolated workspaces where applicable. It cannot detect all semantic conflicts. Report uncertainty and escalate unresolved conflicts. Agent negotiation cannot authorize data disclosure or override resource locks.

## Workstations and stop semantics

Employee sign-in, worker/device enrollment and agent execution identity are distinct. Enrolled workstations establish outbound authenticated connections, expose only approved resources and default to owner-initiated work. Shared execution requires explicit grants. Organization ceilings intersect local consent. Offlining a required workstation parks work; it does not authorize upload or relocation. Remote inference disclosure remains separately governed.

Stop scopes cover org, team, goal, agent and attempt using persisted lineage and generation checks. Pause closes new admission and requests supported checkpoints; cancel ends selected work; emergency stop revokes new effects and terminates reachable supported process trees. In-flight remote API actions may be noncancelable. Record requested, acknowledged, quiescent and unresolved states, with bounded escalation and honest limits during partitions.

## Human interface

A small product UI combines guided team setup, a private/team conversation, durable work/huddle cards, approvals and inspection. Messages identify whether they are discussion, assignment, steering or authorization. Do not turn every utterance into a broadcast activation. Users manage outcomes without opening one tab per agent; agent-level traces remain inspectable. Visual polish follows a real data contract, not synthetic status cards.
