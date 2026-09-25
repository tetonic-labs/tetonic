# Tetonic

Tetonic is the sovereign platform for digital autonomous organizations and AI agent execution.

This repository houses **Tetonic Engine**, its terminal client and editor daemon, and the runtime, storage and orchestration components being reconciled into the agent infrastructure platform.

Built in Rust across five geological layers (`litho`, `mantle`, `core`, `strata`, and `atmos`), the platform enforces structural security invariants: network egress is denied by default, tool execution is isolated in operating system sandboxes, and file modifications execute in atomic transactions with rollback guarantees.

---

## Front Door 1: The Tetonic Coding Assistant

The `tetonic` client exposes the existing coding interface and local control commands.

### Installation

#### Method 1: Quick Install Scripts

**macOS & Linux**:
```bash
curl -fsSL https://raw.githubusercontent.com/tetonic-labs/tetonic/main/scripts/install.sh | bash
```

**Windows (PowerShell)**:
```powershell
irm https://raw.githubusercontent.com/tetonic-labs/tetonic/main/scripts/install.ps1 | iex
```

#### Method 2: Direct Binary Downloads

Pre-built binaries containing both the interactive CLI (`tetonic`) and the background daemon (`tetonicd`):

- [Windows (x86_64 .zip)](https://github.com/tetonic-labs/tetonic/releases/latest/download/tetonic-windows-x64.zip)
- [macOS Apple Silicon (M1/M2/M3/M4 .tar.gz)](https://github.com/tetonic-labs/tetonic/releases/latest/download/tetonic-darwin-arm64.tar.gz)
- [macOS Intel (x86_64 .tar.gz)](https://github.com/tetonic-labs/tetonic/releases/latest/download/tetonic-darwin-x64.tar.gz)
- [Linux (x86_64 .tar.gz)](https://github.com/tetonic-labs/tetonic/releases/latest/download/tetonic-linux-x64.tar.gz)

#### Method 3: Install via Cargo
```bash
cargo install --git https://github.com/tetonic-labs/tetonic lokai-cli
```

#### Method 4: Build from Source
```bash
git clone https://github.com/tetonic-labs/tetonic.git
cd tetonic/engine
cargo build --release -p lokai-cli
```
The compiled binary is placed at `engine/target/release/tetonic` (or `tetonic.exe` on Windows).

### Launching the Assistant

Launch the interactive Terminal User Interface (TUI):
```bash
tetonic
```

Or run a single task non-interactively:
```bash
tetonic "Investigate the failing unit tests and propose a fix"
```

### Supported Inference Providers

Tetonic connects to local or hosted models through the Atmos layer:

- **Local Ollama (Default, Zero Network Egress)**:
  Fully offline, private execution on your local hardware. Start Ollama (`ollama serve`), then run `tetonic`.
- **Anthropic (Claude 3.7 Sonnet / 3.5 Sonnet)**:
  Set your API key:
  ```bash
  export LOKAI_ANTHROPIC_API_KEY="your-key-here"
  ```
- **OpenAI (GPT-4o / o3-mini)**:
  Set your API key:
  ```bash
  export LOKAI_OPENAI_API_KEY="your-key-here"
  ```

---

## Front Door 2: The Tetonic Engine Architecture

The Tetonic Engine organizes all capabilities into a **Five-Layer Earth Model**:

```text
┌─────────────────────────────────────────────────────────────┐
│ Litho: Developer Workbenches, Tetonic CLI/TUI, Editor Daemon  │
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

### Layer Navigation Guide

- **`engine/litho/` (Lithosphere)**: The human interface layer.
  Hosts the **Tetonic** coding assistant: `lokai-cli` (legacy Cargo package, `tetonic` CLI/TUI), `tetonic-app` (application coordination and prompts), `lokaid` (legacy Cargo package, `tetonicd` editor daemon), and `tetonic-tools` (sandboxed coding tools).
- **`engine/mantle/` (Mantle)**: The digital autonomous organization layer.
  Hosts the **Mantle** platform: `tetonic-run` (durable run supervision), `tetonic-orchestrator` (multi-agent topology and delegation), `tetonic-broker` (compute scheduling and queues), `tetonic-node` (remote worker tasks), and `tetonic-capacity` (hardware profiling).
- **`engine/core/` (Core)**: The systems kernel.
  Contains `tetonic-core` (pure agent execution loop), `tetonic-runtime` (assembly builder), `tetonic-sandbox` (OS sandboxes: Windows Job Objects, Linux Landlock, macOS sandbox-exec), `tetonic-transaction` (atomic file staging and rollback), and `tetonic-secrets` (credential scanning).
- **`engine/strata/` (Strata)**: The geological memory and persistence layer.
  Contains `tetonic-memory` (SQLite persistence), `tetonic-artifact` (content-addressed immutable artifacts), and `tetonic-index` (AST code indexing).
- **`engine/atmos/` (Atmosphere)**: The external environment and boundary layer.
  Contains `tetonic-inference` (LLM wire adapters) and `tetonic-egress` (strict default-deny network proxy).
- **`engine/tooling/`**: Verification tooling.
  Contains `tetonic-arch-gate` (mechanical architectural invariant enforcement), `tetonic-bench`, and `tetonic-eval`.

---

## Verification Gate

Tetonic mechanically enforces architectural boundaries, code formatting, and strict Clippy rules across all 31 engine packages:

```bash
cd engine
cargo run -p tetonic-arch-gate -- verify package
```

For complete contributor details, review [CONTRIBUTING.md](CONTRIBUTING.md) and the [Charter](docs/CHARTER.md).

---

## License

Tetonic is developed by Tetonic Labs LLC under a community and commercial model (similar to Docker Desktop):

- **Free Permitted Uses**: Free for individual developers, non-profit academic research, open-source projects, and small businesses with **10 or fewer software developers** and **less than $2,000,000 USD in annual revenue/ARR**.
- **Free 30-Day Evaluation**: Any organization of any size may test and evaluate the software internally for 30 days without cost.
- **Commercial Subscription**: Professional and commercial use by organizations with more than 10 developers or $2M+ in annual revenue requires a paid commercial subscription from Tetonic Labs LLC. Contact enterprise@tetonic.ai for details.
- **Four-Year Open Source Conversion**: Each release automatically converts to standard Apache-2.0 four years following its initial public release.

See [LICENSE](LICENSE) for full legal terms and conditions.
