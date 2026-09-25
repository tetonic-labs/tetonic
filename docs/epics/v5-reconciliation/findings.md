# Source findings

Paths below are source evidence at the report baseline. Claims about absent wiring mean no production caller found in the searched repository, not impossibility of downstream use.

## F01 — Fleet resources are disconnected from execution (critical)

[`FleetManager`](../../../engine/litho/tetonic-app/src/fleet_api.rs) owns Mutex-protected maps for organizations, squads and agent responses. `create_agent` requires world-adapter names, registers an agent with `None` adapter and perception sender, and produces a Running response without building a runtime. `dispatch_rest` is a Rust method, not an authenticated network endpoint. Constructor and dispatcher callers found by `rg` are tests. [`Application`](../../../engine/litho/tetonic-app/src/lib.rs) has no fleet service field.

Impact: an apparently created/running agent has no corresponding managed execution; restart loses fleet records. The model also unnecessarily requires a world adapter for every agent.

Decision: retain organization/team/agent concepts; replace map authority with repositories and controller-backed activation. Return desired/observed states separately. Remove fake Running behavior, adapter-name-only bindings and in-process dispatcher once the real API exists.

## F02 — Organization quota is disconnected and misleading (high)

[`fleet.rs`](../../../engine/mantle/tetonic-orchestrator/src/fleet.rs) increments an AtomicU64 before checking its ceiling, with no window rollover in that implementation. Failed charges remain accumulated. `max_active_agents` is a field, not an admission check in the inspected creation path. `create_agent` charges an arbitrary 1,000 tokens before checking duplicate agent ID.

Decision: delete creation-token fiction. Extend existing broker admission/accounting with organization scope, reservations, settlement, explicit accounting windows and active-execution limits. Creating metadata is not inference consumption. Do not claim budget enforcement across processes until backed by a durable atomic ledger.

## F03 — Existing managed execution is valuable but concrete-agent coupled (critical)

[`identity.rs`](../../../engine/core/tetonic-domain/src/identity.rs) defines persistent identity, immutable job spec and an executor trait. [`identity_job.rs`](../../../engine/litho/tetonic-app/src/identity_job.rs) admits jobs through managed services. [`managed/execution.rs`](../../../engine/mantle/tetonic-run/src/managed/execution.rs) checks durable bindings, identities, capabilities, cancellation and execution claims, but its entry point accepts concrete `tetonic_core::Agent` and `Conversation` values.

[`service.rs`](../../../engine/mantle/tetonic-run/src/service.rs) owns command handling, snapshots, deduplication and event replay with optional storage. Per-run async locks are process-local. Existing recovery can quarantine interrupted work; it does not prove arbitrary automatic resumption. There is no basis to claim this implementation already supports multiple independent control-plane writers safely.

Decision: preserve transition/replay/claim/result-integrity contracts and tests. Generalize the executor boundary beneath them. Require durable storage in server mode. Keep memory-only stores for test/local explicitly ephemeral profiles.

## F04 — World execution bypasses common assembly and managed lifecycle (critical)

[`tetonic-server/main.rs`](../../../engine/mantle/tetonic-server/src/main.rs) defines its own configuration structs, constructs an Ollama provider, adapter, perceptive brain and `Agent` directly, serves debug/health with a hand-written TCP HTTP response, and calls `run_in_world`.

[`Agent::run_in_world`](../../../engine/core/tetonic-core/src/agent.rs) drains perception backlog to the latest entry, calls the brain, validates manifest affordances, checks adapter E-stop and calls `adapter.execute` directly. That method does not call the normal action broker. The standalone provider construction does not use [`build_compute_plane`](../../../engine/litho/tetonic-app/src/compute_plane.rs), which brokers inference in CLI/daemon paths.

Decision: migrate the world loop to a managed harness; route effects and inference through common services. Distinguish replaceable observations from durable events before retaining latest-value draining. Cancellation must interrupt/wake waiting work, not rely only on the next perception. Preserve the experiment until its replacement is live; then delete this special composition and manual HTTP implementation.

## F05 — Keeper/runner role model is a detached prototype (critical)

[`role.rs`](../../../engine/mantle/tetonic-node/src/role.rs) stores runners and agent locations in HashMaps, uses process-local Instant heartbeats and an epoch counter, and composes KeeperRegistry/RunnerClient directly in NodeLifecycle. `NodeLifecycle::init` callers found are tests. Exported role vocabulary and documentation are not an operating distributed coordination service.

Decision: replace registry authority with one fenced assignment contract integrated with existing run leases and worker delivery. Do not use restart-reset counters as cluster fencing. Separate agent worker registration from inference provider registration. Retain useful contract tests after porting them to the real service.

## F06 — The existing remote worker is inference infrastructure (high)

[`job_ingress.rs`](../../../engine/mantle/tetonic-node/src/job_ingress.rs) rejects jobs whose kind is not Infer. Node ingress, trust, TLS, scheduler, lease table and revocation modules nevertheless contain useful infrastructure. Fabric clients are consumed by [`compute_plane.rs`](../../../engine/litho/tetonic-app/src/compute_plane.rs).

Decision: retain this live infrastructure. Define whole-agent execution as a distinct worker capability and protocol contract; do not rename an inference node and advertise agent placement. Inventory handshake/version/lease semantics before deciding which transport can be reused.

## F07 — Coding definition and sessions still shape the application (high)

[`definition.rs`](../../../engine/litho/tetonic-app/src/definition.rs) declares one `CODING_IDENTITY_ID`, fixed specialist roles, role prompts and completion heuristics. [`tetonic-orchestrator/lib.rs`](../../../engine/mantle/tetonic-orchestrator/src/lib.rs) exports routing, critic, briefing, spawning and fleet types together. The application assembles coding behavior alongside platform services.

Decision: allocate distinct persistent agent identities from a reusable coding definition. Move role routing, critic loops, repository context, verify/commit policy and tool subsets into a coding harness/pack. Preserve generic child admission, privilege attenuation, budgets and result provenance. A session is conversation/UI state, not an agent's identity or sole lifecycle authority.

## F08 — Configuration and node role vocabularies overlap without wiring (high)

[`engine_config.rs`](../../../engine/core/tetonic-domain/src/engine_config.rs) declares standalone/coordinator/runner and storage modes including distributed DB and moveable volume. The experiment server parses independent local structs and explicitly rejects modes other than standalone. [`lokaid/main.rs`](../../../engine/litho/lokaid/src/main.rs) chooses coordinator/node/combined behavior through command flags.

Decision: establish one implemented server schema; reject unsupported modes, version it, and separate operator config from agent definitions. Provide a one-time import for experiment config. Delete duplicate parsers after cutover. Do not ship unimplemented modes based on enum availability.

## F09 — Operator control and observability have separate authorities (high)

[`operator_control.rs`](../../../engine/litho/tetonic-app/src/operator_control.rs) controls FleetSupervisor and builds dashboard cards from fleet records and ThoughtStreamHub. [`thought_stream.rs`](../../../engine/litho/tetonic-app/src/thought_stream.rs) uses in-memory queues/broadcast and its own lifecycle events. The experiment has a separate [`TraceStore`](../../../engine/mantle/tetonic-server/src/observability.rs); application/run events provide another path.

Decision: derive lifecycle views from durable accepted events. Turn stop/resume/steer into authorized commands with requested/accepted/effective distinctions. Retain bounded live streaming as delivery, never as state authority. Join events using organization, identity, definition, execution, attempt, action and worker IDs. Record observable model/tool data with redaction; do not imply access to hidden model reasoning.

## F10 — Runtime enforcement exists, but is not universal isolation (high)

[`RuntimeActionBroker`](../../../engine/core/tetonic-runtime/src/action_broker.rs) evaluates policy, binds approval hooks by attempt, and issues parameter-bound capabilities using an in-memory store. [`approval.rs`](../../../engine/litho/tetonic-app/src/approval.rs) combines persistence/validation with live coordination and coding-shell details. Egress is applied through participating clients, not an OS firewall.

Decision: reuse these controls, add trusted tenant/principal/assignment context and durable approval transitions, and enforce each action at the effect boundary. Custom executable harnesses require explicit process/network/credential isolation guarantees. An external process cannot be made safe by passing it a trait wrapper alone. Lost approval waiters must recover from durable records, not recreate unbounded grants.

## F11 — Legacy-named code remains on live paths (medium)

`compute_plane.rs` imports RemoteNodeProvider; [`tetonic-fabric-client/lib.rs`](../../../engine/atmos/tetonic-fabric-client/src/lib.rs) exports legacy modules. [`lokaid/main.rs`](../../../engine/litho/lokaid/src/main.rs) hosts stdio RPC plus node/combined paths. [Release CI](../../../.github/workflows/engine-ci.yml) builds lokai-cli and lokaid.

Decision: do not delete based on naming. Migrate transport callers and delivery assets together. Preserve database history readers until a supported migration/export path exists. Remove compatibility only with an explicit supported-version decision.

## F12 — Optional cognitive strategies have no identified production activation (medium)

Searches found DualProcessBrain construction only in its tests; StreamWorldAdapter and CompositeWorldAdapter usage primarily in tests/soak coverage. These are useful candidates for removal or optional examples, not foundational platform requirements.

Decision: require a named workload for maintaining them. Prefer retaining one proven external-environment adapter and moving scripted brains to test support. No API-consumer absence is proven by this search alone.

## Reproducible search examples

```powershell
rg -n 'FleetManager::new|FleetSupervisor::new|OperatorController::new|dispatch_rest\(' engine --glob '*.rs'
rg -n 'NodeLifecycle::|KeeperRegistry|RunnerClient|EngineConfig::' engine --glob '*.rs'
rg -n 'run_in_world\(|Agent::new\(|DualProcessBrain|CompositeWorldAdapter|StreamWorldAdapter' engine --glob '*.rs'
rg -n 'CODING_IDENTITY_ID|UnsupportedJobKind|job_kind !=' engine --glob '*.rs'
```

Static symbol searches locate candidates; source inspection is required to distinguish test-only constructors from production paths. This report does not equate unit tests, comments, exported types or schema entries with delivered product behavior.
