# Shaping work, resolving missing capabilities and using skills

October 4, 2026; implementation update October 5. User-required refinement of the
orchestration/MVP plan. The first local exploration and durable brief slice is
implemented; the remaining requirements below are still planned. See
[implementation evidence](shaping-evidence-2026-10-05.md).
Extends existing OCT-102/103/104/105 and COORD-A/B/C
ownership; no new parent sprint or independent control plane.

## Start before the outcome is clear

A person can arrive with a question, uncertainty or a poorly understood problem.
They must be able to learn enough to shape the work, not simply supply a complete
brief for the orchestrator. Success can be a better understanding or a decision
to do nothing. Producing tickets is not the objective of every conversation.

Use the same workspace and composer. An invitation such as "What are you working
through?" supports both exploration and a direct assignment. A contextual "Help
me think this through" affordance can clarify intent without requiring a mode
wizard. Explain when work will start; discussing a possible action is not its
authorization. Clear, already-authorized tasks can proceed without a compulsory
shaping session. A standing autonomous mandate may include exploration and
planning without requiring a human to attend every huddle.

The orchestrating agent helps explain unfamiliar concepts, investigate evidence,
challenge assumptions, compare feasible approaches and identify the decisions
that matter. It asks a few timely questions instead of a long intake form. It
distinguishes source facts, interpretations, assumptions and unknowns, adapting
explanation depth to the person. The user can change direction without starting
over or losing why earlier choices were made.

Exploration can use permitted reads and bounded research assignments. These are
real budgeted work with their own acknowledged scope and lineage; they are not
free hidden preprocessing or permission to begin consequential implementation.
Helping someone understand Trello does not authorize editing their Trello board.
Any specialist research uses the same governed child-admission path as execution.

## One evolving plan with progressive detail

Maintain a versioned brief alongside the discussion and link it to the existing
goal/huddle/work records. Show a short current understanding and the next useful
decision first; keep the full evidence and plan available on demand. Avoid a new
permanent dashboard of fields. The underlying record should retain:

- The problem as understood, intended outcome and meaningful success criteria.
- Source evidence, uncertainties and assumptions still needing investigation.
- Alternatives considered, the person's choices and their stated reasons.
- Constraints, non-goals, priorities and the approved resource envelope.
- Proposed assignments, dependencies, team/agent choices, selected skills and
  needed tools/connections, each labeled as proposed or actually available.
- Unresolved prerequisites and what can proceed independently.

The model may suggest edits; it cannot silently replace the person's words or
turn a suggestion into an accepted decision. Pin the accepted plan revision to
its work and assignments. Starting the authorized part of a plan can leave other
branches exploratory or blocked. Conversation, planning, research and execution
can coexist; they are not a mandatory one-way funnel.

For material changes, show a compact change summary: what changed, why, affected
work, additional resources and what is still running. Engine receipts distinguish
accepted edits from pending application at a safe boundary. No claim that an
already-running external action changed retroactively. Preserve personal shaping
discussion in its own information context; publish only explicitly shared brief
content/evidence into team context. Adding an agent to a team does not share the
person's unrelated conversation history.

## Missing capabilities are part of the work

The orchestrator must examine prerequisites during shaping and recheck them at
dispatch. Workers can discover additional needs later. Report the actual gap:
missing source context, absent connector, missing permission, expired credentials,
unsupported operation, unavailable execution environment, or insufficient budget.
An installed connector and a running MCP server are not proof that a particular
agent may use a particular resource or operation.

Create a durable work-scoped capability request through the existing human-control
and resource authorities. It records originating work/plan revision, requester,
affected assignment, exact resource and operation, justification, required
read/write scope, intended duration, capable approver/setup owner and alternatives.
Never include secret values in the request or model transcript. If no supported
adapter exists, say so rather than presenting a working Connect button.

Example brief for a missing Trello source:

> To compare the backlog, this team needs read access to the Delivery board.
> No board changes are needed. Connect an available account, provide an export,
> or continue without this source. Only the backlog comparison is waiting.

The flow is request -> awaiting setup/decision -> access validated or denied ->
affected work rechecked and resumed, revised or parked. A user who cannot grant
the access can route the request to an authorized owner; the engine does not
escalate their privilege. Separate the local owner's setup path from future
organization workflows. Do not send external notifications without configured
authority. Existing org policy can preauthorize routine requests without a human
confirmation for every tool call.

Connection setup uses Tools & MCPs and host-managed credentials; secrets do not
pass through the conversation. A successful login alone does not close the
request: verify the required permitted operation, recheck current work intent
and admission, then acknowledge readiness. Deduplicate repeated requests and
apply expiry/cooldowns; deny/revoke must survive restart. Unrelated work proceeds.
An agent may not evade denial through another agent or a broader credential.

## Skills are a required baseline

Keep these concepts distinct in the resource library and the orchestrator's
context:

| Concept | Meaning | Example |
|---|---|---|
| Skill | Reusable task guidance and supporting resources | How this team triages customer feedback |
| Tool / MCP connection | Operations available through an execution interface | Fetch cards from a board |
| Grant | Enforced authority to perform specified operations on a resource | Read this board, without changing it |
| Context | The relevant facts supplied or retrieved for this work | The board contents and the team's priorities |

A skill does not automatically confer tool access or install a connector. An
agent can know how to do something and still require permission or setup.

Use the open [Agent Skills format](https://agentskills.io/specification) as the
interchange target, checked October 4: a package has `SKILL.md` metadata and
instructions, optionally with references, assets and scripts. Support discovering
descriptions and loading relevant instructions/resources as needed, rather than
inserting every skill into every prompt. The following controls are Tetonic design
requirements, not guarantees supplied by the file format.

Required first profile:

- Create and edit a skill directly, with optional agent assistance. Export the
  authored package so it is portable. Capture a method from work only with the
  right audience and a review for private context; do not silently publish chats.
- Import a local/downloaded package and support one explicit source-fetch path
  for a user-selected package. No marketplace, rating system or automatic crawler
  is required. Downloads obey host network controls and limits; importing content
  never executes bundled code or installs its dependencies.
- Inspect instructions, source, declared compatibility and needed capabilities
  before enabling the package. Preserve a content digest, source/license metadata
  when supplied, local revision and scope. New revisions require deliberate
  adoption; pin active work so an update cannot silently change its instructions.
- Make skills available in personal, team or organization scope through existing
  membership/resource controls. The orchestrator and workers can select relevant
  enabled skills. Team defaults simplify setup without copying private content or
  implying that all members were granted its tools.
- Load approved skill content through the same governed invocation/context path,
  record which revision was used, and allow disable/removal with explicit effects
  on future and active work. Skill instructions have no authority to relax policy,
  disclose private data, create credentials or increase budgets.
- The initial executable subset remains the validated engine tool profile.
  Imported scripts may be inspected but run only through an explicitly supported
  sandboxed execution path; otherwise mark that skill requirement unsupported.
  Reject path escapes and invalid/oversized packages. Treat `allowed-tools` or
  compatibility metadata as declarations to resolve against actual policy, not
  permission grants. No claims that arbitrary downloaded skills will run.

Present Skills beside connections/tools in the existing resource library, with
simple Create, Import and availability controls. Agent/team detail can select
skills there. Preserve one management overlay rather than adding another primary
workspace users must learn. In a plan, show "available", "needs connection" or
"needs approval" as recorded, instead of implying setup from a named skill.

## Reuse and gaps found in source

- Existing scoped transcripts and `ContextService` provide discussion/history
  owners. Goal, work and `HuddleProposal` records provide planning anchors, but
  huddles currently accept work titles, not the full evolving brief above.
- `web/src/components/work/GuideIntake.tsx` contains fixed Research/Analysis
  proposals and synthetic fallback responses. It is not a production shaping
  implementation. Reuse only useful presentation after removing those assumptions;
  do not reconnect that dispatch path as a shortcut.
- `ResourceService`, human controls, execution grants and broker enforcement are
  the owners to extend for capability requests and acknowledgement. ToolsView is
  currently a preview library of samples/drafts; it is not a live setup service.
- `DomainPack` / `SpecialistPack` in `tetonic-orchestrator` supply compiled domain
  behavior and role overlays. They are not a user-installable skill package loader.
  A Rust-source search under `engine/` found no `SKILL.md` importer/discovery
  implementation; this is new functionality with existing runtime integration.
- Scoped artifacts, `bind_compiler`, registered revisions and general-harness
  preparation are candidate reuse points for skill content and invocation
  bindings. The registered executor currently sets `context_compiler: None`;
  merely adding a skill file or library UI will not apply it during execution.
  Implement scope/pinning contracts in these owners, not a second agent runtime.

## Acceptance and sequencing

The user has made skill creation/import and problem shaping baseline requirements.
They are P0 additions to existing parents, not optional OCT-106 customization.
Measure their effort at the October 6 scope review; the previous schedule cannot
be treated as proof they fit. Defer optional work first and report any remaining
October 25 conflict explicitly.

| Existing owner | Additional acceptance |
|---|---|
| OCT-103 / UI-002/003 / COORD-B | Resume a real multi-turn exploration after reload, explain alternatives with sources, retain human choices, and turn only the accepted scoped plan into assignments. Later re-shape it without losing decision history or inventing applied changes. |
| OCT-102 with OCT-103 bindings | Create one skill; import another via the supported package/download path; inspect/enable it; bind its exact revision to a real agent run. Prove contextual use, source integrity, scoped access and denial of unavailable capabilities. Unsupported executable requirements stay explicit. |
| OCT-105 with OCT-102 resources | Encounter an absent source/permission; show one actionable request; supply an authorized connection or alternative; resume only affected work after validation. Cover denial, expiry, revoked permission and unavailable setup owners. |
| OCT-104 / UI-004/005/008 | Same workspace shows draft versus accepted plan, actual skill/resource readiness and concise access requests, with detailed reasoning/evidence available on demand. No forced wizard or extra main screen. |

Use a deliberately unclear user problem for the real-model shaping scenario;
do not provide a prewritten decomposition. Observe whether a fresh person can
understand the options and explain the consequential choices before dispatch.
Use deterministic tests for no unintended dispatch, stale plan edits, private
discussion isolation, duplicate access requests, denial/proxy bypass, credential
secrecy, skill path traversal, import without execution, pinned revision changes,
disabled skills and hosted-context disclosure. Tests and usability evidence remain
separate. The linked evidence records only the implemented slice, not completion
of these acceptance criteria.
