# MVP epic: persistent, governed agent teams

Status: proposed scope and implementation plan, based on the product discussion. No runtime changes implemented by this planning update. This document and the MVP sprint sequence supersede the earlier release-scope and scheduling assumptions; the source inventory, findings and retirement evidence remain valid at their stated baseline.

## Product intent

Tetonic lets people create persistent agent teams, give them goals or ongoing responsibilities, and operate them within enforced authority, private knowledge boundaries and resource limits. Teams can work autonomously, collaborate with people, and ask for intervention when needed. The engine is general purpose. Coding and the Village are integration workloads, not universal product semantics.

First-use objective: install a supported profile, create a small team through straightforward controls, select approved tools/resources and a budget, give it a problem, and see actual work and useful results in one interface. Model/provider credentials must be available; the product must explain missing prerequisites instead of simulating progress.

## Product commitments from the discussion

1. Platform engineers configure logging, telemetry, storage, trust and execution infrastructure. Personal adoption must remain practical.
2. Both directed tasks and ongoing autonomous responsibilities are core. Human presence is not required for every step.
3. Users compose teams and can grant manager/coordinator roles. Role labels never implicitly grant permissions.
4. Larger goals can begin with a huddle: an understandable proposed outcome, work breakdown, dependencies, ownership, effort and intervention points. Accepted work becomes durable tasks, not a checklist trapped in chat.
5. A personal agent can participate in a team without exposing its private history. Sharing is explicit and scoped, including agent-to-agent communication.
6. Teams retain parked, blocked and approval-waiting work while continuing other eligible tasks. Reprioritization and restart do not erase their backlog.
7. Policy is top-down. Workstation grants can narrow organizational authority; joining a room cannot enlarge either.
8. Budget and cancellation lineage follow delegation. Stop includes children, processes and cancelable tools; external completed effects remain irreversible and unresolved effects must be reported honestly.
9. Users need one operational view with conversation, work, evidence, approvals and interventions. A full workplace suite is unnecessary for the MVP.
10. Agent workers may run locally or on servers. Inference placement is separate. Work remains on a required workstation unless explicitly transferred.

## Proposed MVP scope

| Area | Include | Explicitly defer |
|---|---|---|
| Team setup | Guided forms for members, roles, tools/MCP bindings, resources, policy and budgets | Autonomous permanent team reorganization and a full setup-assistant conversation |
| Agents | Stable identities, pinned definitions, a supported built-in harness and optional coding capabilities | Universal compatibility with arbitrary external agents/harnesses; marketplace |
| Generality | One real coding workload and one noncoding external-tool/environment workload use the same lifecycle | A catalog of vertical applications or industry-specific prompts in the core |
| Team work | Goals, huddle proposals, dynamic task dependencies, ownership, queue, parking, reprioritization, escalation | Visual workflow designer; rigid prescribed plans for every task |
| Autonomous activation | Explicit event source and schedule support, durable cursors, bounded catch-up and idle waits | Continuous model polling as the default; every possible connector |
| Knowledge | Private/team scopes, explicit publish, provenance, correction/retention, permission-filtered retrieval | Dedicated graph database or automatically global knowledge graph |
| Collaboration | Private/team conversations, mentions, shared task/proposal views and explicit cross-team request permissions | Voice/video, full document suite, chat-platform replacement |
| Overlap | Claims on declared resources/work, duplicate references, conflict notification and bounded negotiation | Guaranteed semantic detection of all overlapping real-world work |
| Limits | Token accounting, time/deadline, active-agent/concurrency and task-count limits; supported spend estimates/reservations | Exact GPU/I/O metering on every platform or exact billing for arbitrary providers |
| Oversight | Clarification, exact-scope authorization records, evidence-backed proposals, stop tree and declared resume modes | Model confidence as authorization; universal rollback or seamless process migration |
| Execution | Embedded worker; enrolled workstation and remote worker using a common assignment contract | Every OS/isolation backend supported equally at launch |
| Operations | Versioned config, bounded queues, traces/logs/metrics export, durable records, clean install/upgrade/restore | New consensus algorithm and separate Keeper daemon by default |

These scope cuts are recommendations for an achievable MVP, not claims that the user rejected future features. Names, exact UI layout and backend selection remain design decisions.

## Three milestones, with honest deployment claims

- **Local product preview (exit after MVP sprint 4):** usable small-team experience with durable scoped state and enforced limits on one machine. No HA claim. A thin UI must appear early, not only at the end. Internal vertical-slice demonstrations begin in sprint 2.
- **Distributed MVP candidate (exit after MVP sprint 6):** enrolled workstations and server workers, tested ownership/partitions, direct authorized data paths and supported isolation profiles. Explicit capacity envelope from measurement before release claims. A single-authority development deployment remains available.
- **Production release gate:** a documented HA topology and tested control-instance failure behavior are required before claiming production HA. Resolve storage/coordination design in MVP-001; implement and exercise it in MVP-701/702. Multiple API replicas alone are insufficient. If this gate is not met, ship only an explicitly limited preview, not a renamed HA release.

The plan does not assume that two replicated database engines must be implemented immediately. Select the first supported backend(s) and standalone compatibility from actual durability, installation and HA requirements. PostgreSQL is a candidate, not an already-supported fact. Preserve/import existing SQLite histories through a tested migration path.

## MVP product proof

One person creates a team, assigns a substantial goal, reviews its huddle, and leaves it working. Agents divide work within authorized roles and budgets. One task waits for approval while another completes. The same agent has a private interaction that cannot be retrieved in the shared team execution. A delegated task approaches a budget threshold and escalates without escaping its parent allocation. An overlapping resource claim is detected. The user can park/reprioritize work, inspect evidence, and stop the whole execution tree. Restart preserves work and reports the correct recovery state. Repeat the contract with both a coding task and a noncoding environment/tool task.

No success criterion depends on one particular model producing a prescribed plan. Deterministic contract tests prove enforcement; real-model trials measure whether the experience is useful. Neither substitutes for the other.

## Unresolved decisions that must not become hidden implementation guesses

- Initial OS/isolation support and the exact production HA/storage topology.
- Confidentiality promises against organization administrators versus privacy from peers; UI must state the chosen boundary honestly.
- Autonomous creation of persistent members: proposal-only by default for MVP; temporary children require explicit grants and limits.
- Who can change priorities and team authority; encode chosen permissions, not a universal manager prompt.
- Supported custom-harness contract: begin with constrained trusted integrations rather than pretending arbitrary subprocesses are contained.
- Measured target scale and initial design partners. General-purpose architecture does not establish product-market fit.

## Bloat rule

Every new subsystem must serve an acceptance scenario above, reuse an existing owner where possible, and identify what old path it replaces. No second mutable run store, fleet registry, budget ledger, identity system or knowledge universe. Generality means domain-neutral contracts, not implementing every strategy or backend in V5.
