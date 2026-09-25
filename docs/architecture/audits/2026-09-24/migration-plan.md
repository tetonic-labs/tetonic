# Migration plan

This is an ordered architecture backlog, not a claim that the changes have been implemented. It supersedes feature-first sequencing where feature work would deepen the duplicate standing-agent path. Keep existing sprint tickets, but link them to these dependencies and acceptance gates.

## M0 — Restore trustworthy contracts and verification

Addresses F03–F07, F09, F12, F19, F24. First work should be small, reviewable fixes with tests, not a repository reorganization.

1. Fix checkpoint identity lookup and pruning; canonicalize complete payload hashing; add schema/revision fields. Decide compatibility for existing unversioned checkpoints explicitly. Do not silently load a legacy file as a new trusted checkpoint.
2. Fix fleet stop inheritance across registration, individual resume, squad resume, and adapter errors. Expose requested versus acknowledged stop.
3. Reject unsupported hard-boundary types and invalid action-schema payloads. Separate prompt guidance from enforceable permission.
4. Make worker ingress initialization fallible and readiness-dependent on dedup state recovery.
5. Remove lease-proof reuse and holder-validation defects; label the role registry non-production until persistence and transport wiring are complete.
6. Fix formatting and resolve the local workspace build failure. Make CI execute critical distributed packages and test the real server binary. Expand production assembly checks to all binaries; test imported aliases and new crate paths.
7. Replace capability claims such as “persistent,” “running,” “cryptographic,” and measured timing claims where the implementation does not yet establish them.

**Gate:** the ten audit probes have corresponding desired-behavior regressions; relevant package tests and quality gates pass; the server is in CI; unsupported capabilities fail clearly. Preserve the original characterization probes as historical evidence, not active success criteria after fixes.

## M1 — Define ownership once and converge standalone composition

Addresses F01, F02, F17, F20, F22.

1. Record an ADR defining AgentIdentity, AgentDefinition, AgentActivation, Decision, finite Task/Attempt, Effect, and RunnerSession. Define who can write each record.
2. Extract neutral service composition from the coding app. Route the standing server through shared compute admission, policy/action authorization, telemetry, and run supervision.
3. Replace private server config with the canonical supported subset of a versioned config. Leave distributed modes rejected until their readiness gate exists.
4. Make fleet commands persist desired state; a reconciler launches activations and updates observed state. Do not mark running from registry insertion.
5. Add a deterministic non-coding adapter and run it and Village through the same production composition used for an existing coding workflow.

**Gate:** two local agents have distinct identities, state, budgets, and traces; create/start/stop/restart commands are idempotent; failed launch never reports running; all model calls pass through the broker; coding regression tests remain green. This is local multi-agent proof, not distributed failover proof.

## M2 — Commit agent continuity and reconcile effects

Addresses F06–F08, F11–F13, F16, F21.

1. Introduce typed state repository transactions for activation state, inbox, checkpoint head, and outbox. Implement locally in SQLite first.
2. Persist accepted events and source cursors; deduplicate before cognition. Commit state changes, consumed input IDs, and effect intents atomically.
3. Assign stable effect IDs before transmission. Define adapter support for deduplication, status lookup, cancellation, and stale-generation rejection.
4. Add ambiguous outcome handling and recovery. A lost receipt after application must not become an automatic new action.
5. Persist bounded provenance-aware memory and optional intentions. Preserve objective changes as versioned inputs without deleting unrelated learned evidence.
6. Implement checkpoint restoration with exact identity/schema checks, safe fallback, explicit corrupt-state handling, and bounded retention.

**Gate:** crash injection at each transaction/send/receipt boundary yields either a known committed outcome or an explicit reconcilable unknown. Restart preserves memory and pending effects; replay does not duplicate an idempotent world mutation. Event acknowledgement never precedes durable state incorporation.

## M3 — Responsive scheduling, stop semantics, and bounded load

Addresses F04, F10, F11, F14–F16, F23.

1. Introduce one activation state owner and separate control, durable event, snapshot, and background queues.
2. Run inference as tracked child work; propagate cancellation; reject stale proposals using state/policy/ownership revisions. Drain outstanding work during shutdown.
3. Remove lock-order cycles and guards held across recipient delivery. A stalled agent must not block fleet stop/control.
4. Unify request token accounting; make fallback estimates explicit. Select relevant memory under a complete request budget.
5. Integrate tenant/agent reservations and usage settlement. Enforce active-agent limits and real quota windows.
6. Add retention and byte limits for registries, workpads, histories, connections, tasks, and durable queue backlog.

**Gate:** stop/control remains responsive during a deliberately stalled inference call, full inbox, and slow adapter. No new unauthorized effect is admitted after the stop fence; quiescence is reported separately. Overload stays within configured bounds. Independent agents receive fair processing. Record baseline latency/memory numbers and then set explicit service targets.

## M4 — One coordinator, two real runners, proven failover

Addresses F03, F09, F21, F22 plus all prior dependencies.

1. Implement authenticated runner registration with runner-session identity; persist agent ownership generations and coordinator incarnation/term.
2. Add an agent-placement reconciler distinct from inference routing. Place using adapter/state locality, isolation, CPU/RAM capacity, and policy.
3. Integrate fencing at effect admission and adapter/dispatcher boundary. Persist ownership changes conditionally.
4. Transfer checkpoint references and event/effect cursors. Reconcile previous in-flight effects before declaring the new activation healthy.
5. Introduce capability and protocol negotiation for mixed runner/adapter versions.

**Gate:** launch coordinator and two runner processes against a deterministic world; kill a runner during inference, before send, after world application, and before receipt persistence. Partition the old runner while it still computes. The stale runner cannot admit effects after fencing; the new runner resumes the correct identity and state. Coordinator restart cannot reuse old ownership authority.

Do not enable automatic reassignment merely because a heartbeat timer expired. Fence and reconcile according to adapter semantics.

## M5 — Operational product and scale qualification

Addresses F18, F19, F23, F24.

1. Provide client/server administration, durable conditions, effective config, API request idempotency, graceful drain, version compatibility, and rollback rules.
2. Correlate agent/activation/decision/attempt/effect traces. Offer opt-in raw provider output with access control and retention, alongside durable audit and bounded metrics.
3. Add backup/restore drills, migration compatibility tests, corrupt-state recovery, disk-full handling, expired credentials, certificate rotation/revocation, and constrained-network tests.
4. Benchmark with deterministic and real-model workloads. Separate inference bottlenecks from state-store and actor-scheduling bottlenecks.
5. Choose coordinator HA backend based on established transactional semantics and measured requirements. Introduce leader/term testing before HA rollout.
6. Qualify trusted-owner and untrusted-tenant deployments separately. Require enforced isolation and deny unsupported placement for hostile workloads.

**Gate:** documented support matrix and measured service objectives, sustained multi-agent soak, successful restore/rollback drills, and an explicit account of guarantees under network partition and unavailable storage. Release artifacts actually contain the intended engine binaries.

## Required failure matrix

| Scenario | Expected invariant |
|---|---|
| Duplicate event before/after restart | One durable incorporation per scoped ID; repeat delivery is safe. |
| Crash after event commit, before acknowledgement | Event may repeat; incorporation/effect intent do not duplicate. |
| Crash after decision commit, before send | Outbox safely resumes the same effect ID. |
| World applies action; receipt is lost | Unknown outcome reconciles; no blind new logical effect. |
| Lease expires while inference runs | Proposal can be recorded for diagnosis but cannot authorize stale effects. |
| Runner partitions then reconnects after reassignment | Old activation generation is rejected. |
| Coordinator restarts | No reused authority; durable desired/observed state reconciles. |
| Fleet stop then individual resume / registration | Parent stop remains effective. |
| One inbox full | Other agents/control messages continue; overflow is explicit. |
| Two adapters emit concurrently | Source events/provenance survive; one snapshot does not erase another source. |
| Wrong actor checkpoint / corrupt latest / corrupt all | Exact identity checked; fallback or recovery condition is explicit. |
| Storage unavailable during recovery | No empty-history fallback and no readiness claim. |
| Repeated create/retry | No duplicate agent and no duplicate budget charge. |
| Same ID under two tenants | Data/authorization remains isolated. |
| Invalid schema / unsupported boundary | Rejected before effect dispatch. |
| Old/new binary or adapter versions mix | Supported negotiation or clear rejection before execution. |
| Provider malformed/truncated/timeout/over-budget | Bounded classified recovery, no invented action or unbounded retry. |
| Long idle period with an incoming message | Low routine compute use and timely event-driven wakeup. |
| Shutdown with running external work | Admission closed; actual quiescence or explicit unresolved effect recorded. |

## Preserve emergent behavior while improving correctness

The engine should guarantee reliable senses, memory boundaries, actuation, ownership, and transparency. It should not guarantee that an agent chooses a particular story outcome. In Village, notices, gifts, conversations, and planted thoughts are inputs; agents may ignore, interpret, discuss, or act on them.

Keep evaluations split:

- Infrastructure: deterministic assertions about delivery, state, isolation, fencing, budgets, and effects.
- Grounding: rates of schema-valid actions, invented references, repeated rejected actions, stale-state errors, and correct receipt interpretation.
- Continuity: retrieval of relevant previous experience and preservation of self-authored intentions across interruptions/restarts.
- Emergence: longitudinal observations of interaction patterns, with reproducible environment/model/config metadata and no scripted pass condition requiring a particular society or task outcome.

## Commit and migration discipline

Each slice should contain the contract, implementation on one real production path, desired-behavior tests, telemetry, and updated documentation. Avoid declaring an enum, type, or mock test to be a completed feature. Keep compatibility facades temporary with named deletion gates. Stage storage migrations before turning on readers/writers that require them, and retain an explicit rollback path where data format changes permit it.

Do not push a large cleanup of all renamed packages, replace SQLite prematurely, invent consensus, or build a grand universal plugin layer ahead of a working local-to-two-runner vertical slice. The highest-value unit of progress is a proven invariant through the real execution path.
