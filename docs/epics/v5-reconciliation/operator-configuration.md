# Operator configuration and production deployment requirements

User requirement, recorded 2026-09-25: an engineer installing Tetonic must be able to configure at least telemetry, logging and storage. Design the engine for production operation and real distributed deployments. These are release requirements, not optional polish or per-agent settings.

## Configuration ownership

The server has a versioned, validated operator configuration. The installation engineer controls infrastructure connections, security boundaries, service limits and operational behavior. Agent definitions reference approved services/resources; an agent or ordinary employee cannot change server storage, logging destinations or telemetry policy through its definition.

Design the schema and deployment contracts in sprint 0. Implement each setting with the subsystem that consumes it; sprint 5 consolidates packaging, examples and compatibility rather than introducing production configuration for the first time.

## Required configuration surfaces

| Area | Operator controls and required semantics |
|---|---|
| Telemetry | Enable/disable supported metrics and tracing exports; destinations/protocols; service/node identity and deployment attributes; trace sampling; bounded exporter queues, retry/timeout and drop behavior; sensitive-data redaction and label-cardinality limits |
| Logging | Level and component filtering; structured format; stdout/stderr or explicitly supported file output; file rotation/retention ownership; correlation identifiers; redaction; bounded buffering and behavior if a destination fails |
| Storage | Select only implemented backends; configure connections or local paths, secret references, TLS where applicable, pool limits and timeouts; separate authoritative metadata/execution storage from memory/artifact content; define durability, migration, backup/restore and retention behavior |
| Network and security | Bind and advertised addresses, transport security, authentication integration, trust roots and credential references, administrative exposure, allowed outbound destinations, and supported worker isolation profiles |
| Distributed execution | Cluster/node identity, enabled roles, control endpoint/discovery, registration/trust procedure, heartbeat and lease timing, reconnect/backoff, placement eligibility, assignment fencing, drain and shutdown deadlines |
| Admission and capacity | Worker concurrency, bounded pending queues, request/output sizes, execution deadlines, resource ceilings and applicable budget settings |
| Providers and capabilities | Approved inference endpoints, tool/MCP connections and resource profiles; credentials by reference; connection limits, timeouts and supported failure behavior |

This is a requirements inventory, not a claim that every backend/export protocol exists today. Select initial supported implementations explicitly. Reject unavailable backend/role combinations rather than accepting decorative settings. Configurable storage does not require arbitrary interchangeable databases in the first release, but backend assumptions must have explicit interfaces and deployment limits.

## Operational semantics

- Document config sources, deterministic precedence, defaults and validation. Provide a validation-only operation and a redacted effective-config view. Unknown settings and invalid combinations fail clearly.
- Classify each setting as dynamically reloadable or restart-required. Validate a reload before applying it; preserve the previous valid configuration on failure. Publish the effective configuration revision per node without exposing secrets.
- Keep credentials out of logged configuration and agent prompts. Support the selected deployment's secret injection/reference mechanism and define credential rotation behavior.
- Logs, telemetry and durable audit/execution records are different facilities. Turning off tracing must not remove required execution history. An unavailable telemetry sink must have bounded impact; required state/audit persistence failure must surface and stop affected admissions/effects according to the documented contract.
- Define liveness versus readiness. Readiness reflects required storage, authority and execution dependencies; optional telemetry failure is separately observable rather than automatically causing restart loops.
- Specify graceful drain and shutdown behavior, including outstanding effects and approval waits. Do not acknowledge durable work before its configured persistence requirement is met.
- State supported topologies: standalone uses the same services with an embedded worker; distributed mode uses authenticated remote workers with tested partitions, stale assignments and reconnects. Never share a local SQLite file across machines or imply multi-controller HA from a configuration enum.
- Require compatible cluster-critical settings and protocol versions across nodes; detect incompatible registration. Configuration cannot override fencing, tenant isolation or required authorization.

## Implementation and release gates

REC-001 defines the schema, source precedence, supported deployment/storage profiles and failure semantics. REC-101/102 implement authoritative storage and server configuration validation. REC-201 adds worker limits and shutdown semantics. REC-301/302 implement scoped resource storage and configurable correlated logging/telemetry. REC-401/402 implement actual distributed settings with fault tests. REC-501 verifies clean installation, upgrade and operational documentation.

Acceptance must include: installing with non-default supported storage settings; configuring log level/output and telemetry destination without recompilation; rejecting invalid configuration before accepting work; redacted config inspection; exporter outage with bounded queues; storage outage with truthful readiness and fail-closed durable operations; worker reconnect and stale-lease rejection; graceful drain; and backup/restore rehearsal. These are future implementation tests, not checks performed by this planning update.
