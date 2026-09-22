# lokai-runtime

Mandatory **production runtime assembly** (`EngineRuntime`) for daemon and CLI (AC2-1 / M2-1).

Production paths construct agents via `EngineRuntime::assemble_agent` with mandatory
policy, audit, approval hooks, action broker, capability store, and **Sandboxed** subprocess
enforcement (R6-2). `TestRuntime` is the documented test-only escape hatch.

## Modules

| Module | Purpose |
|--------|---------|
| `assembly.rs` | `EngineRuntime`, `TestRuntime`, `AssemblyMode`, validation |
| `action_broker.rs` | `RuntimeActionBroker` — policy, approval, capability issuance |
| `capability_store.rs` | `InMemoryCapabilityStore` — register + single-use consumption; optional `SharedStore` durability with **revoke-on-restart** (R6-1). |
| `approval.rs` | `ProductionApproval` wrapper + `ApprovalKind` |
| `audit.rs` | `NullAudit` for ephemeral CLI runs |
| `build.rs` | Shared `build_tools`, `base_agent_config` (daemon/CLI parity) |
| `context_provider.rs` | `WorkspaceContextProvider` + `build_production_context_compiler` (R4-1 / R4-3 ScannerEngine) |
| `policy.rs` | `load_policy_engine` from `lokai.db` |

## Broker flow (M2-1)

```text
ProposedAction → canonical digest → policy → approval (if required)
→ capability issuance (registered in store) → pre-execution revalidation
→ capability consumption → ProcessExecutor / RepositoryMutationService → audit
```

Session turns wire the host approval hook into the broker via
`EngineRuntime::set_session_approval_hook` so CLI/RPC approval and capability binding share
one path.

## Key API

- `EngineRuntime::new(policy, approval_hook, artifact_store)` — production runtime handle
- `EngineRuntime::artifact_store()` — same store `complete_turn` seals into (H3-2)
- `EngineRuntime::assemble_agent(mode, parts)` — validates audit/approval in `Session` mode; always attaches `ContextCompiler` with ScannerEngine (R4-1 / R4-3)
- `EngineRuntime::build_tools(ws, allow_shell, extras, mutate)` — Sandboxed tier + capability consumer
- `EngineRuntime::set_session_approval_hook(hook)` — bind interactive approval to broker
- `AssemblyMode::Session` vs `CliEphemeral` — persisted audit + host approval vs one-shot CLI
- `ProductionApproval::host` / `allow_all` / `cli_verify_finish_only`
- `load_policy_engine`, `NullAudit`

## Remaining broker bypasses (M2-1 follow-ups)

| Path | Status |
|------|--------|
| `run_shell`, `write_file`, `edit_file`, verify | Brokered in production via `EngineRuntime` |
| Remote inference dispatch | Guarded by M2-2 `DispatchGuard` |
| File reads, `grep`, index tools | Reads/`grep`/`glob`/`list_dir` brokered (R6-1); index/LSP/git listed as infra |
| Git subprocesses (`ProcessExecutor::run_git`) | Internal/worktree only; not broker-issued |
| LSP subprocesses | Sandbox capability audit + `lokai-lsp` allowlisted spawn (long-lived API ready) |
| Ephemeral tests / `TestRuntime` | Intentional escape hatch — no broker |

## Tests

`cargo test -p lokai-runtime -p lokai-tools -p lokai-arch-gate` — assembly, adversarial
capability tests, architecture gate.

## Related docs

- [M2-1 brokers & approvals](../../../docs/epics/epic-2-side-effects/sprints/milestone-2/M2-1-brokers-approvals.md)
- [execution-contract-v1](../../../docs/implementation/contracts/execution-contract-v1.md)
