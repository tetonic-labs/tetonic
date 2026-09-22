# Execution contract v1

**Status:** **Partial** — types in `lokai-domain` (AC2-0); runtime wiring in AC2-1+  
**Authority:** [AC2-consolidation-frame.md](../epics/AC2-consolidation-frame.md)

## Purpose

Every model-originated side effect follows one pipeline:

```text
ProposedAction
  → PolicyDecision (+ ApprovalRequirement)
  → (interactive Approval when required)
  → IssuedCapability
  → AuthorizedAction
  → mandatory Sink (ProcessExecutor | RepositoryMutationService | EgressGuard)
  → ExecutionOutcome
  → AuditRepository
```

Optional hooks and host-specific wiring are **not** a production substitute for this pipeline.

## Core types (`lokai-domain`)

| Type | Role |
|------|------|
| `ProposedAction` | Model/host intent + `AuthorizationContext` |
| `ActionPolicyOutcome` | `PolicyDecision` + `ApprovalRequirement` |
| `IssuedCapability` | Short-lived, scoped permission for one sink |
| `AuthorizedAction` | Capability bound to the action it authorizes |
| `ExecutionOutcome` | Started / completed / failed / cancelled |

Shared ids: `TurnId`, `ActionId`, `JobId`, `AttemptId`, `ApprovalId`, `CapabilityId`, `ExecutionId`.

## Sink traits (sketches)

| Trait | Owner (target) |
|-------|----------------|
| `PolicyEvaluator` | `lokai-policy` |
| `CapabilityIssuer` | `lokai-runtime` |
| `ProcessSink` | `ProcessExecutor` (AC2-3) |
| `MutationSink` | `RepositoryMutationService` (AC2-4) |

HTTP egress remains `lokai-egress::EgressGuard` with capability-checked allow rules.

## Process vs mutation

| | Process | Repository mutation |
|---|---------|---------------------|
| Lifecycle | spawn → run → exit → cancel | base → plan → stage → apply → commit/rollback |
| Owner | `ProcessExecutor` | `RepositoryMutationService` |
| Shared | `IssuedCapability`, audit correlation ids | same |

## Enforcement levels (`ProcessExecutor`)

| Level | Meaning |
|-------|---------|
| **Advisory** | Declared intent + audit only |
| **Constrained** | argv/workdir, env allowlist, timeouts, output limits, process-tree cancel (v1 target) |
| **Sandboxed** | OS network/FS sandbox via platform adapters (future) |

Privacy claims must not exceed the configured enforcement level.

## Production runtime (`EngineRuntime`, AC2-1)

Production assembly **requires**:

- Policy evaluator (`PolicyEngine`)
- Capability issuer
- Approval service
- Audit sink
- Process executor (`ProcessExecutor`, Constrained tier — AC2-3)
- Mutation service (`RepositoryMutationService` — AC2-4)
- Inference provider + provenance (existing fabric seam)

`TestRuntime` may omit components explicitly for unit tests.

Production `EngineRuntime` uses `lokai_tools::EnforcementLevel::Sandboxed` for subprocess sinks.

## Related

- [policy-engine-v1.md](./policy-engine-v1.md) — remote placement rules  
- [AC2-remediation-checklist.md](../epics/AC2-remediation-checklist.md) — AC2 sprint acceptance  

**Last updated:** 2026-06-27 (AC2-3)
