# Tetonic Engine

Rust workspace powering the Tetonic platform, the Lokai coding assistant, and the Mantle fleet orchestrator: universal execution loop, modular capabilities, durable run supervision, local and remote compute, and developer coding tools.

---

## The Five Earth Layers Architecture

The engine is organized into 5 strict architectural layers plus verification tooling:

```text
engine/
├── Cargo.toml          # Workspace root
├── litho/              # Human workbenches, CLI, TUI, daemon, and coding tools
├── mantle/             # Fleet orchestration, run supervision, broker, and node coordination
├── core/               # Systems kernel, sandbox, transactional staging, secrets, and policy
├── strata/             # Durable memory, artifacts, knowledge index, and context recall
├── atmos/              # Inference providers, network egress guard, RPC, and fabric networking
└── tooling/            # Architecture gates, benchmarks, and behavioral evaluations
```

---

## Layer Packages

| Layer | Packages | Primary Responsibility |
|---|---|---|
| **Litho** (`litho/`) | `lokai-cli`, `lokai-app`, `lokaid`, `lokai-lsp`, `lokai-tools` | Terminal user interface, REPL, application service host, JSON-RPC daemon, LSP client, and coding tool execution. |
| **Mantle** (`mantle/`) | `lokai-run`, `lokai-orchestrator`, `lokai-broker`, `lokai-node`, `lokai-enroll`, `lokai-capacity` | Attempt lifecycle state machines, multi-agent coordination, compute scheduling, remote worker fabric, and hardware capacity detection. |
| **Core** (`core/`) | `lokai-core`, `lokai-runtime`, `lokai-sandbox`, `lokai-transaction`, `lokai-secrets`, `lokai-domain`, `lokai-policy`, `lokai-telemetry` | Neutral agent execution loop, OS process sandboxing, atomic file staging and rollback, secret scanning, domain primitives, and audit telemetry. |
| **Strata** (`strata/`) | `lokai-memory`, `lokai-artifact`, `lokai-context`, `lokai-index` | Durable SQLite storage, content-addressed artifact repository, token-budgeted context assembly, and AST code indexing. |
| **Atmos** (`atmos/`) | `lokai-inference`, `lokai-egress`, `lokai-rpc`, `lokai-fabric-client`, `lokai-fabric-protocol` | LLM client adapters (Anthropic, OpenAI, Ollama), network egress default-deny enforcement, stdio JSON-RPC framing, and fabric protocol wire types. |
| **Tooling** (`tooling/`) | `lokai-arch-gate`, `lokai-bench`, `lokai-eval` | Mechanical architecture invariant enforcement, performance benchmarks, and behavioral evaluation suites. |

---

## End-to-End Execution Flow

The lifecycle of an autonomous turn flows across layers through explicit contracts:

1. **Submit**: The user interaction enters via `litho/lokai-cli` or editor RPC into `litho/lokai-app`.
2. **Admit**: The run supervisor (`mantle/lokai-run`) admits the attempt, registers an attempt ID, and manages lifecycle tracking.
3. **Assemble**: The runtime builder (`core/lokai-runtime`) constructs the agent execution bundle with the required tools, system prompts, and policies.
4. **Loop**: The agent kernel (`core/lokai-core`) executes the turn step-by-step, streaming thoughts and evaluating tool proposals.
5. **Compute**: Model completions are processed through `atmos/lokai-inference` under egress validation (`atmos/lokai-egress`).
6. **Execute**: File modifications and command executions pass through `core/lokai-transaction` and `core/lokai-sandbox`.
7. **Persist**: Memory, events, and artifacts are committed to `strata/lokai-memory` and `strata/lokai-artifact`.
8. **Finalize**: The attempt transitions to completed, events are dispatched back to the UI in `litho`, and transaction changes are committed or rolled back.

---

## Common Verification Commands

From `engine/`:

```bash
# Package verification gate (formatting, clippy, and architectural invariants)
cargo run -p lokai-arch-gate -- verify package

# Fast check on a specific package
cargo run -p lokai-arch-gate -- verify fast --crate lokai-core

# Full architecture gate (includes workspace unit tests)
cargo run -p lokai-arch-gate -- verify full

# Run workspace unit tests
cargo test --workspace

# Non-coding consumer proof
python scripts/verify_layout_consumer.py
```
