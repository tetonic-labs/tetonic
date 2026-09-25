# Findings

Paths below are relative to the audited repository; line anchors refer to baseline `eae7db2`. Priorities and evidence labels are defined in [the audit overview](README.md).

## F01 — Multiple execution authorities have not converged

**P1 · Integration gap · Owner: runtime / run / server composition**

`tetonic-server` constructs an `Agent`, `SingleModelBrain`, Ollama provider, and WebSocket adapter directly. It calls `run_in_world`. It does not compose the durable supervisor or compute broker. The coding application, meanwhile, builds a broker-backed compute plane and uses managed attempts. Fleet supervision introduces a third lifecycle registry.

Evidence: [server composition](../../../../engine/mantle/tetonic-server/src/main.rs:118), [managed execution](../../../../engine/mantle/tetonic-run/src/managed/execution.rs:8), [compute plane](../../../../engine/litho/tetonic-app/src/compute_plane.rs:1), [fleet state](../../../../engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs:91).

Consequence: correctness improvements on one path do not automatically apply to another. Budgeting, recovery, policy, observability, and cancellation can disagree. This includes recently added code; it should be treated as integration scaffolding.

Change: one production composition service, with finite job and standing-agent activation modes sharing authorization, inference admission, durable effects, and telemetry. Keep domain-specific context providers separate. Prove this with a non-coding adapter and the Village through the same host.

## F02 — Fleet creation reports execution that has not started

**P1 · Code-confirmed / integration gap · Owner: fleet service**

`FleetManager::create_agent` records the agent and registers `None` for both its adapter and perception sender, then returns `Running`. It launches no agent task. Constructor searches found FleetManager composition in tests, not in the inspected production binaries. The API's “launches” documentation and its tests overstate the behavior.

Evidence: [create_agent](../../../../engine/litho/tetonic-app/src/fleet_api.rs:251), especially registration at line 297 and status at line 309.

Change: distinguish `desired=running`, `observed=pending`, `assigned`, `starting`, and `running`. Report running only after an activation with a valid ownership proof reaches its readiness barrier. A failed launch must yield a failed/pending condition, not a healthy inventory entry. Make repeated creation idempotent.

## F03 — Keeper leases are not durable distributed ownership

**P0 before failover · Reproduced · Owner: coordination**

`KeeperRegistry` is an in-memory map. Registration uses `lease-<local counter>` and starts the lease epoch at 1. A fresh keeper can issue the identical proof for the same runner; the old proof is accepted by the new registry. Heartbeat validation checks lease ID and epoch but not `proof.holder`. Every heartbeat increments the epoch, conflating renewal with ownership generation. No production network registration/agent reassignment composition was found through the inspected role call sites.

Evidence: [registry and registration](../../../../engine/mantle/tetonic-node/src/role.rs:146), [heartbeat](../../../../engine/mantle/tetonic-node/src/role.rs:195). Probes: `keeper_restart_reissues_identical_proof`, `keeper_heartbeat_does_not_validate_holder`.

Change: persist agent ownership separately from runner liveness. Use authenticated runner identities, a durable coordinator term, and an agent activation generation that increases on ownership transfer. Renewal extends expiry without changing the generation. Mutation boundaries must reject stale generations. A heartbeat timeout alone is not proof the previous runner stopped.

## F04 — Fleet stop is not a monotonic hierarchical interlock

**P0 before fleet safety claims · Reproduced · Owner: lifecycle / action boundary**

An individual agent can be resumed while the fleet-wide stop remains active. New registrations are initialized as running even under fleet stop. The supervisor calls adapter resume without checking the global stop, and ignores adapter stop/resume failures. The probes reproduce contradictory supervisor state; a real adapter resume follows the same branch.

Evidence: [registration](../../../../engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs:123), [fleet stop](../../../../engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs:210), [agent resume](../../../../engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs:286).

Change: compute effective stop from independently versioned organization, fleet, agent, and adapter reasons. Clearing a child reason must not clear a parent stop. Record desired stop, acknowledged stop, and outstanding effects separately. Every effect admission validates the effective stop generation. Do not advertise instantaneous fleet-wide quiescence across a network partition.

## F05 — Operational boundaries are partly descriptive and not on the execution path

**P0 before external automation · Reproduced plus code-confirmed · Owner: policy / runtime**

`IntentCharter::evaluate_action` enforces only forbidden action and namespaced allowlist cases; PathFilter and ResourceCap fall through. An unnamespaced action passes NamespaceAllowlist. Searches found no production caller of this charter evaluator. `run_in_world` validates the manifest and E-stop, then dispatches without routing the world action through `RuntimeActionBroker`.

Evidence: [charter evaluation](../../../../engine/core/tetonic-domain/src/charter.rs:84), [world dispatch](../../../../engine/core/tetonic-core/src/agent.rs:400), [existing action broker](../../../../engine/core/tetonic-runtime/src/action_broker.rs:67). Probe: `namespace_allowlist_accepts_unnamespaced_action`.

Change: separate advisory intentions from executable policy. Normalize adapter actions into an authorization request with identity, resource, effect class, and policy version. Reject unsupported hard-boundary types at configuration/admission. Retain world validation as an additional authoritative check. Local Village loopback constraints limit current exposure; this is not a claim of a remotely exploitable deployment.

## F06 — Checkpoint integrity does not protect the complete state

**P0 before trusting recovery · Reproduced · Owner: state storage**

The checkpoint header promises cryptographic integrity, but checksum computation is a 64-bit FNV-style hash. It omits `charter_snapshot` and `created_at`, and concatenates variable fields without boundaries. Changing the charter passes verification; digest/buffer pairs `ab`+`c` and `a`+`bc` produce the same checksum by construction.

Evidence: [checkpoint contract](../../../../engine/core/tetonic-domain/src/checkpoint.rs:35), [checksum](../../../../engine/core/tetonic-domain/src/checkpoint.rs:83). Two checkpoint integrity probes reproduce this.

Change: versioned canonical serialization covering every protected field; a standard cryptographic content digest; an authenticated signature/MAC only when authenticity is required, with explicit trust/key management. Checksums alone do not establish authenticity. Include agent identity, activation generation, event cursor, state schema, and policy/config versions.

## F07 — Checkpoint discovery can restore another agent and choose older state

**P0 before recovery/migration · Reproduced · Owner: state storage**

Loading `alice` matches filename prefix `agent-alice-`, including a checkpoint for `alice-child`. The loaded payload is not compared to the requested agent. Sanitization also maps distinct identifiers to the same filename. Latest selection sorts strings, so checkpoint `9` sorts after `10`. Pruning uses the same broad prefix and ordering. Saving syncs the file then renames, but does not establish a durable indexed head or a cross-writer ownership fence.

Evidence: [save/load](../../../../engine/core/tetonic-core/src/checkpoint.rs:36), [prune](../../../../engine/core/tetonic-core/src/checkpoint.rs:170), [sanitization](../../../../engine/core/tetonic-core/src/checkpoint.rs:209). Two lookup/order probes reproduce the principal defects; pruning impact is code-derived and was not exercised.

Change: collision-free encoded identities or ID-keyed storage; verify exact payload identity; store monotonic numeric revision; atomically update a checkpoint head under ownership/CAS; use unique temporary files. Define platform-specific crash durability, corruption/quarantine reporting, and safe retention. Do not claim millisecond failover from a serialization round-trip test.

## F08 — Event acknowledgement is not coordinated with durable agent state

**P1 · Code-confirmed · Owner: activation runtime / inbox**

The live server stores experience/intentions in RAM and acknowledges events after a parsed decision. This accurately means decision inclusion, but not durable incorporation. A crash after acknowledgement loses that knowledge. Neither the acknowledgement nor an intended effect is committed with agent state.

Evidence: [experience storage](../../../../engine/mantle/tetonic-server/src/experience.rs:6), [acknowledgement](../../../../engine/mantle/tetonic-server/src/perceptive_brain.rs:243).

Change: transactional inbox deduplication, agent-state revision, decision record, and outbox entries; acknowledge after commit. Keep the distinct concepts of delivery, durable incorporation, inferred understanding, and actual compliance. Acknowledgement must never mean the agent was forced to obey a suggestion.

## F09 — Worker ingress recovery silently treats storage failures as empty state

**P0 for durable delivery guarantees · Code-confirmed · Owner: fabric worker storage**

`JobIngressManager::load` uses `.ok()`, `unwrap_or_default`, and per-record `filter_map` on database/decoding failures. It can discard the deduplication history without surfacing degraded recovery. Later write failures are handled, but that does not repair silently lost historical knowledge if storage becomes available again.

Evidence: [ingress loading](../../../../engine/mantle/tetonic-node/src/job_ingress.rs:19).

Change: fallible initialization, explicit empty-first-boot versus unreadable/corrupt-store states, quarantine and operator-visible readiness failure. Test corruption, unavailable storage, mixed valid/invalid rows, and restart before accepting work. Current worker ingress accepts Infer only; do not overstate the present risk as duplicated arbitrary remote mutations.

## F10 — Continuous cognition is a serial loop; dual-process is a router

**P1 · Code-confirmed · Owner: actor runtime**

`run_in_world` awaits brain perception and action execution serially. Cancellation is checked before inference; this path does not race cancellation against in-flight inference or recheck WorkScope immediately before dispatch. The adapter provides additional E-stop/epoch fencing, which is valuable but different from structured task cancellation. `DualProcessBrain` selects one brain, rather than running reflex and deliberation concurrently; its deliberative cost adds the reflex's previous cost even when that reflex did not run on this perception.

Evidence: [world loop](../../../../engine/core/tetonic-core/src/agent.rs:371), [dual routing](../../../../engine/core/tetonic-runtime/src/brain.rs:300), [cancellation contract](../../../../engine/core/tetonic-domain/src/work_scope.rs:26).

Change: one activation actor owns state; inference/effects run as cancellable child work and return revision-tagged proposals. Control events remain responsive during inference. Stale proposals are rejected before dispatch. First implement correct single-brain scheduling; keep dual-process an optional strategy rather than a prerequisite or architectural claim.

## F11 — Snapshot and event channels have incompatible loss semantics

**P1 · Code-confirmed / integration risk · Owner: adapter protocol**

WebSocket perceptions use a bounded queue and `try_send`; when full, new snapshots are discarded. The core drains to the latest retained item. This bounds backlog but does not guarantee the freshest snapshot. CompositeWorldAdapter multiplexes whole perceptions and overwrites event source/kind; the core's global drain can discard another adapter's events/state. World-specific retained delivery helps Village, but is not a generic adapter contract.

Evidence: [WebSocket ingestion](../../../../engine/core/tetonic-runtime/src/websocket_adapter.rs:45), [core drain](../../../../engine/core/tetonic-core/src/agent.rs:390), [composite forwarding](../../../../engine/core/tetonic-runtime/src/composite_adapter.rs:64).

Change: separate latest-value snapshot slots per source from durable event inboxes and high-priority control messages. Preserve source identity, provenance, sequence, and acknowledgement scope in envelope fields rather than mutating semantic event kinds. Define byte limits, overflow outcomes, lag metrics, and fair source scheduling.

## F12 — Manifest schemas are not actually fully validated

**P1 · Reproduced · Owner: domain / adapter contracts**

WorldManifest exposes JSON schemas, but validation checks required-property presence only. Wrong types pass. An empty affordance list permits all actions. The server manufactures generic instant affordances from action-name configuration instead of negotiating actual capabilities, and its inference request sends no structured tools.

Evidence: [manifest validator](../../../../engine/core/tetonic-domain/src/world_adapter.rs:133), [server manifest](../../../../engine/mantle/tetonic-server/src/main.rs:135), [decision request](../../../../engine/mantle/tetonic-server/src/perceptive_brain.rs:222). Probe: `manifest_accepts_wrong_parameter_type`.

Change: supported schema dialect/version, full validation, typed result envelopes and durative action handles. Treat unrestricted manifests as an explicit development mode. Validate configured actions against adapter-advertised capabilities and negotiate compatibility before readiness.

## F13 — Ambiguous effects lack a general reconciliation model

**P1 · Code-confirmed · Owner: action execution / durable effects**

The WebSocket adapter waits for a matching receipt and fences old connections. However, a timeout or disconnection after a world mutation returns an error without a generic status-query/reconciliation path. IDs are generated at adapter submission; there is no durable logical-effect identity shared with state. The server rejects malformed/truncated responses, then ordinarily attempts a future decision rather than applying a bounded classified recovery policy.

Evidence: [action execution](../../../../engine/core/tetonic-runtime/src/websocket_adapter.rs:178), [decision failure handling](../../../../engine/mantle/tetonic-server/src/perceptive_brain.rs:226).

Change: stable effect ID before transmission, durable outbox, adapter idempotency contract, `unknown` outcome, query/cancel handles, and explicit reconciliation. Retry only when semantics permit. Exactly-once external effects cannot be promised for arbitrary adapters; expose weaker guarantees honestly.

## F14 — Fleet quotas do not model real admission or tenancy

**P1 · Code-confirmed · Owner: admission / identity**

Organization token accounting is an ever-increasing atomic counter despite an hourly field name. There is no window reset in this implementation. Agent creation charges a fixed 1,000 tokens before duplicate-agent validation; actual inference usage is not wired to this fleet accounting. `max_active_agents` is declared but not enforced on creation. The newer fleet maps use globally keyed IDs without a complete authenticated tenant context.

Evidence: [quota](../../../../engine/mantle/tetonic-orchestrator/src/fleet.rs:34), [accounting](../../../../engine/mantle/tetonic-orchestrator/src/fleet.rs:86), [creation ordering](../../../../engine/litho/tetonic-app/src/fleet_api.rs:268).

Change: reuse broker reservation/accounting, model tenant/organization/agent/activation budgets, reserve before work and settle actual usage afterward. Make create retries cost-neutral. Add window semantics and durable counters. Scope authorization and reads as well as writes; hierarchy labels alone do not establish isolation.

## F15 — Fleet concurrency contains lock-order and blocked-delivery hazards

**P1 · Code-confirmed risk, no deadlock run · Owner: fleet service**

`get_squad` acquires squads then agent_records; `create_agent` holds agent_records then acquires squads. `get_org` and `create_squad` also have opposing org/squad ordering. `inject_steering` holds synchronous registry read guards across awaited channel sends; one full inbox can delay all later recipients and retain registry locks. Operator control also holds the shared supervisor mutex across awaited operations.

Evidence: [fleet lock paths](../../../../engine/litho/tetonic-app/src/fleet_api.rs:151), [squad read](../../../../engine/litho/tetonic-app/src/fleet_api.rs:226), [creation](../../../../engine/litho/tetonic-app/src/fleet_api.rs:283), [steering](../../../../engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs:156).

Change: a single command/reducer owner or transactional repository per aggregate; take immutable recipient handles before awaiting; bounded independent delivery with durable retry. Add contention tests with controlled barriers. Do not hold global ownership/control locks while waiting on a potentially stalled runner.

## F16 — Memory and context have three separate accounting/selection paths

**P1 · Code-confirmed · Owner: context / agent state**

The standing server has observation facts, recent intents, experience events, and byte/3 prompt budgeting. The core has a tokenizer abstraction and char/3.7 heuristic. The context pipeline uses byte/4 evidence estimation. ExactTokenizer can fall back to a heuristic while `estimated()` still reports false. Existing workspace context retrieval is product-specific and cannot simply be passed every world database.

Evidence: [experience recall](../../../../engine/mantle/tetonic-server/src/experience.rs:34), [server budgeting](../../../../engine/mantle/tetonic-server/src/context_budget.rs:13), [tokenizer](../../../../engine/core/tetonic-core/src/tokenizer.rs:36), [context budgeting](../../../../engine/strata/tetonic-context/src/pipeline/stage6_budget.rs:5).

Change: common request-level accounting with explicit exact/estimated status and provider/template version; neutral evidence/provenance interfaces with world-local retrieval adapters. Retain identity-scoped memory and distinguish observations, claims, suggestions, intentions, and outcomes. Add retention/forgetting and contradiction rules before simply enlarging context.

## F17 — Canonical configuration exists alongside an incompatible server configuration

**P1 · Integration gap · Owner: host composition**

The domain config advertises node modes, storage modes, and a 1-to-1,000 scale invariant. The executable reads a separate private Config type and supports standalone/loopback/Ollama only. Searches of EngineConfig construction found tests, not the server composition. NodeRole duplicates NodeMode through conversions.

Evidence: [domain config](../../../../engine/core/tetonic-domain/src/engine_config.rs:1), [actual config](../../../../engine/mantle/tetonic-server/src/main.rs:23), [node role](../../../../engine/mantle/tetonic-node/src/role.rs:22).

Change: one versioned config schema with validated capabilities. Parse syntax separately from operational readiness. Reject unwired modes. Store the effective config/version in activation records. Define hot-reload versus restart-required changes. Make standalone a deployment composition of the same services, not a separate implementation.

## F18 — Observability has competing event models and no shared standing-agent authority

**P1 · Integration gap · Owner: telemetry / operator API**

The repo has durable run events, telemetry TraceContext/storage gates, application events, ThoughtStreamHub, and a separate server TraceStore. The server's `perception-N` trace IDs are only locally meaningful; its raw ring buffer and health projection are not a durable lifecycle journal. ThoughtStreamHub is a broadcast API with no tenant-filtered subscription argument, so isolation must be enforced by a future host rather than assumed.

Evidence: [server trace](../../../../engine/mantle/tetonic-server/src/observability.rs:1), [thought hub](../../../../engine/litho/tetonic-app/src/thought_stream.rs:88), [telemetry facilities](../../../../engine/core/tetonic-telemetry/src/lib.rs:1).

Change: shared envelope with tenant, agent, activation, decision, attempt, effect, adapter, causation, and schema IDs. Separate durable audit from sampled metrics and opt-in raw model output. Show accepted, running, completed, rejected, unknown, and recovered distinctly. Preserve raw provider output where authorized; do not relabel it as verified thought, belief, or world truth.

## F19 — Gates and CI do not establish the advertised engine guarantees

**P1 · Code-confirmed / executed · Owner: engineering quality**

The static gate passes, but `check_production_runtime` scans only CLI and daemon directories; it never checks the new server. Raw-read detection matches fully qualified paths and misses the `fs::read_to_string` alias in core checkpoint code. CI runs package quality, then a selected test list excluding run, broker, runtime, node, orchestrator, memory, server, and fabric protocol unit suites. Cross-platform build and release target CLI/daemon, not tetonic-server. Compiling all targets under Clippy does not execute those tests.

Evidence: [runtime gate](../../../../engine/tooling/tetonic-arch-gate/src/lib.rs:175), [read gate](../../../../engine/tooling/tetonic-arch-gate/src/freeze.rs:198), [CI](../../../../.github/workflows/engine-ci.yml), [release](../../../../.github/workflows/release.yml). Formatting failed in 32 files. Full workspace tests were attempted but blocked by build errors; see overview.

Change: derive binary/crate coverage from Cargo metadata, add negative/mutation tests for gates, execute the distributed-critical packages, and make the actual engine binary a release target. Add deterministic fault tests and required native OS tests. Keep readiness tied to exercised integration paths, not ticket completion or successful compilation.

## F20 — Product composition and domain boundaries still carry coding assumptions

**P1 · Code-confirmed · Owner: architecture / host extraction**

The app crate imports 25 internal packages and owns compute-plane construction alongside coding behavior. EngineRuntime assembly requires workspace-version and post-edit hooks. Domain includes code-index/LSP/workspace contracts and filesystem-aware configuration loading; core checkpoint persistence introduces raw storage I/O into the kernel. These are practical seams, but the claimed generic engine is not yet cleanly composed independently of them.

Evidence: [inventory](inventory.json), [assembly inputs](../../../../engine/core/tetonic-runtime/src/assembly.rs:35), [domain exports](../../../../engine/core/tetonic-domain/src/lib.rs:1), [compute host](../../../../engine/litho/tetonic-app/src/compute_plane.rs:29).

Change: extract neutral host composition and neutral activation/effect contracts; retain coding as a capability pack. Move checkpoint I/O to storage implementations. Make workspace hooks optional through a typed capability binding rather than requiring meaningless no-ops from non-coding consumers. Avoid a giant rename/move commit or a universal mega-trait.

## F21 — Local state ownership is not a distributed storage contract

**P1 before multiple coordinators · Design risk · Owner: state / coordination**

DurableRunSupervisor serializes commands with in-process per-run locks and commits to local SQLite via SharedStore. This is useful single-authority machinery, not proof that independent coordinator processes can safely share state. A configurable `DistributedDb` enum and a moveable volume do not provide election, fencing, or transactional compare-and-swap.

Evidence: [supervisor](../../../../engine/mantle/tetonic-run/src/service.rs:51), [local store contract](../../../../engine/strata/tetonic-memory/src/lib.rs:1), [run commit](../../../../engine/strata/tetonic-memory/src/run_store.rs:18).

Change: define transaction and consistency semantics before picking an HA backend. Start with one durable coordinator; require conditional updates against expected revision/ownership term. Later select either an established transactional coordination store or a replicated metadata service. Do not put high-volume memories/raw tokens into the coordination log. Do not share a SQLite file as a shortcut to active-active coordination.

## F22 — Distributed compute and distributed agents are different capabilities

**P1 · Code-confirmed / integration gap · Owner: scheduler / worker runtime**

Fabric has real negotiated job/result machinery and node ingress, but the reviewed worker explicitly accepts Infer only. Agent actor registration and migration live in separate role/checkpoint libraries. Compute placement therefore does not demonstrate movable standing-agent execution.

Evidence: [worker job restriction](../../../../engine/mantle/tetonic-node/src/job_ingress.rs:33), [fabric contracts](../../../../engine/atmos/tetonic-fabric-protocol/src/lib.rs:1), [node role model](../../../../engine/mantle/tetonic-node/src/role.rs:366).

Change: separate agent placement (state locality, ownership, adapter access) from inference placement (model, capacity, trust, deadline). Advertise only supported execution capabilities. Add version negotiation for actor checkpoint/event/action protocols, not just compute packets. A remote model call is not an agent migration.

## F23 — Resource retention and async load behavior lack a unified service budget

**P2, P1 before scale · Code-confirmed risk · Owner: service operations**

SharedWorkpad appends without a size limit; supervisor per-run locks remain in a growing map; ThoughtStreamHub bounds events per agent but not aggregate agent count/bytes; the server spawns per-connection health handlers without a concurrency budget. Some other subsystems have strong limits, so this is inconsistent coverage rather than absence of all backpressure.

Evidence: [workpad](../../../../engine/mantle/tetonic-orchestrator/src/fleet.rs:125), [run locks](../../../../engine/mantle/tetonic-run/src/service.rs:97), [thought retention](../../../../engine/litho/tetonic-app/src/thought_stream.rs:88), [HTTP tasks](../../../../engine/mantle/tetonic-server/src/main.rs:182).

Change: explicit object-count and byte budgets, lifecycle cleanup, fair admission, bounded connections/tasks, queue age limits, and backpressure metrics. Replace manual HTTP parsing with a maintained server stack when this becomes a product API. Measure memory per idle/active agent and control-message latency under overload before assigning node-count targets.

## F24 — Deployment and assurance claims exceed demonstrated behavior

**P2, P0 before hostile workloads · Code-confirmed documentation gap / design risk · Owner: operations / security / docs**

Architecture docs describe concurrent cognition, cluster failover, and portable state as realized behavior; several modules implement only local types/tests. Checkpoint comments claim cryptographic guarantees and millisecond restoration without supporting implementation/measurement. The sandbox documentation correctly admits platform confinement gaps, including lack of a complete filesystem OS filter and limits of process-group isolation. Those constraints must govern distributed workload placement.

Evidence: [live architecture](../../../../docs/architecture/live-agent-architecture.md), [checkpoint claims](../../../../engine/core/tetonic-domain/src/checkpoint.rs:1), [sandbox limits](../../../../engine/core/tetonic-sandbox/README.md), [release composition](../../../../.github/workflows/release.yml).

Change: classify every capability as specified, implemented, wired, exercised, or operationally qualified. Separate trusted-owner workers from mutually untrusted tenants. Require enforced isolation for the latter; do not relabel broker warnings as containment. Add startup/readiness checks, graceful drain, backup/restore drills, supported-version matrices, and rollback procedures. Update stale package paths and examples as part of each migration slice.
