# Mantle Layer (`engine/mantle/`)

## Purpose
The **Mantle** layer orchestrates swarms, fleets, and distributed execution. It manages run lifecycles, specialist agent assignment, compute broker routing, node daemon operations, and cluster capacity assessment.

## Packages
- [`lokai-run`](./lokai-run): Durable run supervisor, attempt state machines, and event sourcing.
- [`lokai-orchestrator`](./lokai-orchestrator): Multi-agent turn planning, specialist role coordination, and delegation flows.
- [`lokai-broker`](./lokai-broker): Compute provider scheduling, queueing, fallback strategies, and capacity limits.
- [`lokai-node`](./lokai-node): Remote worker fabric ingress server and task execution runner.
- [`lokai-enroll`](./lokai-enroll): Node discovery, mutual TLS certificate enrollment, and cluster joining.
- [`lokai-capacity`](./lokai-capacity): Local and remote hardware topology detection and GPU/VRAM sizing.
