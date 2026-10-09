# Core

Shared contracts, the agent loop, runtime integration and policy/effect foundations.

| Package | Current responsibility |
|---|---|
| [tetonic-domain](tetonic-domain/) | Shared types, IDs, commands and interface contracts |
| [tetonic-core](tetonic-core/) | Agent model/action loop and conversation/checkpoint mechanics |
| [tetonic-runtime](tetonic-runtime/) | Agent assembly, action broker, capability and policy integration |
| [tetonic-policy](tetonic-policy/) | Policy evaluation and dispatch guard |
| [tetonic-sandbox](tetonic-sandbox/) | Platform-specific process isolation and effect executors |
| [tetonic-transaction](tetonic-transaction/) | Staged file operations and transaction support |
| [tetonic-secrets](tetonic-secrets/) | Secret detection, redaction and scanner contracts |
| [tetonic-telemetry](tetonic-telemetry/) | Structured execution observations and telemetry sinks |

This is a package group, not an independently deployable service or a strict
layering rule. Use the [current ownership map](../../docs/architecture/ownership.md)
and Cargo manifests for dependencies and change routing. A retained library is
not automatically exposed in the connected product.

Saved configuration requests behavior; current authority is checked at runtime.
Do not bypass managed execution by directly constructing an agent in a product
transport. Effect guarantees depend on host, platform and executor.
