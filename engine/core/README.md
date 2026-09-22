# Core Layer (`engine/core/`)

## Purpose
The **Core** layer serves as the systems kernel for Lokai. It provides the neutral agent loop, hardware/OS sandboxing, transactional workspace operations, domain traits, dynamic policy evaluation, secret sanitization, and structured telemetry.

## Packages
- [`lokai-core`](./lokai-core): Pure agent execution loop, model turn driving, and tokenizer abstraction.
- [`lokai-runtime`](./lokai-runtime): Host agent assembly, execution context management, and runtime isolation.
- [`lokai-sandbox`](./lokai-sandbox): Platform-specific OS sandboxing (Windows AppContainer, Linux Landlock/seccomp, macOS sandbox-exec).
- [`lokai-transaction`](./lokai-transaction): Atomic filesystem staging, transactional edits, rollback mechanisms, and version journaling.
- [`lokai-secrets`](./lokai-secrets): Secret scanning, redaction, and local secure keyring integration.
- [`lokai-domain`](./lokai-domain): Domain traits, action models, tool host interfaces, and core identifier primitives.
- [`lokai-policy`](./lokai-policy): Policy enforcement engine, capability verification, and tool permissions.
- [`lokai-telemetry`](./lokai-telemetry): Structured tracing spans, metrics recording, and audit trail emission.
