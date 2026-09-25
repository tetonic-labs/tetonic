"""Render manually traced narratives and mechanically generated evidence catalogs.
[[repository path::literal source anchor]] resolves a verified one-based line.
This validates locations, not the truth of prose. No production files are written.
"""
from pathlib import Path
import json, re, csv, collections, subprocess
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[2]
evidence=[]
def cite(match):
    path,needle=match.group(1).split('::',1)
    concrete={
        'engine/mantle/tetonic-server/src/perceptive_brain.rs':'impl Brain for PerceptiveBrain',
        'engine/mantle/tetonic-node/src/role.rs':'impl KeeperRegistry',
        'engine/mantle/tetonic-node/src/job_ingress.rs':'impl JobIngressManager',
        'engine/mantle/tetonic-node/src/lease_table.rs':'impl LeaseTable',
        'engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs':'impl FleetSupervisor',
        'engine/litho/tetonic-app/src/operator_control.rs':'impl OperatorController',
        'engine/litho/tetonic-app/src/capacity_service.rs':'impl CapacityService for DefaultCapacityService',
    }
    if needle=='impl' and path in concrete: needle=concrete[path]
    lines=(ROOT/path).read_text(encoding='utf-8-sig').splitlines()
    found=[i for i,line in enumerate(lines,1) if needle in line]
    if not found: raise ValueError((path,needle))
    n=found[0]
    evidence.append({'file':path,'line':n,'anchor':needle,'document':current})
    return f'[{Path(path).name} — `{needle}`](../../../{path}#L{n})'

docs={}
docs['README.md']=r'''# Tetonic Engine: as-built architecture

**Source snapshot:** `0f722054c691ee90f57391fe8e9fafbe9a99a108`, branch `feature/domain-pack-decoupling`, repository `C:/Users/Mark Miller/Desktop/repos/lokai`. Analysis date: 2026-09-25. Tracked implementation files were clean at capture. Pre-existing untracked `docs/design/` and the sprint-6 Village integration directory are outside this snapshot and were not used as evidence. This package is a documentation-only addition.

## Read this first

Tetonic is a Rust workspace with **32 packages and several distinct execution paths**. The CLI and editor daemon assemble a coding-oriented application around sessions, agent turns, a durable run supervisor, policy-controlled tools, context retrieval, and inference that can reach enrolled remote workers. A separate `tetonic-server` executable runs one configured agent against a WebSocket world. That executable uses the shared agent/brain/adapter contracts but does **not** construct the application's durable run, compute-broker, session, or fleet machinery. Library implementations for organizations, squads, fleet supervision, keeper registration, and agent checkpoints exist; their presence does not establish a deployed distributed standing-agent service. These are implementation boundaries, not a proposed architecture. Evidence: [[engine/litho/tetonic-app/src/lib.rs::pub struct Application]], [[engine/litho/tetonic-app/src/compute_plane.rs::pub struct ComputePlane]], [[engine/mantle/tetonic-server/src/main.rs::async fn main]], [[engine/litho/tetonic-app/src/fleet_api.rs::pub struct FleetManager]].

The most important ownership distinction is between **durable job execution**, **live conversational execution**, and **world interaction**. A run projection is not a live agent; a session's conversation is not a durable run journal; a world receipt is not a transaction in Tetonic's SQLite database. Each has its own failure and recovery boundary. [[engine/mantle/tetonic-run/src/service.rs::pub struct DurableRunSupervisor]], [[engine/litho/tetonic-app/src/session_live.rs::pub struct LiveSession]], [[engine/core/tetonic-runtime/src/websocket_adapter.rs::struct Request]].

## Organizations, squads, and agents — level 0

Tetonic's fleet model organizes agents as **Organization → Squad → Agent**. An **organization** groups squads and holds a token budget, an active-agent limit field and a token-consumption counter. A **squad** belongs to an organization and groups agent IDs around an `IntentCharter`: shared strategic intent and operational boundaries. Each squad also owns a **SharedWorkpad**, where callers can post attributed bulletins for peers to read. These are implemented management objects, with in-memory state. [[engine/mantle/tetonic-orchestrator/src/fleet.rs::pub struct Organization]], [[engine/mantle/tetonic-orchestrator/src/fleet.rs::pub struct Squad]], [[engine/mantle/tetonic-orchestrator/src/fleet.rs::pub struct SharedWorkpad]].

```mermaid
flowchart TB
  Manager[FleetManager: creation and lookup API] -->|creates and indexes| Org[Organization]
  Org -->|owns quota fields and usage counter| Budget[Organization budget state]
  Org -->|registers squads by ID| Squad[Squad]
  Squad -->|owns| Charter[Shared IntentCharter]
  Squad -->|owns| Pad[SharedWorkpad: attributed bulletins]
  Squad -->|stores member IDs| Members[Agents belonging to the squad]
  Manager -->|registers organization and agent records| Supervisor[FleetSupervisor]
  Supervisor -->|tracks by agent ID| Managed[ManagedAgent: status and heartbeat]
  Managed -->|optional squad ID reference| Squad
  Managed -.->|steering delivery only when channel attached| Channel[Agent perception channel]
  Managed -->|optional control handle| Adapter[WorldAdapter]
```

Scope: the implemented fleet management model, independent of deployment. Solid arrows show local ownership, references or API calls as labeled; the dashed arrow is asynchronous in-process channel delivery. All depicted management state is volatile; this diagram includes no database or network listener. Squad membership stores agent IDs, while the supervisor separately holds managed-agent records. `ManagedAgent.squad_id` is optional at the lower-level API; the FleetManager creation path assigns a squad. Evidence: [[engine/litho/tetonic-app/src/fleet_api.rs::pub struct FleetManager]], [[engine/litho/tetonic-app/src/fleet_api.rs::pub async fn create_agent]], [[engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs::pub struct ManagedAgent]], [[engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs::pub async fn inject_steering]].

**Management and execution have distinct responsibilities.** FleetManager creates and looks up organizations, squads and agent records. FleetSupervisor tracks registered agents, exposes heartbeat/status queries, applies squad steering and invokes stop/resume on attached adapters. Squad steering adjusts the shared charter and attempts delivery to members with attached perception channels. The workpad is a shared data structure; it is not automatically inserted into every member's model context. [[engine/litho/tetonic-app/src/fleet_api.rs::impl FleetManager]], [[engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs::pub async fn inject_steering]], [[engine/mantle/tetonic-orchestrator/src/fleet.rs::impl SharedWorkpad]].

**Current integration:** the FleetManager agent-creation path registers an agent with no adapter or perception channel and returns a Running status without starting an agent loop. The CLI/daemon Application and standalone world server do not currently instantiate this hierarchy in their inspected composition roots. Thus organizations and squads are part of the implemented system model, while the execution paths below operate without that fleet integration. Budget enforcement is also partial: agent creation charges a fixed 1000 tokens; the inspected code has no hourly counter reset or active-agent-limit check on that path. See [the fleet lifecycle detail](organizations-and-squads.md) for creation, steering, quotas and API behavior. [[engine/litho/tetonic-app/src/fleet_api.rs::pub async fn create_agent]], [[engine/mantle/tetonic-orchestrator/src/fleet.rs::pub fn record_tokens]], [[engine/litho/tetonic-app/src/lib.rs::pub struct Application]], [[engine/mantle/tetonic-server/src/main.rs::async fn main]].

## Navigation

[Organizations and squads](organizations-and-squads.md) explains the management hierarchy introduced above.

1. [Entrypoints and composition](entrypoints.md): every Cargo binary, startup branches, scripts, shutdown.
2. [Execution paths](execution.md): finite agent turns, local/remote inference, tools, world decisions and receipts.
3. [State and persistence](state.md): ownership, transitions, leases, replay, restart and memory.
   [Supporting subsystem details](subsystems.md): context, tools, indexing, artifacts, transactions, capacity, security and telemetry.
4. [Interfaces and operations](operations.md): protocols, configuration, security, queues, cancellation and observability.
5. [Disconnected implementations and limits](limits.md): code that exists without corresponding production wiring; unresolved guarantees.
6. [Package and module atlas](atlas.md): all packages, direct manifest edges, source modules and declarations.
7. [Coverage and verification](verification.md): scope, methodology, checks and unresolved work.
8. [Evidence index](evidence.md), [file ledger](coverage.csv), [storage declarations](storage-tables.json), [machine-readable source atlas](source-atlas.json).

## System context — level 0

```mermaid
flowchart TB
  User[CLI user] -->|in-process CLI commands| CLI[lokai process]
  Editor[Editor client] -.->|async framed JSON-RPC over stdio| D[lokaid coordinator process]
  CLI -->|constructs| App[Application and coding runtime]
  D -->|constructs on initialize| App
  App -->|awaited provider calls| CP[Compute broker and provider routing]
  CP -->|provider HTTP or fabric destination check| EG[EgressGuard: coordinator instance]
  EG -.->|authorized HTTP inference| O[Local Ollama process]
  EG -->|authorized destination| FT[Fabric client: pinned TLS transport]
  FT -.->|authenticated fabric requests| W[lokaid worker process]
  W -->|worker Ollama provider| WG[EgressGuard: worker instance]
  WG -.->|authorized HTTP inference| WO[Worker Ollama process]
  App -->|read / serialized write| DB[(Coordinator SQLite stores)]
  App -->|authorized effects| FS[Workspace and sandboxed children]
  S[tetonic-server process] -->|constructs| B[PerceptiveBrain and Agent world loop]
  B -->|Ollama provider| SG[EgressGuard: standalone server instance]
  SG -.->|authorized loopback HTTP inference| O
  B -->|await adapter calls| WA[World WebSocket adapter]
  WA -.->|direct WebSocket after startup loopback URL check| World[External world authority]
  S -->|owns in RAM| RAM[Experience and trace buffers]
```

Solid arrows describe local calls, construction, or ownership as labeled; dashed arrows describe asynchronous process transports, not a delivery guarantee. Cylinder denotes durable storage; the RAM node is volatile. The shared Application node represents a common composition pattern, **not** a singleton shared across CLI and daemon processes. The world, its physics, and its rendering client are external to this repository's authority. Evidence: [[engine/litho/lokai-cli/src/main.rs::async fn main]], [[engine/litho/lokaid/src/main.rs::async fn serve]], [[engine/litho/tetonic-app/src/node_worker.rs::pub async fn run_node_serve]], [[engine/mantle/tetonic-server/src/main.rs::let brain]], [[engine/core/tetonic-runtime/src/websocket_adapter.rs::pub fn connect]].

**EgressGuard is an active network authorization boundary.** Application assembly injects it into inference/fabric clients; the worker and standalone world server construct their own instances. The ordinary destination path resolves addresses and permits the configured loopback inference port or an explicit matching IP/port rule, otherwise returning a denial and recording that decision. Ollama HTTP requests use guarded transport methods. The fabric client checks the destination with the guard before opening its own pinned TLS connection. These are local guard objects, not a separate proxy server. Evidence: [[engine/litho/tetonic-app/src/compute_plane.rs::pub async fn build_compute_plane]], [[engine/atmos/tetonic-egress/src/lib.rs::async fn authorize]], [[engine/atmos/tetonic-inference/src/lib.rs::.post_ndjson_stream]], [[engine/atmos/tetonic-fabric-client/src/client.rs::async fn open_fabric_tls]], [[engine/mantle/tetonic-node/src/fabric.rs::pub fn default_ollama]], [[engine/mantle/tetonic-server/src/main.rs::EgressGuard::loopback_inference]].

The guard answers whether a network destination is permitted; action policy, secret scanning and TLS identity checks supply different controls. It is not a process-wide firewall. In particular, the world WebSocket adapter calls `connect_async` directly: the standalone server applies a loopback URL check at startup, but that connection does not pass through EgressGuard. The diagram deliberately keeps that edge separate. See [security and operations](operations.md) for the surrounding controls. [[engine/mantle/tetonic-server/src/main.rs::world_url.scheme()]], [[engine/core/tetonic-runtime/src/websocket_adapter.rs::connect_async(&url)]].

## Implementation layers — level 1

| Workspace area | Implementation responsibility | Important distinction |
|---|---|---|
| `core` | Domain contracts; finite/world agent loops; policy, secrets, telemetry; runtime assembly; sandbox and workspace transactions | Shared contracts do not force every executable through the same composition root. |
| `strata` | SQLite stores, content-addressed artifacts, code index and context compilation | Durable stores and volatile prompt memory are separate. |
| `mantle` | Compute brokerage, durable runs, orchestration, capacity, enrollment, node ingress, standalone world server | Worker inference is wired; keeper/fleet standing-agent distribution is not equivalently wired. |
| `atmos` | Inference and egress, fabric client/protocol, editor RPC | Network protocols and local stdio are different trust boundaries. |
| `litho` | Application assembly, coding tools, LSP, CLI and daemon | Much of the operational product remains coding-workspace oriented. |
| `tooling` | Architecture checks, evaluations and benchmarks | These executables verify or measure selected behavior; they are not services. |

This table groups actual package locations; [the atlas](atlas.md) is the complete manifest-derived dependency view. Directory layering alone is not proof of an enforced runtime boundary.

## Accuracy boundary

Every one of the **959 baseline tracked files** has an explicit ledger disposition and hash. All applicable text artifacts were scanned in full for declarations and control/storage markers. Selected implementation paths were then read and traced substantively. **A lexical scan is not semantic review of every statement.** This package does not claim complete formal verification, a resolved whole-program call graph, cross-platform sandbox certification, or a live distributed/Village end-to-end test. Remaining uncertainties are recorded rather than filled in from design documents. See [verification](verification.md).
'''

docs['entrypoints.md']=r'''# Entrypoints and composition

[Overview](README.md) · [Execution](execution.md) · [Atlas](atlas.md)

## Complete Cargo binary inventory

Cargo metadata declares eight binary targets. Two are test helpers; they must not be mistaken for deployable engine services.

| Binary | Startup and principal branch | Lifetime / exit |
|---|---|---|
| `lokai` (`lokai-cli`) | Parse arguments; early help/estate/offline commands; otherwise bootstrap Application, choose TUI or one-shot turn | Close session after execution; record network activity and consolidation; return result. |
| `lokaid` | Schema output, supervisor parent, pure worker enrollment/serve, or coordinator stdio; `--combined` adds worker serving | Coordinator drains sessions for at most five seconds, stops combined accept loop, aborts writer. Worker listener has its own lifetime. |
| `tetonic-server` | Read required TOML `--config`; validate standalone/loopback bounds; construct direct Ollama brain, WebSocket adapter, trace and health listener | Select world loop against Ctrl+C; runtime exit drops tasks. No Application session close/recovery path. |
| `tetonic-eval` | Clap Run / Compare / Integrity; LocalSet; evaluation orchestrator and corpus selection | Write/report results; failed pass-floor or evaluation can return error/nonzero. |
| `tetonic-arch-gate` | Default/Arch, Quality, or Verify tier | Print findings; nonzero on failures. Verify can launch formatting, compilation, lint and tests. |
| `tetonic-bench` | Generate synthetic repository/index/vector inputs; run timed operations | Print measurements; clean temporary corpus. Does not benchmark real model intelligence. |
| `tetonic-sandbox-adv` | Dispatch selected adversarial scenario | Some scenarios deliberately stall, exhaust resources or attempt forbidden effects; only run under the harness. |
| `lsp-mock-server` | Read framed stdin JSON; initialize/definition/diagnostic responses; optional stall/crash modes | EOF/framing failure or requested crash/stall branch exits. Test support, not a real language server. |

Source entry anchors: [[engine/litho/lokai-cli/src/main.rs::async fn main]], [[engine/litho/lokaid/src/main.rs::fn main]], [[engine/mantle/tetonic-server/src/main.rs::async fn main]], [[engine/tooling/tetonic-eval/src/main.rs::fn main]], [[engine/tooling/tetonic-arch-gate/src/main.rs::fn main]], [[engine/tooling/tetonic-bench/src/main.rs::fn main]], [[engine/core/tetonic-sandbox/bins/adversarial_runner.rs::fn main]], [[engine/litho/tetonic-lsp/tests/support/mock_server.rs::fn main]]. Exact Cargo target declarations are in [packages.json](packages.json).

## Coordinator startup — level 2

```mermaid
flowchart TD
  M[lokaid main] -->|flag dispatch| Choice{Mode}
  Choice -->|print schema| Schema[Print schema and return]
  Choice -->|supervise| Parent[Re-exec child with restart backoff]
  Choice -->|node without combined| Worker[Enroll or serve worker]
  Choice -->|default or combined| RT[Tokio runtime: four workers plus LocalSet]
  RT -->|open| Audit[(Audit store)]
  RT -->|construct| Queue[Redacted bounded outbound queue]
  Queue -.->|wake delivery| Writer[Single stdout writer task]
  RT -->|optional spawn_local| Fabric[Combined fabric listener]
  RT -->|construct| Daemon[Daemon: initially no services]
  Daemon -.->|read framed stdin| Handle[Authenticate and dispatch]
  Handle -->|initialize awaits bootstrap| App[Application plus EngineServices]
  Handle -->|EOF / writer exit / shutdown| Drain[Cancel sessions and bounded drain]
  Drain -->|abort| Writer
```

Solid arrows are local synchronous operations or awaited calls as labeled; dashed arrows indicate asynchronous queue/pipe activity. Audit is persistent; Daemon and queue are in process. The daemon being alive does not mean initialization or agent execution has occurred. Evidence: [[engine/litho/lokaid/src/main.rs::async fn serve]], [[engine/litho/lokaid/src/daemon.rs::pub fn new]], [[engine/litho/lokaid/src/daemon.rs::pub async fn handle]], [[engine/litho/lokaid/src/daemon.rs::pub async fn shutdown]], [[engine/litho/tetonic-app/src/daemon_bootstrap.rs::impl Application]].

The daemon authenticates `initialize` and gates other methods until authenticated unless the explicit auth-disable configuration applies. Unknown methods produce MethodNotFound; malformed JSON produces a ParseError with null request id and processing continues. Framing failure/EOF ends the input loop. Responses and notifications share one stdout writer, so there are no competing direct JSON frame writes. `chat/send` admits a turn and streams later work; its immediate RPC response is not turn completion. [[engine/litho/lokaid/src/daemon.rs::methods::CHAT_SEND]], [[engine/litho/lokaid/src/main.rs::serde_json::from_slice]], [[engine/atmos/tetonic-rpc/src/outbound.rs::pub struct OutboundQueue]].

`--supervise` is process restart machinery: it removes the supervisor flag for the child, inherits stdio, caps abnormal restarts at eight in sixty seconds, and backs off from 200 ms to five seconds. It does not preserve a suspended inference call or shell process. [[engine/litho/lokaid/src/supervise.rs::pub fn run]].

## CLI composition

CLI argument handling selects early command branches before normal agent bootstrap. These include estate management, time travel/checkpoints, code indexing/querying, project-memory operations and session history. Normal execution constructs Application through `bootstrap_cli`, builds session/approval/TUI delivery, and runs either interactive chat or one-shot execution inside a LocalSet. The same code closes the session with success/error after the selected UI path. Shell allowance requires the extra explicit acknowledgement flag. [[engine/litho/lokai-cli/src/main.rs::let interactive]], [[engine/litho/lokai-cli/src/main.rs::bootstrap_cli]], [[engine/litho/lokai-cli/src/main.rs::app.close_session]], [[engine/litho/tetonic-app/src/cli_bootstrap.rs::impl Application]].

Application assembles initialization, live sessions, runs, run manager, supervisor, policies, approvals, estate/capacity services and turn bindings. `TurnBind` carries the provider/broker/tokenizer/runtime/store/workspace dependencies into turns. Installing the compute plane attaches supervisor bridges to remote providers and the broker. This is the connection that makes application inference participate in managed execution; the standalone server does not call it. [[engine/litho/tetonic-app/src/lib.rs::pub struct Application]], [[engine/litho/tetonic-app/src/product_submit.rs::pub struct TurnBind]], [[engine/litho/tetonic-app/src/product_submit.rs::pub fn install_compute_plane]].

## Worker composition and enrollment

`lokaid --node --enroll` generates enrollment/audit keys, opens the worker database, loads or creates TLS identity, writes a private expiring code file, validates the chosen enrollment transport/bind policy, and waits for enrollment completion. Success persists a coordinator pin; expired/failed handshakes return errors. The code-file guard removes the temporary code at scope exit. Serving is a separate subsequent invocation. [[engine/litho/tetonic-app/src/node_worker.rs::pub async fn run_node_enroll]].

`lokaid --node` opens `worker.db`, refuses to serve without coordinator pins, chooses bind/port/Ollama settings, obtains a capacity status, and enters `FabricServer::run`. A degraded/missing capacity profile is reported at startup; the actual request path has additional admission rules. Combined mode launches that listener as a local task and returns a shared stop flag to daemon shutdown. [[engine/litho/tetonic-app/src/node_worker.rs::pub async fn run_node_serve]], [[engine/litho/tetonic-app/src/node_worker.rs::pub fn spawn_combined_fabric]].

## Standalone world-server composition — level 2

```mermaid
flowchart LR
  C[TOML config] -->|parse and validate| Main[main]
  Main -->|construct| P[OllamaProvider plus loopback EgressGuard]
  Main -->|spawn transport task| A[WebSocketWorldAdapter]
  Main -->|construct| B[PerceptiveBrain wrapping SingleModelBrain]
  P -->|provider dependency| B
  Main -->|construct| Agent[Agent with EmptyToolHost]
  B -->|Brain implementation| Agent
  Agent -->|await run_in_world| A
  A -.->|WebSocket| World[External world]
  B -->|observer events| Trace[Volatile TraceStore]
  A -->|observer events| Trace
  Main -.->|spawn HTTP listener and per-connection tasks| Health[Health / debug trace endpoint]
  Health -->|read| Trace
```

Solid arrows show local construction/calls; dashed arrows show task creation or network traffic as labeled. No node in this view persists the agent's learned state. World authority is outside the process. Evidence: [[engine/mantle/tetonic-server/src/main.rs::async fn main]], [[engine/core/tetonic-core/src/agent.rs::pub async fn run_in_world]], [[engine/mantle/tetonic-server/src/observability.rs::pub struct TraceStore]].

## Scripts, build and CI entry surfaces

The [script entrypoint index](script-entrypoints.json) and [ledger](coverage.csv) include scripts and executable fixtures. They are not silently omitted. Scripts fall into these separately callable families:

| Family | Inputs → operations → outputs; failure/lifetime boundary |
|---|---|
| `scripts/install.sh`, `install.ps1` | Platform/release inputs → fetch/install packaged binaries → local installed files. External release availability and platform tools remain environmental dependencies. |
| `scripts/release.py` and release workflow | Version/flags → repository/tag checks and optional gate → release/tag/build workflow. Publishing was not executed during analysis. |
| `engine/scripts/gen_ts_protocol.py`, `gen_fabric_protocol_doc.py` | Schema/source inputs → generated protocol artifacts. Generated declarations describe shapes, not daemon dispatch reachability. |
| `run_required_tests.py`, `test_required_tests.py` | Package/test filters → subprocess cargo tests and result-count checks; zero executed tests is treated as failure by the required-test runner. |
| `smoke_lokaid.py` | Spawn daemon, send framed requests, inspect results; a controlled client/harness, not another server composition. |
| `verify_layout_consumer.py` | Construct external consumer/build inputs and invoke build tooling; environment-sensitive compatibility probe. |
| `engine/bench/*.py` | Corpus/model/runtime parameters → benchmark-specific requests or generated workloads → timing/results. Test files verify selected harness behavior. |
| `engine/corpus/scripts/*.py` | Setup/reset/scaffold/integrity operations on evaluation corpus. Some mutate fixture workspaces; not run here. |
| `tetonic-tools/fix_cmd.py`, `fix_tests.py` | Maintenance scripts modifying source/test text. Not a runtime tool API and not executed here. |
| Test fixture executables | Inputs to sandbox/eval/index tests; their execution is harness-owned, not automatically started by the product. |

This is a role and entry-surface classification, not a claim that every script branch has been executed or semantically verified. Remaining script-level dynamic coverage is recorded in verification. Source locations and discovered entry lines are linked from the generated atlas; all script text participates in the lexical inventory.

CI's gate runs `verify package`; its test job names eleven packages rather than the entire workspace. Its cross-platform release-build matrix targets `lokai-cli` and `lokaid`. Therefore that workflow alone does not establish a tested `tetonic-server` world integration or passing tests for every broker/run/fabric crate. [[.github/workflows/engine-ci.yml::jobs:]], [[engine/tooling/tetonic-arch-gate/src/main.rs::fn main]].
'''

docs['execution.md']=r'''# Execution paths and operational traces

[Overview](README.md) · [State](state.md) · [Operations](operations.md)

## Finite application turn — level 3 sequence

```mermaid
sequenceDiagram
  participant Client as CLI or RPC handler
  participant App as Application / live session
  participant Runs as Run service / durable supervisor
  participant DB as Optional SQLite store
  participant Agent as Local attempt executor / Agent
  participant Model as Broker-backed provider
  participant Tools as Authorized tool runtime
  Client->>App: Submit chat turn or spawn command (local call)
  App->>App: Admission and acquire conversation ownership
  App->>Runs: Build identity/job binding and prepare managed execution
  Runs->>DB: Persist command, event and projection when configured
  Runs->>Agent: Claim execution and invoke attempt
  loop Until final answer, cancellation, error or step limit
    Agent->>Agent: Compile context and request metadata
    Agent->>Model: Await chat request
    Model-->>Agent: Stream/deliver response
    opt Model requests tools
      Agent->>Tools: Validate and authorize proposed calls
      Tools-->>Agent: Results or denial/error
      Agent->>Agent: Append conversation state
    end
  end
  Agent-->>Runs: Attempt outcome
  Runs->>DB: Record completion/failure through supervisor
  App->>App: Return conversation, end turn and deliver result
  App-->>Client: Asynchronous events / final completion
```

Scope: one application turn; all arrows are local calls/returns except downstream provider/tool transports, expanded below. Solid arrows are calls (many awaited); dashed arrows are replies/events. SQLite is the persistent boundary, live session the conversation owner. This is the principal path with optional orchestration branches collapsed, not a statement that every turn runs a planner/critic. Evidence: [[engine/litho/tetonic-app/src/product_submit.rs::pub fn submit_chat_turn]], [[engine/litho/tetonic-app/src/turn_execution.rs::run_orchestrated_turn]], [[engine/mantle/tetonic-run/src/managed/execution.rs::ClaimExecution]], [[engine/core/tetonic-core/src/agent.rs::pub struct Agent]].

Admission and ownership matter before a model call: turns can be rejected while draining, capacity work is active, another turn owns the session, or capacity gates do not allow execution. The owned-turn guard restores conversation ownership and publishes a dropped-turn outcome on unwinding/drop. Agent identity, definition/input digests, capability bindings and artifact bindings are checked against the managed task; the neutral `AgentAttemptExecutor` contract is not itself the coding implementation. [[engine/litho/tetonic-app/src/product_submit.rs::struct OwnedTurn]], [[engine/core/tetonic-domain/src/identity.rs::pub struct AgentJobSpec]], [[engine/mantle/tetonic-run/src/managed/execution.rs::ClaimExecution]].

The finite Agent loop checks cancellation and limits, compacts context when configured, constructs chat messages/tool schemas, supplies fabric metadata, then handles text/tool responses. The coding pack supplies workspace-oriented identity, tools and context. Routing, critic and revision paths are selected by application configuration; generic `Brain` support coexists with the provider-driven finite loop. It would be inaccurate to describe the current product as only a domain-neutral standing-agent loop. [[engine/core/tetonic-core/src/agent.rs::pub struct Agent]], [[engine/litho/tetonic-app/src/coding_pack.rs::impl]], [[engine/litho/tetonic-app/src/turn_execution.rs::run_orchestrated_turn]].

## Inference and remote compute — level 3

```mermaid
flowchart TD
  Request[Application ChatRequest with execution identity] -->|await chat| Wrapper[BrokerInferenceProvider]
  Wrapper -->|submit / admit| Broker[DefaultComputeBroker]
  Broker -->|read run task attempt and reserve budget| Sup[Supervisor and reservation state]
  Broker -->|dispatch adapter| Provider[Configured inference provider]
  Provider -->|local route| Local[OllamaProvider]
  Provider -->|pooled eligible route| Remote[Remote fabric client]
  Local -->|guarded HTTP methods| Guard[EgressGuard: coordinator]
  Guard -.->|authorized HTTP request / streamed response| O[Local Ollama]
  Remote -->|ensure_allowed before connect| Guard
  Guard -->|authorized fabric destination| TLS[Fabric client TLS transport]
  TLS -.->|TLS signed fabric request| Ingress[Worker ingress]
  Ingress -->|validate / deduplicate / lease| WS[(Worker store and live lease table)]
  Ingress -->|Ollama provider| WorkerGuard[EgressGuard: worker]
  WorkerGuard -.->|authorized HTTP infer| WO[Worker Ollama]
  Ingress -.->|result / stream frames| Remote
  Remote -->|validate identity lease digests signature| Result[Accepted or rejected result]
  Result -->|settle / release reservation| Broker
```

Solid arrows are local calls/state access; dashed arrows cross process boundaries asynchronously. Worker store persistence is distinct from the volatile active lease table. Route availability depends on enrollment, trust/policy and compute-plane configuration, not just a model name. Evidence: [[engine/litho/tetonic-app/src/compute_plane.rs::pub struct ComputePlane]], [[engine/mantle/tetonic-broker/src/broker.rs::pub struct DefaultComputeBroker]], [[engine/mantle/tetonic-node/src/job_ingress.rs::impl]], [[engine/mantle/tetonic-node/src/lease_table.rs::impl]], [[engine/atmos/tetonic-fabric-protocol/src/result_validate.rs::pub fn]].

The guard nodes represent in-process authorization, with HTTP transport supplied by the guard for Ollama. Fabric performs `ensure_allowed` before its own socket/TLS setup; the worker constructs a provider with a separately configured guard. A routing decision does not bypass these destination checks. [[engine/atmos/tetonic-inference/src/lib.rs::.post_ndjson_stream]], [[engine/atmos/tetonic-fabric-client/src/client.rs::async fn open_fabric_tls]], [[engine/mantle/tetonic-node/src/fabric.rs::pub fn default_ollama]].

The broker is not simply a load balancer: it checks managed execution state, reserves budgets and revalidates before dispatch, tracks in-flight work, and settles/relinquishes reservations on result/error/cancellation paths. Its scheduler/circuit/speculation modules are part of the implementation; their existence does not mean all policies are activated in every composition. Application compute-plane assembly is the reachability evidence. [[engine/mantle/tetonic-broker/src/broker.rs::pub struct DefaultComputeBroker]], [[engine/litho/tetonic-app/src/compute_plane.rs::pub struct ComputePlane]].

Remote result acceptance validates protocol/identity/revocation and execution binding, including run/task/attempt/lease/input/workspace values and signatures. Worker ingress persists accepted request deduplication and terminal cached outcomes, but active execution is not an agent-process migration. A failed request can be rejected, quarantined or retried according to the applicable layer; transport retry and durable task retry must not be conflated. [[engine/atmos/tetonic-fabric-protocol/src/result_validate.rs::pub fn]], [[engine/mantle/tetonic-node/src/job_ingress.rs::impl]], [[engine/mantle/tetonic-run/src/retry.rs::pub fn apply_failure_with_retry]].

## Tool authorization and effect execution — level 3

```mermaid
flowchart LR
  T[Proposed tool action] -->|canonicalize| P[RuntimeActionBroker]
  P -->|evaluate action| Policy[Policy engine]
  Policy -->|deny| Deny[No capability / error]
  Policy -->|approval required| Approve[Attempt approval hook]
  Approve -->|approved| Cap[Issue scoped single-use capability]
  Policy -->|allow| Cap
  Cap -->|register / consume / validate| Exec[Authorized ToolHost]
  Exec -->|workspace edit| Tx[Transaction staging and journal]
  Exec -->|process request| Sandbox[Platform sandbox backend]
  Tx -->|commit / rollback| Files[(Workspace files)]
  Sandbox -.->|spawn / wait / cancel| Child[OS child process]
```

Solid arrows are local control/effect calls; dashed arrow is asynchronous OS process execution. Workspace journal/files are persistence boundaries. This diagram describes application runtime effects, not world-action authorization. Evidence: [[engine/core/tetonic-runtime/src/action_broker.rs::impl]], [[engine/core/tetonic-runtime/src/capability_store.rs::pub struct InMemoryCapabilityStore]], [[engine/litho/tetonic-tools/src/lib.rs::pub fn execute_authorized_cancellable]], [[engine/core/tetonic-sandbox/src/backend/mod.rs::pub fn platform_backend]], [[engine/core/tetonic-transaction/src/lib.rs::pub mod]].

Runtime capabilities bind action kind, canonical parameters, identity/execution scope, workspace/data class and policy version. The broker issues a 300-second, one-use capability; absence/failure of a required approval hook denies the action. This is a local capability boundary, not a proof that an external world validates the same token. Transactional edits and sandboxed process execution have separate cleanup/commit semantics. [[engine/core/tetonic-runtime/src/action_broker.rs::evaluate_and_issue]], [[engine/core/tetonic-runtime/src/capability_store.rs::impl]], [[engine/core/tetonic-sandbox/src/backend/mod.rs::async fn execute_cancellable]].

## World-agent decision — level 3 sequence

```mermaid
sequenceDiagram
  participant World as External world authority
  participant Adapter as WebSocket adapter task
  participant WorldAgent as Agent run_in_world
  participant Brain as PerceptiveBrain
  participant Model as SingleModelBrain / Ollama
  World-->>Adapter: WebSocket perception packet
  Adapter->>Adapter: Parse, apply stop/connection epoch, add last receipt and cadence signal
  Adapter-->>WorldAgent: try_send into bounded perception queue
  WorldAgent->>WorldAgent: Drain backlog, retain latest and apply SensoryFilter
  WorldAgent->>Brain: Await perceive
  Brain->>Brain: Scope memory, build bounded context and choose cadence
  Brain->>Model: Await structured-decision inference over HTTP
  Model-->>Brain: Deltas and completed response or error
  alt Valid complete decision
    Brain-->>Adapter: Queue acknowledgement for included event ids
    Brain-->>WorldAgent: WorldAction with decision and epoch metadata
    WorldAgent->>WorldAgent: Validate manifest and emergency stop
    WorldAgent->>Adapter: Await execute(action)
    Adapter-->>World: WebSocket action with unique action_id
    World-->>Adapter: Correlated action_result
    Adapter-->>WorldAgent: ActionResult or timeout/disconnection error
  else Timeout / malformed / truncated decision
    Brain-->>WorldAgent: No valid action / error path
    Note over Brain,World: No successful-decision acknowledgement from this branch
  end
```

Scope: standalone server. Solid arrows denote local calls; dashed arrows denote network messages, queue delivery or returns as labeled. The world alone owns physical effects; Brain owns volatile experience. An event acknowledgement means inclusion in a validated decision, **not** proof that its resulting action succeeded. Evidence: [[engine/core/tetonic-core/src/agent.rs::pub async fn run_in_world]], [[engine/core/tetonic-runtime/src/websocket_adapter.rs::pub fn connect]], [[engine/mantle/tetonic-server/src/perceptive_brain.rs::impl]], [[engine/core/tetonic-runtime/src/brain.rs::pub struct SingleModelBrain]].

The core loop drains queued perceptions before acting. `SensoryFilter` responds to urgency/events and changed/new signals; it is not a deep comparison of arbitrary world-state JSON. The adapter injects a changing decision-window signal so an otherwise quiet world can still lead to decisions. It also adds the last action receipt. The server's brain enforces decision cadence and a longer idle cadence, with event-driven wakeup logic. [[engine/core/tetonic-core/src/agent.rs::pub async fn run_in_world]], [[engine/core/tetonic-runtime/src/websocket_adapter.rs::decision_window]], [[engine/mantle/tetonic-server/src/perceptive_brain.rs::with_idle_interval]].

The accepted decision contract is structured JSON with an allowed action kind and object payload, plus bounded optional intention fields. The brain records inference/parse/action stages and preserves scoped experience and compact intention state across decisions in the same process. That does not give the agent omniscience: the input state is supplied by the external world and bounded by server context assembly. Conversely, Tetonic's generic adapter does not itself enforce the world's visibility radius or collision rules. Those guarantees must be established in the world implementation. [[engine/mantle/tetonic-server/src/perceptive_brain.rs::impl]], [[engine/mantle/tetonic-server/src/experience.rs::struct]], [[engine/mantle/tetonic-server/src/context_budget.rs::struct]], [[engine/core/tetonic-runtime/src/websocket_adapter.rs::async fn execute]].

One action is pending at a time in the WebSocket transport. A matching `action_id` receipt completes it; an unrelated receipt does not. A successful socket write alone does not complete `execute`. Receipt/deadline handling uses a five-second deadline, and disconnect fails pending/queued actions rather than replaying them. Reconnect increments a connection epoch; stale actions are rejected. A timeout cannot prove an external effect did not happen—it proves no timely correlated completion was returned. [[engine/core/tetonic-runtime/src/websocket_adapter.rs::let mut pending]], [[engine/core/tetonic-runtime/src/websocket_adapter.rs::async fn execute]].

## Context assembly: two separate implementations

The coding/application context package has a staged compiler: normalization, retrieval, filtering, ranking, deduplication, budgeting and sealing. Workspace/index/history inputs belong to this path. The world server instead assembles its current perception, retained experience and intention through its own context-budget module. These are not interchangeable persistent-memory systems. [[engine/strata/tetonic-context/src/lib.rs::pub mod]], [[engine/mantle/tetonic-server/src/context_budget.rs::struct]], [[engine/mantle/tetonic-server/src/experience.rs::struct]]. Token estimates and actual model tokenizer accounting also vary by path; budget arithmetic does not prove exact provider token usage. See the package atlas for compiler stage and tokenizer source files.
'''

docs['state.md']=r'''# State ownership, persistence and recovery

[Overview](README.md) · [Execution](execution.md)

## Owners and durability

| State | Authoritative owner | Persistence / restart boundary |
|---|---|---|
| Live session/conversation and turn ownership | Application `SessionLiveStore` / `LiveSession` | Live objects are in memory; stored messages/session metadata are separate rehydration inputs. |
| Run/task/attempt projection and command history | `DurableRunSupervisor` | Optional SQLite persistence; without a store it uses in-memory maps. |
| Run mutation serialization | Per-run asynchronous mutex | In-process serialization, not multi-coordinator consensus. |
| Coordinator sessions, audit, policy/trust, capacity and run tables | `Store` / `SharedStore` | SQLite; dedicated serialized writer and read connections. |
| Worker pins, ingress dedup/terminal outcome records | Worker store / ingress | Separate worker SQLite state. Active leases use process-local timing/state. |
| Artifacts and quarantined results | Artifact storage implementation | Content/file storage, separate from a run's accepted-artifact reference. |
| Workspace file edits | Transaction subsystem and filesystem | Journal/staging/commit boundary; not a distributed transaction with external tools. |
| Code symbols/chunks/embeddings | Index database | Derived repository index, distinct from authoritative workspace files. |
| World perception, experience, facts, intention, action receipt | Adapter and PerceptiveBrain | In-memory, bounded, scoped; no automatic persistence in server composition. |
| World geometry, objects and actual action effects | External world server | Not owned or persisted by this repository's standalone server. |
| Fleet/organization/squad/keeper maps | Their library instances | In-memory library state; see wiring limitations. |

Evidence: [[engine/litho/tetonic-app/src/session_live.rs::pub struct LiveSession]], [[engine/mantle/tetonic-run/src/service.rs::pub struct DurableRunSupervisor]], [[engine/strata/tetonic-memory/src/lib.rs::pub struct SharedStore]], [[engine/mantle/tetonic-node/src/lease_table.rs::impl]], [[engine/mantle/tetonic-server/src/experience.rs::struct]], [[engine/litho/tetonic-app/src/fleet_api.rs::pub struct FleetManager]]. The full lexical SQL table inventory is [storage-tables.json](storage-tables.json); it lists declarations, including test/migration declarations, not 69 independent production databases.

## Durable command path — level 3

```mermaid
flowchart TD
  Cmd[RunCommand plus envelope] -->|await acquire| Lock[Per-run mutex]
  Lock -->|load and validate sequence / dedup| Snapshot[Current RunSnapshot]
  Snapshot -->|apply command| Transition[Transition / DAG / lease / acceptance checks]
  Transition -->|persistent mode: transactional commit| DB[(Projection plus event plus command dedup)]
  Transition -->|memory mode| RAM[In-memory projection / events / dedup]
  DB -->|after commit| Hook[Run event hook]
  DB -->|read journal / compaction snapshot| Replay[Replay and recovery verification]
  Replay -->|verified projection or recovery required| Snapshot
```

Solid arrows are local synchronous/awaited operations as labeled; no network transport exists in this diagram. The database transaction is the persistence boundary. The per-run mutex is process-local. Evidence: [[engine/mantle/tetonic-run/src/service.rs::fn run_lock]], [[engine/mantle/tetonic-run/src/service.rs::pub fn replay_run]], [[engine/strata/tetonic-memory/src/run_store.rs::commit_run_command]], [[engine/mantle/tetonic-run/src/transition.rs::pub fn apply_command]].

The command envelope carries command id, expected sequence, trace/actor/timestamp, workspace version and idempotency key. Transitions do not simply trust a supplied target status: dependencies, state, input/workspace digests, task versions and lease proof are checked in the relevant handlers. Completion acceptance and artifact acceptance are separate operations. [[engine/core/tetonic-domain/src/run.rs::pub struct CommandEnvelope]], [[engine/mantle/tetonic-run/src/acceptance.rs::pub fn try_accept_completion]], [[engine/mantle/tetonic-run/src/transition.rs::fn apply_accept_artifact]].

## Run lifecycle — level 4

```mermaid
stateDiagram-v2
  [*] --> Created: CreateRun
  Created --> Active: StartRun
  Created --> Succeeded: FinishRun success
  Created --> Failed: FinishRun failure
  Active --> Succeeded: FinishRun success
  Active --> Failed: FinishRun failure
  Created --> Canceled: CancelRun or FinishRun canceled
  Active --> Canceled: CancelRun or FinishRun canceled
  RecoveryRequired --> Canceled: CancelRun with inspected expected sequence
```

Arrows are local command-driven projection transitions, not network messages or execution guarantees. This is the command transition graph, not all recovery assignments. `CancelRun` assigns `Canceling` and then `Canceled` within one transition; the resulting committed projection does not pause at Canceling while processes drain. Recovery code can mark a run RecoveryRequired outside these ordinary command edges. Finishing a run is a separate command; this diagram does not imply all task completion automatically proves a successful run. Evidence: [[engine/mantle/tetonic-run/src/transition.rs::fn apply_start_run]], [[engine/mantle/tetonic-run/src/transition.rs::fn apply_cancel_run]], [[engine/mantle/tetonic-run/src/transition.rs::fn apply_finish_run]], [[engine/mantle/tetonic-run/src/service.rs::pub fn replay_run]].

## Task and attempt lifecycle — level 4

```mermaid
flowchart TD
  T[Created task] -->|dependency recomputation| B[Blocked or Ready]
  B -->|Ready plus CreateAttempt| L[Task Leased / Attempt Created]
  L -->|LeaseAttempt| AL[Attempt Leased]
  AL -->|StartAttempt plus proof| R[Attempt Running / Task Running]
  R -->|ClaimExecution once| Dispatch[Executor may run]
  Dispatch -->|CompleteAttempt validated| Success[Attempt Succeeded / winner selection]
  Success -->|AcceptArtifact| Accepted[Task Succeeded / artifact reference]
  Success -->|RejectArtifact| Failure[Verification failure and retry evaluation]
  AL -->|FailAttempt or ExpireLease| Failure
  R -->|FailAttempt or ExpireLease| Failure
  Failure -->|retry policy and side-effect safety permit| Retry[Task Ready with next_retry_at]
  Failure -->|no retry| TF[Task Failed / dependency propagation]
  Retry -->|subsequent admitted attempt| L
```

Solid arrows represent synchronous state transformation inside the supervisor; Dispatch refers to separate awaited execution, not execution inside SQLite. This view omits cancellation arrows for readability: CancelTask/CancelRun mark applicable active attempts canceled; successful tasks have special rejection/preservation rules. `Starting`, `Superseded` and `Skipped` appear in domain/state handling, but the normal StartAttempt handler goes directly from Leased to Running. Do not insert a mandatory Starting phase from the enum alone. Evidence: [[engine/mantle/tetonic-run/src/transition.rs::fn apply_create_attempt]], [[engine/mantle/tetonic-run/src/transition.rs::fn apply_start_attempt]], [[engine/mantle/tetonic-run/src/transition.rs::fn apply_claim_execution]], [[engine/mantle/tetonic-run/src/acceptance.rs::pub fn apply_winner_selection]], [[engine/mantle/tetonic-run/src/retry.rs::pub fn apply_failure_with_retry]].

Retry state records a failure count/class/reason and next retry time. Retry depends on policy or verification remediation and side-effect safety; otherwise dependency failure propagation runs. The existence of `next_retry_at` is not enough to prove every route to CreateAttempt enforces backoff: the MarkTaskReady handler explicitly checks it while CreateAttempt has its own checks. This is an implementation detail that a simplified retry diagram would conceal. [[engine/mantle/tetonic-run/src/retry.rs::pub fn apply_failure_with_retry]], [[engine/mantle/tetonic-run/src/transition.rs::fn apply_mark_task_ready]], [[engine/mantle/tetonic-run/src/transition.rs::fn apply_create_attempt]].

## Lease and duplicate boundaries

A durable attempt lease contains holder, id, epoch, issued/expiry times and heartbeat sequence. Renewal validates proof and monotonic heartbeat sequence. ClaimExecution requires Running, an unclaimed execution and a current lease. Completion validates current lease, input/task/workspace binding and winner state; a replay of the same successful result digest is handled specially. These controls operate on supervisor snapshots. [[engine/mantle/tetonic-run/src/transition.rs::fn apply_lease_attempt]], [[engine/mantle/tetonic-run/src/transition.rs::fn apply_record_heartbeat]], [[engine/mantle/tetonic-run/src/acceptance.rs::pub fn try_accept_completion]].

Worker active leases and keeper registration proofs are separate mechanisms, not additional replicas of that same lease state. The worker table tracks active job/attempt/epoch expiry using process-local time; keeper code maintains its own node/assignment maps. It would be inaccurate to draw a single globally authoritative lease database across all three. [[engine/mantle/tetonic-node/src/lease_table.rs::impl]], [[engine/mantle/tetonic-node/src/role.rs::impl]].

## Restart and recovery

Supervisor construction attempts recovery; failure places it in a safe mode. Replay reconstructs/validates state from events and any compaction floor snapshot. Interrupted work requires recovery handling rather than continuing arbitrary live futures after restart. Application recovery/resume APIs are explicit higher-level operations, and the process supervisor is only a restart wrapper. [[engine/mantle/tetonic-run/src/service.rs::pub fn new]], [[engine/mantle/tetonic-run/src/service.rs::pub fn replay_run]], [[engine/litho/tetonic-app/src/recovery_api.rs::impl Application]], [[engine/litho/tetonic-app/src/resume.rs::pub fn rehydrate_messages]], [[engine/litho/lokaid/src/supervise.rs::pub fn run]].

The world server does not open this store or call these recovery APIs. Its experience buffer, seen-event cache, facts, intention and trace cursor are recreated at startup. The standalone checkpoint library is a separate file-based facility; no claim of automatic server checkpoint loading follows from its existence. [[engine/mantle/tetonic-server/src/main.rs::async fn main]], [[engine/mantle/tetonic-server/src/experience.rs::struct]], [[engine/core/tetonic-core/src/checkpoint.rs::impl CheckpointManager]].

## Storage relationships — level 3 logical view

```mermaid
flowchart LR
  Session[Session identity] -->|associated run id| Run[Run projection]
  Run -->|contains task records and dependency edges| Task[Task binding and state]
  Task -->|attempt identity / winner| Attempt[Attempt record and lease]
  Attempt -->|validated completion| Result[Result digest]
  Task -->|accepted reference| Artifact[Artifact identity]
  Artifact -->|resolves separately| CAS[(Artifact content store)]
  Run -->|sequence and commands| Events[(Run journal and dedup records)]
  Session -->|separate storage| Messages[(Messages / tool and audit records)]
```

Arrows describe logical references in serialized structures and storage operations, **not** asserted SQL foreign keys. All are local persistence relationships; there is no async network edge in this view. SQLite atomicity covers its own transaction, not CAS files, workspace effects and external services together. Evidence: [[engine/core/tetonic-domain/src/run.rs::pub struct RunSnapshot]], [[engine/strata/tetonic-memory/src/run_store.rs::commit_run_command]], [[engine/strata/tetonic-memory/src/lib.rs::pub struct Store]].
'''

docs['operations.md']=r'''# Interfaces, security, concurrency and operations

[Overview](README.md) · [Execution](execution.md) · [Limits](limits.md)

## External interfaces

| Interface | Server/client and scope | Acceptance is not completion |
|---|---|---|
| Editor JSON-RPC | `lokaid` framed stdin/stdout; method constants/schema/types in `tetonic-rpc` | Admission response precedes streamed work for chat/spawn. |
| CLI arguments | `lokai`, tooling binaries and daemon mode flags | Command choice can bypass normal agent startup entirely. |
| Server TOML | Required `tetonic-server --config`; private deserialized structs | Generic domain EngineConfig is not automatically this executable's configuration contract. |
| Health/debug HTTP | Loopback server listener; trace retrieval by cursor | Transport connected and last observed stage do not prove agent progress or a successful world effect. |
| World WebSocket | Typed perception/action/result envelopes; event acknowledgements | Socket write, decision parsing, event ack and correlated action result are distinct events. |
| Ollama HTTP | Inference provider and worker-local model runtime | Network success does not prove a valid structured decision or successful tool/action. |
| Fabric transport | Enrolled coordinator/client and worker ingress over TLS | Request admission, lease execution and result validation are separate gates. |
| LSP stdio | Workspace-configured language server launched by app/LSP integration | Diagnostics and request results are advisory inputs to coding context/tools. |

Evidence: [[engine/litho/lokaid/src/daemon.rs::pub async fn handle]], [[engine/atmos/tetonic-rpc/src/protocol.rs::pub mod methods]], [[engine/mantle/tetonic-server/src/main.rs::struct Config]], [[engine/mantle/tetonic-server/src/main.rs::let health_adapter]], [[engine/core/tetonic-runtime/src/websocket_adapter.rs::pub fn acknowledge_events]], [[engine/litho/tetonic-app/src/lsp_launcher.rs::impl]].

RPC dispatch includes initialization, session lifecycle/classification/inference selection, chat, cancellation, approvals, models, egress/policy, estate capacity status/doctor/optimize/profile operations, fabric status/trust, agent spawning, project consolidation, run snapshot/resume/cancel and secret-rule/fingerprint operations. Some control-plane mutations are blocked under strict RPC settings. Dispatch, rather than the schema alone, is the current supported-method evidence. [[engine/litho/lokaid/src/daemon.rs::pub async fn handle]].

## Configuration boundaries

The world server rejects unknown TOML fields, non-standalone mode, non-loopback health bind, non-loopback HTTP inference and non-loopback `ws` world URLs. It requires at least a 1000 ms decision interval, nonempty agent id/actions, positive timeout/completion limits, at least 1024 context tokens and room for completion plus safety margin. Defaults include 30-second idle cadence, 384 completion tokens and 512-token margin. It appends agent id/name/event protocol to the world URL. These are startup checks, not a general distributed deployment config loader. [[engine/mantle/tetonic-server/src/main.rs::struct Config]], [[engine/mantle/tetonic-server/src/main.rs::fn default_idle_interval]], [[engine/mantle/tetonic-server/src/main.rs::async fn main]].

The application/daemon instead combines CLI/session settings, environment variables, workspace policy, stored trust/enrollment/capacity state and runtime service bindings. Node startup reads enrollment/fabric ports, bind/advertise settings and `LOKAI_OLLAMA`. A session inference selection or capacity reload does not rewrite the standalone server's TOML or reconstruct its Brain. [[engine/litho/tetonic-app/src/daemon_bootstrap.rs::impl Application]], [[engine/litho/tetonic-app/src/node_worker.rs::pub async fn run_node_serve]], [[engine/litho/tetonic-app/src/product_submit.rs::pub fn reload_inference_services]].

Compatibility is enforced at several independent levels: serde/schema parsing, RPC method dispatch, fabric protocol/version/result checks and world packet parsing. There is no evidence here of one universal version-negotiation mechanism covering all these interfaces. Generated TypeScript declarations are a client shape artifact; runtime support comes from daemon dispatch and handler behavior. [[engine/atmos/tetonic-fabric-protocol/src/result_validate.rs::pub fn]], [[engine/core/tetonic-runtime/src/websocket_adapter.rs::serde_json::from_str]], [[engine/litho/lokaid/src/daemon.rs::pub async fn handle]].

## Concurrency and bounded resources

| Location | Ownership / primitive | Consequence |
|---|---|---|
| Daemon | Four-worker Tokio runtime plus LocalSet; one stdout writer | Live non-Send conversation work remains local; other Send tasks can run independently. |
| Live application turn | Owned conversation and per-session turn admission | Concurrent attempts to own the same turn are rejected rather than freely mutating the conversation. |
| Durable supervisor | Per-run async mutex | Serializes commands in this process; unrelated runs have separate locks. |
| SharedStore | Dedicated writer thread, message commands, read pool | Serializes writes; async callers receive results through completion channels. |
| RPC outbound | Bounded queue with classes, token/progress coalescing and failure status | Backpressure is not an unlimited lossless event log. |
| World adapter | Action queue 8; perception queue 32; ack queue 16; one pending action | Full perception/ack `try_send` can drop the new item; no durable local inbox. |
| World connection | Five-second connect timeout; reconnect backoff 0.5–5 seconds | Connection recovery does not replay pending actions. |
| World action | Five-second receipt deadline; expiry checked by transport timer | Missing receipt fails the local operation; does not establish external rollback. |
| Server trace | At most 2048 events / 8 MiB; payloads over 256 KiB replaced with truncation metadata | Cursor gaps and truncation are observable; trace is not complete durable history. |

Evidence: [[engine/litho/lokaid/src/main.rs::worker_threads]], [[engine/litho/tetonic-app/src/product_submit.rs::struct OwnedTurn]], [[engine/mantle/tetonic-run/src/service.rs::fn run_lock]], [[engine/strata/tetonic-memory/src/lib.rs::pub struct SharedStore]], [[engine/atmos/tetonic-rpc/src/outbound.rs::pub struct OutboundQueue]], [[engine/core/tetonic-runtime/src/websocket_adapter.rs::let (tx, mut rx)]], [[engine/mantle/tetonic-server/src/observability.rs::pub fn record]].

## Cancellation and stop propagation — level 3

```mermaid
flowchart TD
  Cancel[Application cancel / daemon shutdown] -->|close admission and signal| Scope[WorkScope cancellation]
  Cancel -->|supervisor command| Run[Canceled run projection]
  Scope -->|observed at await/check boundaries| Exec[Agent / broker / process work]
  Exec -->|leases released after cleanup| Quiet[Quiescence]
  Stop[World E-Stop packet or adapter stop] -->|set switch / invalidate epoch| Epoch[Adapter stop state]
  Epoch -->|reject stale or stopped action| Gate[Action dispatch gate]
  Ctrl[Server Ctrl+C] -->|select branch / process return| End[Runtime tasks dropped]
```

Arrows are local calls/signals, not network delivery guarantees; the E-Stop packet itself arrives over WebSocket as described in execution. Canceled durable state, requested cancellation and actual process quiescence are separate milestones. World stop and application cancellation are different paths. Evidence: [[engine/core/tetonic-domain/src/work_scope.rs::pub struct WorkScope]], [[engine/litho/lokaid/src/daemon.rs::pub async fn shutdown]], [[engine/mantle/tetonic-run/src/transition.rs::fn apply_cancel_run]], [[engine/core/tetonic-runtime/src/websocket_adapter.rs::Some("estop")]], [[engine/mantle/tetonic-server/src/main.rs::tokio::select!]].

Daemon shutdown rejects new work, cancels sessions and polls in-flight turns until zero or a five-second deadline, then main aborts the writer. Thus the code does not promise that every final notification is flushed before process exit. Sandbox cancellable execution has an explicit fail-closed default for unsupported backends. Backend cleanup behavior remains platform-specific. [[engine/litho/lokaid/src/daemon.rs::pub async fn shutdown]], [[engine/litho/lokaid/src/main.rs::writer.abort]], [[engine/core/tetonic-sandbox/src/backend/mod.rs::async fn execute_cancellable]].

## Trust and security boundaries

Application effect authorization combines policy evaluation, approvals, scoped capability issuance/consumption, workspace path checks and sandbox execution. Egress guard and secret scanning are additional boundaries around data leaving the process. These checks are not a single all-encompassing security layer: each caller must use the configured wrapper. The world server uses loopback restrictions and manifest action names but does not invoke the coding RuntimeActionBroker for world effects. [[engine/core/tetonic-runtime/src/action_broker.rs::evaluate_and_issue]], [[engine/core/tetonic-sandbox/src/backend/mod.rs::fn validate_working_directory]], [[engine/litho/lokaid/src/main.rs::with_redactor]], [[engine/mantle/tetonic-server/src/main.rs::EmptyToolHost]], [[engine/core/tetonic-core/src/agent.rs::pub async fn run_in_world]].

Sandbox selection uses compile-time platform branches for Windows, Linux and macOS. It validates working-directory scopes and builds a minimal environment, excluding secret-looking variable names when requested. Capabilities/enforced controls and missing controls determine isolation outcomes; a sandbox request alone is not proof of equivalent OS enforcement on every platform. No destructive adversarial fixtures were executed for this documentation task. [[engine/core/tetonic-sandbox/src/backend/mod.rs::pub fn platform_backend]], [[engine/core/tetonic-sandbox/src/backend/mod.rs::pub(crate) fn build_minimal_env]], [[engine/core/tetonic-sandbox/src/backend/mod.rs::pub fn evaluate_outcome]].

Worker enrollment persists coordinator pins and TLS identity. Fabric transport and result checks bind work to enrolled identity and execution scope. The separate keeper library's local proof/epoch behavior should not be treated as an equivalent authenticated distributed lease protocol. [[engine/litho/tetonic-app/src/node_worker.rs::pub async fn run_node_enroll]], [[engine/atmos/tetonic-fabric-protocol/src/result_validate.rs::pub fn]], [[engine/mantle/tetonic-node/src/role.rs::impl]].

## Observability — level 3

```mermaid
flowchart LR
  App[Application lifecycle / tool / run events] -->|local callbacks| Audit[(Configured audit / run store)]
  App -->|event translation| RPC[Notifier and redacted queue]
  RPC -.->|framed stdio| UI[Editor / client]
  Brain[World inference request / delta / response / parse] -->|observer callback| Trace[Bounded volatile TraceStore]
  Adapter[Action submit / result / error] -->|observer callback| Trace
  Trace -->|payload-free status| Health[Health snapshot]
  Trace -.->|HTTP cursor polling when enabled| Debug[Raw debug trace consumer]
```

Solid arrows are local synchronous callbacks or state access; dashed arrows are asynchronous client transports. Only the explicitly configured audit/run store is durable in this view. Server health remains available when payload capture is disabled. Evidence: [[engine/litho/tetonic-app/src/events.rs::pub]], [[engine/litho/lokaid/src/main.rs::with_redactor]], [[engine/core/tetonic-runtime/src/brain.rs::pub struct SingleModelBrain]], [[engine/mantle/tetonic-server/src/observability.rs::pub fn record]], [[engine/mantle/tetonic-server/src/observability.rs::pub fn since]].

Trace stages include inference request/response/deltas, parsed decisions, submitted actions/results and classified errors. Health changes to inferring, validating, action_pending, acting, waiting, rejected or failed based on observed stages; it keeps last-success/failure metadata. It is a derived observation, not a second authority over world state. Raw trace is opt-in and bounded; it is not a guarantee of access to hidden model reasoning. [[engine/mantle/tetonic-server/src/observability.rs::pub fn record]], [[engine/mantle/tetonic-server/src/main.rs::let health_adapter]].
'''

docs['limits.md']=r'''# Disconnected implementations, inconsistencies and limits

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

Evidence: [[engine/mantle/tetonic-server/src/main.rs::async fn main]], [[engine/litho/tetonic-app/src/fleet_api.rs::pub struct FleetManager]], [[engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs::impl]], [[engine/mantle/tetonic-node/src/role.rs::impl]], [[engine/litho/tetonic-app/src/operator_control.rs::impl]], [[engine/litho/tetonic-app/src/thought_stream.rs::pub]], [[engine/core/tetonic-runtime/src/lib.rs::pub mod]].

## Fleet state is not execution state

Fleet registration writes management records and status; optional adapter/channel handles determine whether steering can reach a running world loop. An API response labeled Running therefore does not establish inference or action execution. The application composition root has no FleetManager field, and standalone main constructs its agent directly. [[engine/litho/tetonic-app/src/fleet_api.rs::pub struct FleetManager]], [[engine/mantle/tetonic-orchestrator/src/fleet_supervisor.rs::impl]], [[engine/litho/tetonic-app/src/lib.rs::pub struct Application]], [[engine/mantle/tetonic-server/src/main.rs::let agent]].

The organization/squad implementation holds quotas and a shared workpad in memory. A quota field is only a declared limit until the relevant operation enforces it; similarly, a token counter does not by its name establish a rolling hourly window. These structures are not evidence of an operational multi-tenant distributed quota service. [[engine/mantle/tetonic-orchestrator/src/fleet.rs::pub struct]].

## World delivery does not provide global exactly-once effects

The adapter has bounded volatile queues, drops new perceptions when full, and does not replay pending actions on disconnect. Event acknowledgement is queued separately and means decision inclusion. Correlated receipts improve visibility, but no atomic transaction spans model decision, local ack queue, world action, world persistence and Tetonic process restart. Consequently this implementation does not establish exactly-once external effects or crash-safe agent memory. [[engine/core/tetonic-runtime/src/websocket_adapter.rs::pub fn connect]], [[engine/core/tetonic-runtime/src/websocket_adapter.rs::pub fn acknowledge_events]], [[engine/mantle/tetonic-server/src/perceptive_brain.rs::impl]].

The engine accepts world-supplied state JSON and action receipts. Whether a receipt corresponds to physically legal movement, a visibility-constrained observation or durable inventory change is an external-world contract. This package does not claim to audit The Village repository, its Pixi renderer or Colyseus simulation. Those need their own revision-bound evidence if included in a larger system architecture.

## Recovery and distributed guarantees

Durable supervisor serialization is per process and per run; it is not a multi-writer consensus algorithm. Fabric workers provide remote compute and result validation, not transparent movement of live conversations, world adapters or model-local caches. Local keeper maps are not interchangeable with the durable attempt lease/journal. [[engine/mantle/tetonic-run/src/service.rs::fn run_lock]], [[engine/litho/tetonic-app/src/compute_plane.rs::pub struct ComputePlane]], [[engine/mantle/tetonic-node/src/job_ingress.rs::impl]], [[engine/mantle/tetonic-node/src/role.rs::impl]].

Worker ingress initialization loads persisted dedup records but includes fallback/filtering behavior for unavailable or invalid stored entries. That behavior must not be summarized as unconditional crash-safe deduplication. Exact storage-fault/restart outcomes need controlled probes against the deployed storage/environment. [[engine/mantle/tetonic-node/src/job_ingress.rs::impl]].

## Other important limits

- State enums and exported methods include paths beyond the normal caller chain; the diagrams do not imply every variant is reachable from every executable.
- Context accounting is implemented in more than one place and uses estimates in some paths. An input budget is not proof of exact model-window compliance for every tokenizer/model combination.
- CI tests a selected package list and builds two product executables in its matrix; this is not whole-workspace or world-experiment integration coverage.
- OS-specific sandbox implementations are present, but only source inspection is represented here. Cross-platform enforcement and adversarial cleanup have not been rerun for this package.
- A lexical inventory cannot prove absence of dead code, complete macro-expanded call relationships, or all feature-dependent behavior.

Evidence: [[engine/core/tetonic-domain/src/run.rs::pub enum AttemptState]], [[engine/mantle/tetonic-server/src/context_budget.rs::struct]], [[.github/workflows/engine-ci.yml::Run Core Tests]], [[engine/core/tetonic-sandbox/src/backend/mod.rs::pub fn platform_backend]].
'''

for template in HERE.glob('*.md.in'):
    docs[template.name[:-3]]=template.read_text(encoding='utf-8')
for current,body in docs.items():
    rendered=re.sub(r'\[\[(.*?)\]\]',cite,body)
    (HERE/current).write_text(rendered,encoding='utf-8')

# Generated appendix: every package, exact internal manifest edges, every Rust source file.
packages=json.loads((HERE/'packages.json').read_text(encoding='utf-8'))
atlas=json.loads((HERE/'source-atlas.json').read_text(encoding='utf-8'))
rows=list(csv.DictReader((HERE/'coverage.csv').open(encoding='utf-8')))
out=['# Package and module atlas\n','[Overview](README.md) · [Coverage](coverage.csv) · [Evidence](evidence.md)\n',
     'Generated from Cargo metadata and whole-file lexical extraction. Dependency arrows mean **declared local package dependencies**, not runtime calls or network traffic. `dev` and optional dependencies are labeled. Source declarations include test/cfg code and are not proof of production reachability. Ownership, failure handling and operational wiring are described in the narrative; unresolved leaf behavior remains mechanical coverage.\n']
for idx,p in enumerate(packages):
    root=str(Path(p['manifest']).parent).replace('\\','/')+'/'
    out+= [f"## {p['name']}\n",f"Manifest: [{p['manifest']}](../../../{p['manifest']}).\n",'```mermaid','flowchart LR',f'  root["{p["name"]}"]']
    for j,d in enumerate(p['internal_dependencies']):
        label=(d['kind'] or 'normal')+(' optional' if d['optional'] else '')
        out.append(f'  root -->|{label}| d{j}["{d["name"]}"]')
    out+=['```\n','This package-level graph is entirely local build configuration; it has no persistence or process semantics. Evidence is the linked manifest and [packages.json](packages.json).\n','| Target | Kind | Entry source |','|---|---|---|']
    for t in p['targets']:
        out.append(f"| {t['name']} | {', '.join(t['kind'])} | [{t['source']}](../../../{t['source']}) |")
    out+=['\n| Source file | Declarations (lexical) | Control/storage markers |','|---|---:|---:|']
    for a in atlas:
        if a['file'].startswith(root) and a['file'].endswith('.rs'):
            out.append(f"| [{a['file'][len(root):]}](../../../{a['file']}) | {len(a['definitions'])} | {len(a['control_storage_markers'])} |")
    out.append('')
out+=['## Non-package entry surfaces\n','Script entrypoints and all remaining configuration/fixture/resource files are individually classified in the [coverage ledger](coverage.csv). See [script-entrypoints.json](script-entrypoints.json) for discovered functions/main guards. Those indexes do not establish arbitrary script behavior.\n']
(HERE/'atlas.md').write_text('\n'.join(out),encoding='utf-8')
(HERE/'evidence.json').write_text(json.dumps(evidence,indent=2)+'\n',encoding='utf-8')
out=['# Source evidence index\n','[Overview](README.md) · [Coverage](coverage.csv)\n','These anchors were resolved against actual source text. A citation records the scope supporting the adjacent narrative, not a claim that every line of that file was manually verified. Hashes in the ledger bind the source to this snapshot.\n','| Document | Source | Anchor |','|---|---|---|']
for e in evidence:
    out.append(f"| [{e['document']}]({e['document']}) | [{e['file']}:{e['line']}](../../../{e['file']}#L{e['line']}) | `{e['anchor']}` |")
(HERE/'evidence.md').write_text('\n'.join(out)+'\n',encoding='utf-8')
referenced={e['file'] for e in evidence}
for r in rows:
    if r['path'] in referenced:r['review_depth']='selected implementation/call-path review with narrative source anchors; not every statement verified'
with (HERE/'coverage.csv').open('w',newline='',encoding='utf-8') as f:
    w=csv.DictWriter(f,fieldnames=list(rows[0]));w.writeheader();w.writerows(rows)
print(json.dumps({'documents':len(docs)+2,'evidence_anchors':len(evidence),'source_files_cited':len(referenced),'packages':len(packages),'tracked_files':len(rows)}))
