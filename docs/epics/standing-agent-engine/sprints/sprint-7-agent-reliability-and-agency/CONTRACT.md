# Shared contract decisions for Sprint 7 / Village Sprint 4

Status: Proposed design requirements; finalized by SAE-701 and implemented by VG-401.

## Responsibility boundary

| Concern | Tetonic | Village |
|---|---|---|
| Truth | Preserves origin and evidence; never invents action success | Owns authoritative physical and social delivery state |
| Memory | Stores/selects agent experiences, beliefs and intentions | Emits local observations/outcomes; keeps world audit history |
| Senses | Budgets and processes delivered perceptions | Computes visibility, hearing, local tools and access rules |
| Actions | Chooses, validates envelope, correlates and dispatches | Validates actual preconditions and applies effects |
| Scheduling | Manages inference, waits, cancellation and retries | Emits task completion, stimuli and meaningful world changes |
| Persistence | Agent checkpoint and delivery cursor | World snapshot and mutation log |
| Operator UI | Supplies health and generic control capabilities | Renders experiment controls, state and evidence |
| Preferences | Optional agent configuration and internal state | Optional physical needs and environmental conditions |

## Required envelope semantics

These are proposed fields/semantics, not an already supported wire schema.

- Identity: protocol version, world/session ID, world epoch, agent ID and configuration revision.
- Observation: monotonic sequence, observed tick/time, sensing scope and current local state. Unknown is explicit; absent is not automatically zero or false.
- Event: stable event ID, type, origin, occurrence time, delivery audience and relevant entity/action/task IDs.
- Action: unique action ID, originating decision ID, observation reference, context revision and parameters validated against advertised capability schemas.
- Result: accepted/rejected plus structured reason and factual effects; asynchronous task completion has its own event.
- Lifecycle: per-agent runtime state, last success, current decision and classified error/recovery metadata.
- Traces: source cursor and bounded capture metadata, effective generation options, finish reason and partial-decision flags.

Use additive migration where feasible. Never invent global facts to make an old client's required fields appear populated.

## Receipt and memory rules

An accepted navigation starts a task; it does not establish arrival. A spoken statement establishes an utterance and its recipients; it does not establish the statement's truth. A thought/intention establishes agent-owned state; it does not mutate the physical world. A perception-delivery acknowledgement establishes retention or processing as explicitly specified; it does not establish understanding.

The Village must not synthesize a resident's mental model from world truth. Tetonic must not query spectator APIs to complete missing memories. Agent beliefs may be incorrect, but provenance must remain available for inspection.

## Delivery and scheduling

Snapshots may be coalesced. Relevant events must be retained until the agreed acknowledgement point, deduplicated by ID and bounded with explicit overflow policy. Decide whether acknowledgement follows durable retention or decision inclusion; trace both separately where available. A pending critical event must not be silently discarded by context compaction.

Per-agent decisions are serialized. Operator E-stop takes precedence over inference completion. Stale context/version checks prevent actions planned under obsolete control state. Ordinary world changes still require fresh server-side action validation; a version field is not sufficient collision/resource protection.

## Intervention semantics

1. Public world stimulus: real notice/resource change, discovered under local sensing rules.
2. Private external message: attributed information for one agent.
3. Assigned objective: explicit directed-mode control.
4. Internal-state experiment: separately enabled, validated edit to allowed agent-owned fields with a before/after audit record.

None of these implies compliance. Editing internal state must not be implemented by smuggling an operator message into a fabricated external observation.

## Decisions to close during SAE-701

- Supported protocol version and compatibility/upgrade order.
- Token counting capability/fallback, completion reserve and accounting margin.
- Delivery acknowledgement point and overflow/backpressure behavior.
- Event retention versus checkpoint durability and replay cursors.
- Action idempotency lifetime and ambiguous transport result resolution.
- Allowed structured-output schema and provider capability fallback.
- Cross-repository fixture ownership/version distribution.
- Separation of current-state truth, remembered evidence and self-authored intentions.

Record decisions, rationale and alternatives here. An unresolved decision blocks its dependent implementation, not independent source audits or tests.


## Accepted local event contract — 2026-09-24

The first implemented capability version is event_protocol=1 in the WebSocket URL. The experiment gateway requires it; unknown versions or missing version close with 1008. Non-experiment legacy clients retain their previous behavior. This is a local capability version, not a claim that the entire TWP schema migration is complete.

Perception state.data.delivery includes protocol, world_session, ack_policy=parsed_decision, pending and dropped_total. Event payload._delivery carries stable id, world_session, agent_id and occurred_tick. Snapshots repeatedly offer the oldest two pending events. events_ack.data carries world_session, event_ids and decision_id. Socket identity determines the acknowledging agent. IDs not offered to that recipient cannot consume events. A successfully parsed decision queues acknowledgement; failed/oversized/truncated decisions do not. This establishes decision inclusion only, never belief/compliance or durable storage.

Both repositories carry identical event-delivery-v1.json fixtures. Tetonic's fixture is engine/core/tetonic-runtime/tests/fixtures/event-delivery-v1.json; Village's is world/tests/fixtures/event-delivery-v1.json. Tests parse and exercise the wire shape. Village state schema is village_local_v4 after removing duplicate private_suggestions/recent_experiences fields.

World events are retained per recipient, max 128. Overflow drops oldest with an explicit trace containing lost IDs and persistent dropped_total; no silent loss guarantee is made beyond that bound. Acknowledgement transport queue is bounded; if full/disconnected, the world retains events and offers them again. Runtime memory is run-scoped and acknowledges after a parsed decision; world/engine restarts are not yet durable.

Action IDs are protected by a world-owned run ledger (4096 entries, no eviction). Exact repeated parameters return original receipts; conflicting reuse and capacity overflow reject new mutations. Replay protection ends at world reset; durable replay protection is pending.

Upgrade order for this local slice: stop the managed pair, rebuild both, then start world and engine. A mixed old/new experiment client fails explicitly. Full schema advertisement, durable acknowledgement and generic protocol-error health reporting remain open.
