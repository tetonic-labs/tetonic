# Shared work, blackboard and director context

October 4, 2026. Product refinement of UI-008 and COORD-A/B/C; not a new sprint.
The working prototype is `/dev/team-work/`. Its activity remains illustrative.

## Product contract

- **Map:** work relationships determine placement. Reserve destination docking
  areas and route dependencies through gutters. Directional edges express a
  supplied dependency, not a conversation parent or a requirement that all work
  in a stream stop. Missing/cyclic dependencies require an explicit warning.
- **Areas of work:** one human goal/area can contain parallel projects owned by
  different teams. Group by an explicit relationship, not inferred similarity.
  Color reinforces a written label, using restrained copper/green/ink brand
  tokens. Areas, projects, teams and agent identities remain distinct.
- **Blackboard:** original shared contributions, tool outputs and operator
  direction, formatted for reading without rewriting them into project cards.
  Show author, scope, ordering and source identity. This is shared output, not
  private model reasoning or an agent's unrelated personal conversations.
- **Ask about work:** an operator can ask about a project, area or permitted work
  across teams from the main composer. The director consults bounded, current,
  read-only context and cites original records. Asking a status question never
  dispatches tasks, approves effects or expands the operator's access.

## Implemented in the product example

`projectLayout.ts` replaces fixed coordinate arrays with stable topological
ordering, a folded two-column layout, reserved destination slots and orthogonal
dependency routes. Labels live in the inspector instead of crossing nodes. The
layout supports additional nodes and multiple dependencies. Dense edge crossings,
large graph virtualization and preserving every node's position after topology
edits are not solved by this example.

`ProjectBlackboard` renders original authored fixture messages and tool results.
Example operator directions are appended in this tab and scoped to the selected
project; no live agent receives them. Playback exposes only messages available at
that example event. The overview groups Product and Research under Customer
experience and Operations under Reliable operations.

`workContext.ts` supplies a reusable read-only projection with source IDs,
project/team/area scope, revision, observed-at metadata, completeness and bounded
retrieval. The prototype's Ask mode searches those records and opens exact
sources. It explicitly says no model is connected; it is not a natural-language
answering implementation. Frontend scoping is a presentation feature, not an
authorization boundary. A production adapter must supply authorized records.

## Reuse the existing engine authorities

Static inspection on October 4 confirms these integration points:

| Need | Existing implementation to extend |
|---|---|
| Team work and explicit goal/run relationships | `ResourceService::list_team_work_items` in `engine/litho/tetonic-app/src/resources/team_work.rs` |
| Authorized run state | `ContextService::inspect_run` in `resources/run_inspection.rs`, backed by `DurableRunReader` |
| Lifecycle replay and retention gaps | `ContextService::replay_run` / `poll_run`; retain their sequences and gap result |
| Shared authored output | `ContextService::transcript` in `resources/contexts.rs`, backed by scoped transcript storage |
| Accepted artifacts | `ContextService::bind_artifacts` and the existing artifact store |
| Local product adapter | `local_workspace.rs`, `local_workspace/workroom.rs`, and `web/src/lib/localEngine.ts` |

`inspect_run` already checks membership and organization/context bindings and
rechecks before release. It does not provide an atomic revocation fence over
delivery. Reuse and strengthen that boundary where needed; do not introduce an
organization-wide director credential or assume visibility from agent membership.

The local task reader currently calls `transcript(..., 100)`, keeps assistant/tool
roles and reads a bounded accepted artifact. That projection is not a complete
team blackboard. The current transcript method exposes a limit but no cursor.
Lifecycle replay is not a replacement for original message history or model-token
streaming. Existing local digest output is also a bounded summary, not this tool.

## Integration slices and acceptance

1. **Durable relationships and shared-output reader.** Extend the existing team
   work records with explicit area/project/dependency relationships where missing.
   Validate organization/context membership and cycles on writes. Add cursor-based
   shared transcript reads with stable message IDs, source scope, revisions,
   original content, supported roles, continuation and retention-gap metadata.
   Preserve timestamps/causal references across runs; do not invent one global
   event order or convert private discussions into shared contributions.
2. **Read-only work-context tool.** Add a governed tool over these readers, with
   authenticated principal, requested scope, query/filter, cursor and bounded
   record/token budget. Return only authorized sources, version/freshness,
   completeness, citations and continuation. Reauthorize source access and links;
   check revocation before returning content. Large scope queries must be paged,
   not a full-fleet prompt dump. Query execution itself uses a budget.
3. **Director invocation and UI.** Connect Ask mode to a configured model through
   the existing inference/broker path, with this read-only tool. Treat shared
   records as untrusted data, preserve citations and distinguish observations
   from interpretations. Hosted inference must obey disclosure policy. Invalid or
   stale citations stay explicit; unavailable history cannot become a confident
   answer. Keep the write-capable direction/approval paths separate by authority.
4. **Proof.** Use two actual agents on shared work. Verify original messages,
   handoffs and status are consistent between map, blackboard and cited answers.
   Exercise multiple teams contributing to an area; partial/retained histories;
   unknown scope; revocation during lookup; cross-context/private source denial;
   retries without duplicate messages; hostile instructions inside source output;
   unavailable inference; and questions that must not mutate work. No query may
   silently widen scope or imply a successful external action.

Reuse takes priority: no second run store, transcript journal, agent scheduler,
approval service, team registry or budget ledger is introduced by this plan.
