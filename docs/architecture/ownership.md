# System ownership

This map assigns behavior to existing implementation owners. It is the review
contract for incremental architecture work, based on `19a9af98` on October 8,
2026. No new service, crate or runtime is implied by an owner below. Entry points
include public Rust/local API interfaces and identified internal composition
functions; they are not necessarily remote APIs.

For each change, identify its behavioral owner, the durable records it affects,
and the boundary test that should change with it. Keep the authorization and
execution paths intact when extracting modules. The
[terminology](terminology.md) explains similarly named records.

## Change routing

| Change | Start with the existing owner | Do not substitute |
|---|---|---|
| Agent creation, editing and saved capabilities | Agent registry and revision services | A UI-only agent or mutable running definition |
| Organization/team membership and execution grants | Authorized resource services | Roles or principals supplied by a request body |
| Work shaping, assignment dependencies and agent collaboration | Work coordination and its product adapters | An independent loop in the UI, broker or legacy orchestrator |
| Start, retry, stop, suspend or reconcile execution | Managed execution | A second task supervisor or status table |
| Human questions, approval decisions or emergency stops | Authorized human controls and managed lifetime | A modal-only decision or a generic reusable approval |
| Add a tool, MCP connection, skill or harness | Capability services and registered runtime assembly | Provider-specific execution that bypasses grants |
| Select a model or inference worker | Provider adapters and compute broker | Changing which host owns the agent's tool effects |
| Budget or usage behavior | The matching budget ledger and admission path | A new counter in UI state or an unrelated broker estimate |
| Memory sharing, recall or artifact visibility | Scoped context services | Automatically pooling agents' histories |
| Map, work progress or operator inspection | Authorized read projections and web presentation | Inferring success from animation, chat text or fixtures |
| Server configuration and resource lifetime | Existing application host composition | Another independently constructed application host |

## Host composition and entry points

**Responsibility:** configure and connect the store, policy, runtime, providers,
credentials and resource services; expose the current operator/product adapters.

- **Entry points:** [CLI commands](../../engine/litho/tetonic-cli/src/main.rs),
  [`LocalWorkspace::open_with_workspace`](../../engine/litho/tetonic-app/src/local_workspace/bootstrap.rs),
  [`prepare_launch` and registered launch functions](../../engine/litho/tetonic-app/src/job_launch.rs).
- **Composition:** [`Application`](../../engine/litho/tetonic-app/src/lib.rs),
  [`services`](../../engine/litho/tetonic-app/src/services.rs),
  [`TurnBind`](../../engine/litho/tetonic-app/src/product_submit.rs), and
  [`build_compute_plane`](../../engine/litho/tetonic-app/src/compute_plane.rs).
  These hold shared dependencies; they are not additional authorities for agent
  identity, work or run state.
- **Boundary evidence:** tests in `job_launch.rs` and `compute_plane.rs`,
  [CLI operator journeys](../../engine/litho/tetonic-cli/tests/control_cli.rs),
  and local HTTP validation in
  [`local_ui.rs`](../../engine/litho/tetonic-cli/src/local_ui.rs).

**Current seam:** initialization is spread across these files, and `TurnBind`
still has an old lifecycle-oriented name. Host construction and operator config
are the next extraction boundary, not a reason to replace managed execution.
The broad [engine configuration schema](../../engine/core/tetonic-domain/src/engine_config.rs)
does not establish that every mode or storage choice is wired into startup.

## Agent identities and revisions

**Responsibility:** retain a continuing agent identity and publish exact versions
of its harness configuration. Resolve saved preferences within current host and
authorization limits when preparing execution.

- **Entry points:** [`ResourceService::register_agent`, `edit_agent`, `publish_agent_revision`](../../engine/litho/tetonic-app/src/resources/agents.rs);
  [`PreparedAgentRevision` and general harness preparation](../../engine/litho/tetonic-app/src/resources/general_harness.rs).
  The [agent product adapter](../../engine/litho/tetonic-app/src/local_workspace/agents.rs)
  supplies editor/catalog behavior.
- **Durable owner:** [`organization_agents`](../../engine/strata/tetonic-memory/src/organization_agents.rs),
  [`organization_agent_revisions`](../../engine/strata/tetonic-memory/src/organization_agent_revisions.rs),
  [`organization_agent_edits`](../../engine/strata/tetonic-memory/src/organization_agent_edits.rs)
  and [`identity_store`](../../engine/strata/tetonic-memory/src/identity_store.rs).
- **Dependencies and rule:** resource authorization → revision storage → prepared
  identity/job specification. Saved tool names and model preferences are data,
  not grants. Editing an agent must not silently rewrite a revision pinned by
  accepted work or a running attempt.
- **Boundary evidence:** [agent revision tests](../../engine/strata/tetonic-memory/src/organization_agent_edit_tests.rs),
  [product editing tests](../../engine/litho/tetonic-app/src/local_workspace/agents/editing_tests.rs),
  [agent editor UI tests](../../web/tests/agent-editing.test.tsx).

## Organizations, membership and authority

**Responsibility:** authenticate callers, evaluate current membership and exact
resource actions, and bind separately authorized execution and context access.

- **Entry points:** [`ResourceService` / `ResourceAuthority`](../../engine/litho/tetonic-app/src/resources.rs),
  [`LocalControl`](../../engine/litho/tetonic-app/src/resources/local_control.rs),
  [`execution_grants`](../../engine/litho/tetonic-app/src/resources/execution_grants.rs)
  and [`execution_authority`](../../engine/litho/tetonic-app/src/resources/execution_authority.rs).
- **Durable owner:** [`team_store`](../../engine/strata/tetonic-memory/src/team_store.rs),
  [`execution_grants`](../../engine/strata/tetonic-memory/src/execution_grants.rs),
  context-access records and delegated grants in `tetonic-memory`.
- **Dependencies and rule:** trusted credential verification → current resource
  authorization → checked store operation. A resource identifier, saved agent,
  enrollment certificate or roster membership cannot replace that chain.
  Resource authorization is not a promise to undo an already admitted effect
  if a grant is subsequently revoked.
- **Boundary evidence:** [resource authorization tests](../../engine/litho/tetonic-app/src/resources/tests.rs),
  [delegated grant tests](../../engine/litho/tetonic-app/src/resources/delegated_grants_tests.rs).

**Current seam:** the local product uses fixed owner/org/team identifiers in
[`local_workspace.rs`](../../engine/litho/tetonic-app/src/local_workspace.rs).
Explicit application scope must precede a general multi-user host. Do not
duplicate the same local-owner assumptions in new features.

## Work shaping and team coordination

**Responsibility:** accept human intent, maintain briefs and plans, select agents,
dispatch eligible assignments, collect contributions, and handle work questions
and continuation. Models may propose changes; authorized services accept them.

- **Entry points:** [shaping](../../engine/litho/tetonic-app/src/local_workspace/shaping.rs),
  [plan execution](../../engine/litho/tetonic-app/src/local_workspace/plan_execution.rs),
  [`team_work_controller`](../../engine/litho/tetonic-app/src/team_work_controller.rs),
  and [`PlanDispatch`](../../engine/litho/tetonic-app/src/resources/plan_dispatch.rs).
  [Work resource services](../../engine/litho/tetonic-app/src/resources/team_work.rs)
  own authorized mutations; the local adapter binds host settings and output.
- **Durable owner:** [`TeamGoal`, `TeamWorkItem`, `HuddleProposal` and delegation](../../engine/strata/tetonic-memory/src/team_work.rs),
  [briefs](../../engine/strata/tetonic-memory/src/work_briefs.rs),
  [`WorkTeam` versions/bindings](../../engine/strata/tetonic-memory/src/work_teams.rs),
  [`HuddleExecution` receipts](../../engine/strata/tetonic-memory/src/huddle_execution.rs)
  and [execution-derived progress](../../engine/strata/tetonic-memory/src/huddle_execution/progress.rs).
- **Dependencies and rule:** work services → registry and grants → managed
  delegation. Coordination may run independent assignments concurrently, subject
  to dependencies and capacity. It must not mint its own attempt leases, bypass
  budgets or make a conversation message authoritative proof of completion.
  A plan-start receipt preserves accepted inputs; it is not a second run journal.
- **Boundary evidence:** [parallel groups](../../engine/litho/tetonic-app/src/local_workspace/plan_group_tests.rs),
  [plan execution](../../engine/litho/tetonic-app/src/local_workspace/plan_execution_tests.rs),
  [human intervention](../../engine/litho/tetonic-app/src/local_workspace/plan_human_tests.rs),
  [provider coordination](../../engine/litho/tetonic-app/src/local_workspace/providers/tests/coordination.rs).

**Current seam:** coordination spans application modules and `local_workspace`;
it is not yet a cleanly extracted generic work service. The retained
`tetonic-orchestrator` coding/router library is not the owner of this product
path simply because of its package name.

## Managed execution and recovery

**Responsibility:** admit jobs; own live attempts and durable transitions; enforce
executor claims, deadlines and cancellation lineage; finalize outcomes; reconcile
interrupted execution and restore explicitly supported checkpoints.

- **Entry points:** [`ManagedRunService`](../../engine/mantle/tetonic-run/src/managed/service.rs)
  and [`DurableRunSupervisor`](../../engine/mantle/tetonic-run/src/service.rs).
  [`registered_executor`](../../engine/litho/tetonic-app/src/resources/registered_executor.rs)
  prepares and submits to this owner; it is not another supervisor.
- **Durable owner:** [`RunSnapshot`, `TaskRecord`, `AttemptRecord` and commands/events](../../engine/core/tetonic-domain/src/run.rs)
  persisted by [`commit_run_command`](../../engine/strata/tetonic-memory/src/run_store.rs).
  Event, projection and command idempotency are committed together, with capacity
  and usage integration at the transaction boundary.
- **Dependencies and rule:** managed execution → runtime executor + transactional
  store + authorized scope + finalization dependencies. The agent produces a
  candidate outcome; managed finalization establishes the result. Retries and
  inspection must not construct a new executor merely because work exists.
- **Boundary evidence:** [managed service tests](../../engine/mantle/tetonic-run/tests/managed_service_tests.rs),
  [execution claims](../../engine/mantle/tetonic-run/tests/execution_claim.rs),
  [durable human waits](../../engine/litho/tetonic-app/src/resources/registered_executor/human_wait_tests.rs),
  [hard process-loss regression](../../engine/litho/tetonic-app/src/local_workspace/providers/tests/process_recovery_tests.rs).

Cancellation controls managed execution and its attached descendants/resources;
it cannot reverse a completed external effect. General crash replay remains
distinct from supported human-wait restoration. See
[baseline limits](../epics/v5-reconciliation/sprints/architecture-baseline/BASE-001.md#new-proof-and-narrow-recovery-correction).

## Human intervention and approvals

**Responsibility:** persist human decisions and stops, validate their exact scope,
and connect them to work continuation and managed execution lifetime.

- **Entry points:** [authorized human controls](../../engine/litho/tetonic-app/src/resources/human_controls.rs),
  [shell approval binding](../../engine/litho/tetonic-app/src/resources/shell_approval.rs),
  and [plan questions/amendments](../../engine/litho/tetonic-app/src/local_workspace/plan_human.rs).
- **Durable owner:** [`ControlStop`, `EffectApproval` and associated records](../../engine/strata/tetonic-memory/src/human_controls.rs),
  accepted plan/question state and managed suspension/checkpoint records.
  Product adapters deliver decisions to the existing work/runtime owners.
- **Dependencies and rule:** authorize the decision → validate its target and
  revision/digest → apply through the bound continuation or effect path. A shell
  approval for one attempt must not authorize another agent's identical command.
  Persisting a stop and quiescing live execution are related but distinct steps;
  the UI must not claim all effects have stopped just because it saved a record.
- **Boundary evidence:** [human control tests](../../engine/strata/tetonic-memory/src/human_controls_tests.rs),
  [parallel approvals](../../engine/litho/tetonic-app/src/local_workspace/providers/tests/parallel_approval_tests.rs),
  [plan questions](../../engine/litho/tetonic-app/src/local_workspace/plan_human_tests.rs),
  [handoff UI](../../web/tests/human-handoff.test.tsx).

## Harness assembly and action enforcement

**Responsibility:** compose the selected provider and permitted capabilities into
an agent loop, then enforce model/tool action boundaries throughout execution.

- **Entry points:** application
  [`submit_registered_job`](../../engine/litho/tetonic-app/src/resources/registered_executor.rs),
  with internal [`prepare_registered_harness`](../../engine/litho/tetonic-app/src/resources/registered_executor/preparation.rs)
  and [`assemble_registered_harness`](../../engine/litho/tetonic-app/src/resources/registered_executor/assembly.rs);
  runtime [`EngineRuntime::assemble_agent`](../../engine/core/tetonic-runtime/src/assembly.rs),
  [`RuntimeActionBroker`](../../engine/core/tetonic-runtime/src/action_broker.rs),
  and the [`tetonic-core` agent loop](../../engine/core/tetonic-core/src/lib.rs).
- **State and dependencies:** application assembly resolves pinned definitions,
  current grants, scope, provider, tools and audit sinks. Runtime supplies action
  authorization/capabilities; the core loop consumes provider and tool interfaces.
  Managed execution supplies the attempt fence and lifetime. Loop/conversation
  state is not a second durable lifecycle owner.
- **Rule:** add harness adapters through this composition and managed boundary.
  A vendor model adapter does not supply a managed vendor-native harness.
  Switching provider must not bypass tool authorization, approved data scope,
  cancellation, audit or usage accounting.
- **Boundary evidence:** [runtime assembly tests](../../engine/core/tetonic-runtime/src/assembly_tests.rs),
  [registered execution tests](../../engine/litho/tetonic-app/src/resources/registered_executor_tests.rs),
  [provider parity](../../engine/litho/tetonic-app/src/local_workspace/providers/tests/parity.rs),
  [independent shell approvals](../../engine/litho/tetonic-app/src/local_workspace/providers/tests/parallel_approval_tests.rs).

The enforcement responsibilities remain separate even when the host composes
them together:

| Owner | Boundary it supplies |
|---|---|
| [tetonic-policy](../../engine/core/tetonic-policy/src/lib.rs) | Policy evaluation and dispatch decisions; not caller authentication |
| [RuntimeActionBroker](../../engine/core/tetonic-runtime/src/action_broker.rs) | Action authorization and capability issuance within the runtime |
| [tetonic-egress](../../engine/atmos/tetonic-egress/src/lib.rs) | Outbound destination/transport control; not permission to invoke a tool |
| [tetonic-secrets](../../engine/core/tetonic-secrets/src/lib.rs) | Secret scanning/redaction used at configured boundaries; not an execution grant |
| [tetonic-sandbox](../../engine/core/tetonic-sandbox/src/lib.rs) | Platform-specific effect/process isolation; not a replacement for scope or approval checks |

## Tools, MCP and skills

**Responsibility:** describe available capabilities, retain installations and
bindings, resolve selected capabilities, and implement effects under runtime
enforcement.

- **Entry points:** [`McpRegistry`](../../engine/litho/tetonic-app/src/mcp.rs),
  [`SkillLibrary`](../../engine/litho/tetonic-app/src/skills.rs), their `ToolHost`
  wrappers, and [`tetonic-tools`](../../engine/litho/tetonic-tools/src/lib.rs).
  The [MCP](../../engine/litho/tetonic-app/src/local_workspace/mcp.rs) and
  [skill](../../engine/litho/tetonic-app/src/local_workspace/skills.rs) product
  adapters expose workspace management; agent revisions select capabilities.
- **Durable owner:** [`workspace_mcp`](../../engine/strata/tetonic-memory/src/workspace_mcp.rs)
  and [`workspace_skills`](../../engine/strata/tetonic-memory/src/workspace_skills.rs),
  plus agent revisions and execution grants. Connection state and imported
  instructions do not independently grant authority.
- **Dependencies and rule:** selected tool hosts → runtime capabilities and
  current bindings → effect implementations. File operations use the relevant
  workspace/transaction path; process execution uses the process broker and
  sandbox path; MCP uses its governed transport. Do not claim every external
  effect can be staged, rolled back or fully isolated on every platform.
- **Boundary evidence:** [managed MCP tests](../../engine/litho/tetonic-app/src/mcp/tests/managed.rs),
  [skill execution](../../engine/litho/tetonic-app/src/local_workspace/providers/tests/skills.rs),
  [hosted shell execution](../../engine/litho/tetonic-app/src/local_workspace/providers/tests/shell.rs),
  [agent capabilities UI](../../web/tests/agent-capabilities.test.tsx).

## Inference, compute placement and transport

**Responsibility:** admit and route model requests, account for compute, resolve
providers/targets, enforce outbound policy and secret scanning, and communicate
with configured inference workers.

- **Entry points:** application [`build_compute_plane`](../../engine/litho/tetonic-app/src/compute_plane.rs),
  [`BrokerInferenceProvider`](../../engine/mantle/tetonic-broker/src/adapters/inference.rs),
  [provider adapters](../../engine/atmos/tetonic-inference/src/lib.rs),
  and [provider selection/discovery](../../engine/litho/tetonic-app/src/local_workspace/providers.rs).
- **Records and dependencies:** broker reservations and scheduler decisions use
  `tetonic-memory` when composed with durable storage. Provider transport uses
  `tetonic-egress`; remote inference also uses fabric protocol/client, enrollment
  and node ingress. Worker delivery receipts live in
  [`WorkerStore`](../../engine/strata/tetonic-memory/src/worker_store.rs), separate
  from the agent run journal.
- **Rule:** a remote model request does not move the agent's tools or workspace
  onto the model server. The [current worker](../../engine/mantle/tetonic-node/src/job_ingress.rs)
  accepts inference jobs, not general agent execution. Workstation placement
  records are not by themselves a remote executor or distributed lease service.
- **Boundary evidence:** `compute_plane.rs` tests, broker/egress/provider crate
  tests, [provider tool journeys](../../engine/litho/tetonic-app/src/local_workspace/providers/tests/tools.rs),
  [workstation placement records](../../engine/strata/tetonic-memory/src/workstation_placement_tests.rs).

## Budgets, capacity and usage

**Responsibility:** authorize bounded work, reserve delegated effort, meter model
usage, and enforce concurrent execution/compute capacity. These are distinct
resources with different accounting boundaries, not one interchangeable number.

- **Entry points:** authorized
  [work budget services](../../engine/litho/tetonic-app/src/resources/work_budgets.rs),
  [`WorkUsageProvider`](../../engine/litho/tetonic-app/src/resources/work_usage.rs),
  [execution limits](../../engine/litho/tetonic-app/src/resources/execution_limits.rs),
  and broker admission.
- **Durable owner:** [work reservations](../../engine/strata/tetonic-memory/src/work_budgets.rs),
  [reported token usage and holds](../../engine/strata/tetonic-memory/src/work_usage.rs),
  [root capacity](../../engine/strata/tetonic-memory/src/run_capacity.rs),
  [child capacity](../../engine/strata/tetonic-memory/src/child_capacity.rs),
  compute reservation and scheduler-decision records.
- **Dependencies and rule:** admission reserves against the relevant ledger;
  execution records attributable usage; completion/stop settles supported
  reservations. Unknown provider usage must remain unknown/held, not become zero.
  Token accounting is not exact currency billing or a guarantee that the final
  in-flight request cannot exceed a reported-token allowance.
- **Boundary evidence:** [work budget tests](../../engine/strata/tetonic-memory/src/work_budgets_tests.rs),
  [usage tests](../../engine/strata/tetonic-memory/src/work_usage_tests.rs),
  [resumption accounting](../../engine/strata/tetonic-memory/src/work_usage_resume_tests.rs),
  [usage UI](../../web/tests/usage.test.tsx).

## Scoped context and artifacts

**Responsibility:** authorize reads, recall and explicit publication of information;
bind audit history and artifact access to the correct context; assemble context
only from permitted sources.

- **Entry points:** [`ContextService`](../../engine/litho/tetonic-app/src/resources/contexts.rs),
  [scoped compiler binding](../../engine/litho/tetonic-app/src/resources/context_compiler.rs),
  [artifact access](../../engine/litho/tetonic-app/src/resources/context_artifacts.rs),
  and [`tetonic-context`](../../engine/strata/tetonic-context/src/lib.rs).
- **Durable owner:** context identity, access, history, recall, publication and
  artifact bindings in `tetonic-memory`; payload storage in `tetonic-artifact`.
  A content hash locates a payload; it is not authorization to read it.
- **Dependencies and rule:** verified caller + current context access → selected
  history/recall/artifacts. Team participation is not implicit publication of
  private history. The current registered general harness binds explicit scoped
  recall when granted; its assembly does not attach a global briefing or a
  default coding context compiler.
- **Boundary evidence:** [context pipeline tests](../../engine/litho/tetonic-app/src/resources/context_pipeline_tests.rs),
  access/history tests in `tetonic-memory`, and
  [registered execution tests](../../engine/litho/tetonic-app/src/resources/registered_executor_tests.rs).

## Persistence, projections and presentation

**Responsibility:** `tetonic-memory` owns SQL, migrations, atomic commits and
store-level integrity checks. Application inspection builds authorized product
views. The web client organizes and presents those views; telemetry reports
activity without becoming authoritative state.

- **Entry points:** [`Store` / `SharedStore`](../../engine/strata/tetonic-memory/src/lib.rs),
  [`run_store`](../../engine/strata/tetonic-memory/src/run_store.rs),
  [`LocalWorkspace::snapshot` and task projection](../../engine/litho/tetonic-app/src/local_workspace.rs),
  [`run_inspection`](../../engine/litho/tetonic-app/src/resources/run_inspection.rs),
  and [`LocalEngineProvider`](../../web/src/context/LocalEngineContext.tsx).
- **Dependencies and rule:** domain commands and authorized resource operations
  → checked store transactions → projections → web adapters/components. Keep
  domain invariants already enforced in storage when moving application code.
  Do not place product orchestration in storage or SQL in the UI/transport.
- **Presentation:** [`LocalEngine`](../../web/src/lib/localEngine.ts) handles the
  HTTP boundary; [`engineAdapters`](../../web/src/lib/engineAdapters.ts) maps
  engine records; [`TeamWorkspace`](../../web/src/components/team-work/TeamWorkspace.tsx)
  renders the product. Polling caches can be stale; local animation, optimistic
  edits and sample traces must not manufacture execution success or authority.
- **Boundary evidence:** [durability tests](../../engine/strata/tetonic-memory/src/durability_tests.rs),
  [migration tests](../../engine/strata/tetonic-memory/src/migration_tests.rs),
  [engine client](../../web/tests/engine-client.test.tsx),
  [engine adapters](../../web/tests/engine-adapters.test.tsx),
  [workspace integration](../../web/tests/team-work-engine.test.tsx).

## Dependency rules and current exceptions

The rules above describe ownership to preserve in changes. They do not assert
that the repository already has an ideal physical layout or complete mechanical
enforcement. Current production Cargo dependencies include
`tetonic-run → tetonic-runtime`, `tetonic-runtime → tetonic-memory`,
`tetonic-core → tetonic-inference`, and
`tetonic-broker → tetonic-run`. The application depends on most engine packages.
These are real edges, not a strictly descending Earth-layer stack.

Use domain contracts for shared vocabulary, application services for use cases,
managed execution for lifetime, and adapters for transport/presentation. An
extraction should reduce misplaced responsibility without cloning its state or
authority. Review the callers and tests before changing a dependency.

[`tetonic-arch-gate`](../../engine/tooling/tetonic-arch-gate/src/lib.rs) implements
specific static checks and exceptions. Some checks still mention retired entry
points. A passing gate does not prove all ownership boundaries above, universal
sandboxing, or end-to-end product behavior. Tightening enforcement and retiring
stale rules is subsequent work; the ownership map must not overstate it.

## Package inventory

All 28 engine workspace packages, as reported by Cargo metadata. Paths remain
unchanged in this step. A retained library is not automatically a supported
standalone product or a feature exposed by the local UI.

| Folder | Package | Current architectural role |
|---|---|---|
| litho | [tetonic-cli](../../engine/litho/tetonic-cli) | `tetonic` commands and local HTTP adapter |
| litho | [tetonic-app](../../engine/litho/tetonic-app) | Host composition, authorized services, work coordination and projections |
| litho | [tetonic-tools](../../engine/litho/tetonic-tools) | Concrete local tool effects and tool-host integration |
| litho | [tetonic-lsp](../../engine/litho/tetonic-lsp) | Retained language-server client integration |
| mantle | [tetonic-run](../../engine/mantle/tetonic-run) | Managed execution, durable supervision and finalization |
| mantle | [tetonic-orchestrator](../../engine/mantle/tetonic-orchestrator) | Retained coding/router strategies; not the current team-work controller |
| mantle | [tetonic-broker](../../engine/mantle/tetonic-broker) | Compute admission, scheduling and inference/process broker adapters |
| mantle | [tetonic-capacity](../../engine/mantle/tetonic-capacity) | Hardware/model capacity and topology |
| mantle | [tetonic-node](../../engine/mantle/tetonic-node) | Worker service machinery and inference ingress |
| mantle | [tetonic-enroll](../../engine/mantle/tetonic-enroll) | Node enrollment, identities and trust transport |
| core | [tetonic-domain](../../engine/core/tetonic-domain) | Shared types, IDs, commands and interface contracts |
| core | [tetonic-core](../../engine/core/tetonic-core) | Agent model/action loop and conversation/checkpoint mechanics |
| core | [tetonic-runtime](../../engine/core/tetonic-runtime) | Agent assembly, action broker, capability and policy integration |
| core | [tetonic-policy](../../engine/core/tetonic-policy) | Policy evaluation and dispatch guard |
| core | [tetonic-sandbox](../../engine/core/tetonic-sandbox) | Platform-specific process isolation and effect executors |
| core | [tetonic-transaction](../../engine/core/tetonic-transaction) | Staged file operations and transaction support |
| core | [tetonic-secrets](../../engine/core/tetonic-secrets) | Secret detection, redaction and scanner contracts |
| core | [tetonic-telemetry](../../engine/core/tetonic-telemetry) | Structured execution observations and telemetry sinks |
| strata | [tetonic-memory](../../engine/strata/tetonic-memory) | Durable SQLite records, migrations and transactional integrity |
| strata | [tetonic-artifact](../../engine/strata/tetonic-artifact) | Artifact payload storage |
| strata | [tetonic-context](../../engine/strata/tetonic-context) | Context compilation and retrieval interfaces |
| strata | [tetonic-index](../../engine/strata/tetonic-index) | Retained code indexing and search machinery |
| atmos | [tetonic-inference](../../engine/atmos/tetonic-inference) | Provider adapters, model transport and pooling |
| atmos | [tetonic-egress](../../engine/atmos/tetonic-egress) | Outbound transport authorization and guard |
| atmos | [tetonic-fabric-client](../../engine/atmos/tetonic-fabric-client) | Client transport for enrolled compute targets |
| atmos | [tetonic-fabric-protocol](../../engine/atmos/tetonic-fabric-protocol) | Fabric messages and delivery contracts |
| tooling | [tetonic-arch-gate](../../engine/tooling/tetonic-arch-gate) | Engineering gate and static architecture/quality checks |
| tooling | [tetonic-bench](../../engine/tooling/tetonic-bench) | Retained benchmarking support |

Regenerate the dependency inventory with
`cargo metadata --manifest-path engine/Cargo.toml --no-deps --format-version 1`.
Filter by `workspace_members`; exclude dev dependencies when discussing production
edges. Additions or responsibility changes should update this map in the same
change as the implementation.
