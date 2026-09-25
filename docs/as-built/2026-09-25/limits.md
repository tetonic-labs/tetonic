# Disconnected implementations, inconsistencies and limits

[Overview](README.md) · [Verification](verification.md)

This page describes current implementation limits. It is not a redesign or enhancement backlog. Negative wiring statements are bounded to the analyzed entrypoints and call-site searches; dynamic plugin loading or external consumers were not established.

| Implementation | What exists | What the current wiring does not establish |
|---|---|---|
| `tetonic-server` | One configured world agent, direct model provider, local health/trace server | Distributed placement, durable run recovery, fleet lifecycle or agent-state migration. |
| `FleetManager` / `FleetSupervisor` | Organization/squad/agent records, status/control methods, optional adapter/perception handles | Creating a record marked Running does not start a task. The creation path registers optional runtime handles as absent. |
| Keeper registry / runner registration helpers | In-memory node registration, proof/epoch and assignment bookkeeping | An executable keeper service, consensus, persistent membership or a production heartbeat network loop. |
| Checkpoint facilities | File-based state serialization/checks | Automatic snapshot/load of the standalone world's running brain. |
| Operator-control and thought-stream modules | Library control/observability APIs | A corresponding public server route for every library method. |
| Generic domain configuration | Engine-oriented config types | Replacement of the standalone server's private TOML structs. |
| Brain variants / stream/composite adapters | Alternative library strategies/transports | Activation in the server, which explicitly constructs PerceptiveBrain + SingleModelBrain + WebSocket adapter. |

Evidence: [main.rs — `async fn main`](../../../engine/mantle/tetonic-server/src/main.rs#L75), [fleet_api.rs — `pub struct FleetManager`](../../../engine/litho/tetonic-app/src/fleet_api.rs#L102), [fleet_supervisor.rs — `impl FleetSupervisor`](../../../engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs#L103), [role.rs — `impl KeeperRegistry`](../../../engine/mantle/tetonic-node/src/role.rs#L152), [operator_control.rs — `impl OperatorController`](../../../engine/litho/tetonic-app/src/operator_control.rs#L99), [thought_stream.rs — `pub`](../../../engine/litho/tetonic-app/src/thought_stream.rs#L20), [lib.rs — `pub mod`](../../../engine/core/tetonic-runtime/src/lib.rs#L3).

## Fleet state is not execution state

Fleet registration writes management records and status; optional adapter/channel handles determine whether steering can reach a running world loop. An API response labeled Running therefore does not establish inference or action execution. The application composition root has no FleetManager field, and standalone main constructs its agent directly. [fleet_api.rs — `pub struct FleetManager`](../../../engine/litho/tetonic-app/src/fleet_api.rs#L102), [fleet_supervisor.rs — `impl FleetSupervisor`](../../../engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs#L103), [lib.rs — `pub struct Application`](../../../engine/litho/tetonic-app/src/lib.rs#L124), [main.rs — `let agent`](../../../engine/mantle/tetonic-server/src/main.rs#L170).

The organization/squad implementation holds quotas and a shared workpad in memory. A quota field is only a declared limit until the relevant operation enforces it; similarly, a token counter does not by its name establish a rolling hourly window. These structures are not evidence of an operational multi-tenant distributed quota service. [fleet.rs — `pub struct`](../../../engine/mantle/tetonic-orchestrator/src/fleet.rs#L34).

## World delivery does not provide global exactly-once effects

The adapter has bounded volatile queues, drops new perceptions when full, and does not replay pending actions on disconnect. Event acknowledgement is queued separately and means decision inclusion. Correlated receipts improve visibility, but no atomic transaction spans model decision, local ack queue, world action, world persistence and Tetonic process restart. Consequently this implementation does not establish exactly-once external effects or crash-safe agent memory. [websocket_adapter.rs — `pub fn connect`](../../../engine/core/tetonic-runtime/src/websocket_adapter.rs#L38), [websocket_adapter.rs — `pub fn acknowledge_events`](../../../engine/core/tetonic-runtime/src/websocket_adapter.rs#L158), [perceptive_brain.rs — `impl Brain for PerceptiveBrain`](../../../engine/mantle/tetonic-server/src/perceptive_brain.rs#L168).

The engine accepts world-supplied state JSON and action receipts. Whether a receipt corresponds to physically legal movement, a visibility-constrained observation or durable inventory change is an external-world contract. This package does not claim to audit The Village repository, its Pixi renderer or Colyseus simulation. Those need their own revision-bound evidence if included in a larger system architecture.

## Recovery and distributed guarantees

Durable supervisor serialization is per process and per run; it is not a multi-writer consensus algorithm. Fabric workers provide remote compute and result validation, not transparent movement of live conversations, world adapters or model-local caches. Local keeper maps are not interchangeable with the durable attempt lease/journal. [service.rs — `fn run_lock`](../../../engine/mantle/tetonic-run/src/service.rs#L97), [compute_plane.rs — `pub struct ComputePlane`](../../../engine/litho/tetonic-app/src/compute_plane.rs#L29), [job_ingress.rs — `impl JobIngressManager`](../../../engine/mantle/tetonic-node/src/job_ingress.rs#L18), [role.rs — `impl KeeperRegistry`](../../../engine/mantle/tetonic-node/src/role.rs#L152).

Worker ingress initialization loads persisted dedup records but includes fallback/filtering behavior for unavailable or invalid stored entries. That behavior must not be summarized as unconditional crash-safe deduplication. Exact storage-fault/restart outcomes need controlled probes against the deployed storage/environment. [job_ingress.rs — `impl JobIngressManager`](../../../engine/mantle/tetonic-node/src/job_ingress.rs#L18).

## Other important limits

- State enums and exported methods include paths beyond the normal caller chain; the diagrams do not imply every variant is reachable from every executable.
- Context accounting is implemented in more than one place and uses estimates in some paths. An input budget is not proof of exact model-window compliance for every tokenizer/model combination.
- CI tests a selected package list and builds two product executables in its matrix; this is not whole-workspace or world-experiment integration coverage.
- OS-specific sandbox implementations are present, but only source inspection is represented here. Cross-platform enforcement and adversarial cleanup have not been rerun for this package.
- A lexical inventory cannot prove absence of dead code, complete macro-expanded call relationships, or all feature-dependent behavior.

Evidence: [run.rs — `pub enum AttemptState`](../../../engine/core/tetonic-domain/src/run.rs#L53), [context_budget.rs — `struct`](../../../engine/mantle/tetonic-server/src/context_budget.rs#L7), [engine-ci.yml — `Run Core Tests`](../../../.github/workflows/engine-ci.yml#L51), [mod.rs — `pub fn platform_backend`](../../../engine/core/tetonic-sandbox/src/backend/mod.rs#L58).
