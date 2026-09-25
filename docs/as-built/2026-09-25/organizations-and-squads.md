# Organizations, squads and fleet management

[High-level overview](README.md) · [Execution paths](execution.md) · [Implementation limits](limits.md)

## What each object represents

| Object | Owned state | Responsibility |
|---|---|---|
| Organization | ID/name, quota fields, atomic token counter, squad map | Group squads and maintain organization-level accounting. |
| Squad | ID/name, organization ID, locked charter, member-ID list, workpad | Group agents around shared intent, boundaries and peer information. |
| IntentCharter on a squad | Strategic intent and operational boundaries | Supply shared direction; squad steering mutates boundary settings. |
| SharedWorkpad | Locked vector of author/subject/body/time bulletins | Local post/read-all/clear interface; no automatic subscriptions or prompt injection. |
| FleetManager | Organization/squad maps, squad-to-org map, agent response records, supervisor reference | Create/look up management objects and dispatch REST-shaped requests. |
| FleetSupervisor | Organization and managed-agent maps, global stop switch | Track status/heartbeat and deliver controls through optional runtime handles. |
| ManagedAgent | Agent ID, optional squad ID, status, last heartbeat, optional adapter/channel | A supervision record; it does not itself contain or start an Agent/Brain loop. |

Evidence: [fleet.rs — `pub struct Organization`](../../../engine/mantle/tetonic-orchestrator/src/fleet.rs#L51), [fleet.rs — `pub struct Squad`](../../../engine/mantle/tetonic-orchestrator/src/fleet.rs#L162), [fleet.rs — `pub struct Bulletin`](../../../engine/mantle/tetonic-orchestrator/src/fleet.rs#L128), [fleet_api.rs — `pub struct FleetManager`](../../../engine/litho/tetonic-app/src/fleet_api.rs#L102), [fleet_supervisor.rs — `pub struct ManagedAgent`](../../../engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs#L33), [fleet_supervisor.rs — `pub struct FleetSupervisor`](../../../engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs#L91).

## Creation and membership

`create_org` rejects a duplicate organization ID, constructs its quota with the requested token ceiling and an active-agent field of 50, then registers the same organization object with the supervisor. `create_squad` verifies the organization exists, checks squad-ID uniqueness across the manager, constructs an IntentCharter from `domain_context`, registers the squad with the organization and updates manager indexes. Squad membership is a deduplicated vector of AgentIds. [fleet_api.rs — `pub async fn create_org`](../../../engine/litho/tetonic-app/src/fleet_api.rs#L122), [fleet_api.rs — `pub async fn create_squad`](../../../engine/litho/tetonic-app/src/fleet_api.rs#L175), [fleet.rs — `pub fn add_member`](../../../engine/mantle/tetonic-orchestrator/src/fleet.rs#L189).

`create_agent` requires a nonempty list of world-adapter names and resolves the organization through its squad mapping. It records a fixed creation cost of 1000 tokens **before** checking for a duplicate agent ID. It then registers a ManagedAgent with no actual adapter or perception channel, saves an AgentResponse and adds the ID to squad membership. The role and adapter names are response metadata on this path; they do not construct a brain or world connection. An optional agent charter is used to report its boundary count, not installed into an executing agent here. There is no automatic squad-charter inheritance into a new live Brain in this method. [fleet_api.rs — `pub async fn create_agent`](../../../engine/litho/tetonic-app/src/fleet_api.rs#L251).

`get_agent` reconciles its response status from the supervisor record. Status can therefore reflect a management control change, but Running is initially assigned by `ManagedAgent::new`, not inferred from an executing task. Organization/squad counts derive from their respective manager/maps/member records; these are not measurements of CPU execution or active inference. [fleet_api.rs — `pub async fn get_agent`](../../../engine/litho/tetonic-app/src/fleet_api.rs#L326), [fleet_supervisor.rs — `impl ManagedAgent`](../../../engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs#L42), [fleet.rs — `pub fn total_agent_count`](../../../engine/mantle/tetonic-orchestrator/src/fleet.rs#L103).

## Shared direction and communication

`inject_steering` finds the squad in registered organizations, applies boundary adjustments to its charter, converts the steering vector into a world event, and awaits a send to each member that has a registered perception sender. It returns the number of successful channel sends. That count is not confirmation of model consumption, acceptance of the directive or resulting world effects. Without attached senders, charter mutation can occur with zero deliveries. [fleet_supervisor.rs — `pub async fn inject_steering`](../../../engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs#L156), [fleet.rs — `pub fn apply_steering`](../../../engine/mantle/tetonic-orchestrator/src/fleet.rs#L218).

The workpad stores bulletins until explicitly cleared; `read_all` clones the current vector. It has no bound, durable storage, automatic model-context inclusion or membership check in `post` itself. Posting an AgentId attributes a bulletin; it does not authenticate its author. This describes the primitive's behavior, not an externally authenticated squad messaging service. [fleet.rs — `impl SharedWorkpad`](../../../engine/mantle/tetonic-orchestrator/src/fleet.rs#L135).

## Budgets, controls and persistence boundaries

The token field is named `max_tokens_per_hour`, but `record_tokens` increments a cumulative counter and checks it against the ceiling without implementing a timed reset. Exceeding the ceiling returns an error after incrementing; it does not roll the increment back. `max_active_agents` is stored but is not checked in this agent-creation method. These organization counters are distinct from the compute broker's inference reservations; this creation path does not connect them to measured model usage. [fleet.rs — `pub fn record_tokens`](../../../engine/mantle/tetonic-orchestrator/src/fleet.rs#L86), [fleet_api.rs — `pub async fn create_agent`](../../../engine/litho/tetonic-app/src/fleet_api.rs#L251).

Fleet stop/resume changes managed-agent status and invokes adapter controls when handles are present. Heartbeat health reads timestamps supplied through explicit heartbeat calls. These management records are in memory behind locks; this code does not persist them through the durable run supervisor. Nor does registering a record create an automatic heartbeat producer. [fleet_supervisor.rs — `pub fn emergency_stop_fleet`](../../../engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs#L210), [fleet_supervisor.rs — `pub fn resume_fleet`](../../../engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs#L226), [fleet_supervisor.rs — `pub fn record_heartbeat`](../../../engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs#L59).

## API shape and executable wiring

`dispatch_rest` is a local async dispatcher for method/path/body arguments. It recognizes organization, squad and agent creation/lookup paths under `/api/v1`. A dispatcher with REST-shaped paths is not itself an HTTP listener. For the nested agent-creation route, the organization path segment is ignored; the method resolves the actual organization from squad membership. No tenant-authentication layer should be inferred from that URL structure. [fleet_api.rs — `pub async fn dispatch_rest`](../../../engine/litho/tetonic-app/src/fleet_api.rs#L342).

The inspected Application composition and standalone server main do not construct FleetManager/FleetSupervisor to own their running agents. The world server creates one Agent directly. The management hierarchy is therefore an implemented library subsystem with a missing connection to those product execution paths. Its presence belongs in the high-level system model even though those runtime integrations remain absent. [lib.rs — `pub struct Application`](../../../engine/litho/tetonic-app/src/lib.rs#L124), [main.rs — `let agent`](../../../engine/mantle/tetonic-server/src/main.rs#L170).
