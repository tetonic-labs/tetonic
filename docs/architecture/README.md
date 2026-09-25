# Tetonic Architecture

Tetonic is built around a five-layer Earth model that isolates user interfaces (Lokai), multi-agent fleet orchestration (Mantle), execution safety, persistent memory, and external network interactions.

---

## Architectural Principles

1. **Digital Sanctuary**: The developer machine is private by default. Outbound network traffic is forbidden except through explicit, allowlisted egress adapters.
2. **Defensive Isolation**: The application and UI layers never touch the raw operating system directly. All file modifications use transactional staging, and all child processes execute within OS sandboxes.
3. **Clean Product Separation**:
   - **Lokai Coding Assistant** (`litho/`): The interactive developer tool, TUI, and editor daemon.
   - **Mantle Platform** (`mantle/`): Generic fleet orchestration and digital autonomous organization infrastructure. Coding heuristics belong exclusively in Litho.

---

## The Five Earth Layers

```text
┌─────────────────────────────────────────────────────────────┐
│ Litho: User Interfaces, CLI, TUI, Editor Daemon, Tools     │
├─────────────────────────────────────────────────────────────┤
│ Mantle: Fleets, Run Lifecycles, Broker, Node Coordination   │
├─────────────────────────────────────────────────────────────┤
│ Core: Agent Loop, Sandboxing, Transactions, Policies        │
├─────────────────────────────────────────────────────────────┤
│ Strata: Durable Memory, Artifacts, Knowledge Index          │
├─────────────────────────────────────────────────────────────┤
│ Atmos: LLM Inference, Egress Guard, Wire RPC Protocols      │
└─────────────────────────────────────────────────────────────┘
```

### 1. Litho (`engine/litho/`)
The interface layer between human developers and the agent engine.
- [`lokai-cli`](../../engine/litho/tetonic-cli): Interactive terminal user interface (Ratatui-based).
- [`lokai-app`](../../engine/litho/lokai-app): Application service layer, product definitions, and agent prompts.
- [`lokaid`](../../engine/litho/tetonicd): Background daemon exposing JSON-RPC over stdio for IDE integration.
- [`lokai-lsp`](../../engine/litho/lokai-lsp): Language Server Protocol client.
- [`lokai-tools`](../../engine/litho/lokai-tools): Concrete coding tool implementations bound to sandboxed executors.

### 2. Mantle (`engine/mantle/`)
The digital autonomous organization and distributed execution layer.
- [`lokai-run`](../../engine/mantle/lokai-run): Attempt lifecycle tracking, durable run supervision, and event dispatch.
- [`lokai-orchestrator`](../../engine/mantle/lokai-orchestrator): Multi-agent role coordination and specialist delegation.
- [`lokai-broker`](../../engine/mantle/lokai-broker): Compute scheduling, queue management, and fallback strategies.
- [`lokai-node`](../../engine/mantle/lokai-node): Remote worker task runner and cluster fabric endpoint.
- [`lokai-enroll`](../../engine/mantle/lokai-enroll): Mutual TLS node discovery and cluster joining.
- [`lokai-capacity`](../../engine/mantle/lokai-capacity): Hardware topology, GPU, and VRAM sizing.

### 3. Core (`engine/core/`)
The systems kernel providing execution primitives and safety boundaries.
- [`lokai-core`](../../engine/core/lokai-core): Pure agent execution loop driving step-by-step model turns.
- [`lokai-runtime`](../../engine/core/lokai-runtime): Runtime agent assembly and execution isolation.
- [`lokai-sandbox`](../../engine/core/lokai-sandbox): Platform-specific OS sandboxing (Windows Job Objects / AppContainer, Linux Landlock/seccomp, macOS sandbox-exec).
- [`lokai-transaction`](../../engine/core/lokai-transaction): Atomic file staging, transactional edits, and rollback mechanisms.
- [`lokai-secrets`](../../engine/core/lokai-secrets): Secret scanning, credential detection, and redaction.
- [`lokai-domain`](../../engine/core/lokai-domain): Domain models, tool traits, and core identifiers.
- [`lokai-policy`](../../engine/core/lokai-policy): Dynamic policy evaluation and permission checks.
- [`lokai-telemetry`](../../engine/core/lokai-telemetry): Structured tracing and audit spans.

### 4. Strata (`engine/strata/`)
The durable state and knowledge repository layer.
- [`lokai-memory`](../../engine/strata/lokai-memory): SQLite storage for sessions, runs, and memories.
- [`lokai-artifact`](../../engine/strata/lokai-artifact): Content-addressed immutable artifact store.
- [`lokai-context`](../../engine/strata/lokai-context): Token-budgeted context assembly and recall algorithms.
- [`lokai-index`](../../engine/strata/lokai-index): AST-based code indexing and semantic search.

### 5. Atmos (`engine/atmos/`)
The external environment and network boundary layer.
- [`lokai-inference`](../../engine/atmos/lokai-inference): Multi-provider model adapters (Anthropic Claude, OpenAI, Ollama).
- [`lokai-egress`](../../engine/atmos/lokai-egress): Strict outbound network proxy enforcing default-deny policies.
- [`lokai-rpc`](../../engine/atmos/lokai-rpc): Protocol schemas and framing contracts for stdio JSON-RPC.
- [`lokai-fabric-client`](../../engine/atmos/lokai-fabric-client): Cluster fabric client transport.
- [`lokai-fabric-protocol`](../../engine/atmos/lokai-fabric-protocol): Wire types and serialization for fabric communication.

---

## Tooling (`engine/tooling/`)
- [`lokai-arch-gate`](../../engine/tooling/lokai-arch-gate): Mechanical checker validating structural layer boundaries, lint rules, and dependency constraints.
- [`lokai-bench`](../../engine/tooling/lokai-bench): Performance benchmarking suite.
- [`lokai-eval`](../../engine/tooling/lokai-eval): Behavioral eval corpus and grading harness.
