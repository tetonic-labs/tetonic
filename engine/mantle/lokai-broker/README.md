# lokai-broker

Application-facing **ComputeBroker**, hierarchical **AdmissionController**, and
**weighted scheduler** (M6-1 / M6-2 / M6-3 Complete).

## Purpose

Single entry for local and remote compute:

```text
Task ready → placement → admission → schedule → reservation → dispatch → result → release
```

`ComputeBroker` does **not** mutate RunSupervisor task state directly; cancellation
intent is submitted as RunSupervisor commands. **`submit` fails closed** if no
RunSupervisor is bound (`ComputeBrokerError::SupervisorRequired`) — there is no
fail-open snapshot. Inference executes through
`InferenceTargetAdapter` → `PooledProvider` using broker `preferred_target` /
`fallback_order`. `BrokerInferenceProvider` is the outbound Infer scan
chokepoint: every chat runs `ScannerEngine` before dispatch. `redact_outbound`
is the shared function (also used by `lokai-eval` recorded-fixture turns). Local hops
redact and continue. Remote hops fail closed on high-confidence findings
(`InferenceError::RemoteSecretDenied`) and send nothing. A failed redaction
audit write also fail-closes (`InferenceError::SecretScanFailed`).

Infer hops bind attempt IDs through RunSupervisor (`CreateAttempt` + `LeaseAttempt`)
before dispatch. If the ComputeRequest run is not in the journal (interactive chat
sometimes mints `run_{uuid}` when fabric metadata is incomplete), the hop path
**CreateRun + StartRun** first so chat is not fail-closed before Ollama. Sequential
failover fails the previous attempt, then leases a new id. Speculative extra hops use
`HopAttemptMode::Additional` (no unleased `att_{uuid}` mint). Failover/additional hops
fail closed when no supervisor is bound. Ineligible placement still `continue`s
(`revalidate_hop_placement`). Speculation losers cancel the leased loser session via
`ActiveJobRegistry::cancel_session`.

## Key API

| Type | Role |
|------|------|
| `ComputeBroker` / `DefaultComputeBroker` | submit / cancel / status / `chat_admitted` |
| `BrokerInferenceProvider` | `InferenceProvider` facade used by CLI and lokaid; H1-1 outbound secret scan |
| `schedule_infer_chat` / `decide` | placement-gated weighted ranking |
| `revalidate_hop_placement` | fresh `evaluate_placement` per failover hop |
| `MemoryReservationStore` | Durable compute reservations in `lokai.db`. After restart, `recover_reservations` **cancels** active rows (H3-3: the holder process is gone). |
| `InMemoryReservationStore` | Ephemeral fallback when there is no audit store (intentional loss on restart). |
| `MemorySchedulerStore` | durable `SchedulerDecision` in lokai.db |
| `PredictionCalibration` | live MAE → uncertainty margins |
| `CircuitBreakerRegistry` | per-worker Closed/Open/HalfOpen |
| `chat_with_failover` / `continue_after_worker_loss` | broker-driven fallback |
| `HierarchicalAdmissionController` | budgets + queue decisions |
| `dispatch::revalidate_before_dispatch` | placement + capability freshness gate for **remote** workers only. Local (`node_local` / `LocalOnly`) skips the capability cache so a down enrolled worker cannot fail-close loopback Ollama. Also refuses process-class jobs (`TestShard` / `IndexShard`) aimed at a worker with `WorkerTargetRefused` — those are the kinds that would otherwise be silently downgraded to local execution (M1, INV-EXEC-002; test `tests/no_v4_dispatch.rs`). WorkerTarget implements **Infer only** (`lokai-node` `job_ingress.rs` refuses everything else with `UnsupportedJobKind`), so `Embed` / `AnalyzeCode` / `ReviewArtifact` are refused at the worker rather than at placement — M1 CONVERGE C-4. |

## Status

**M6-1 Complete** — admission, hierarchical budgets, queue/fairness, durable
reservations, daemon wiring, IndexShard/TestShard gated paths.

**M6-2 Complete** — weighted scheduler, durable decisions, circuits, Secret
local-only, speculation gate (default off), `LOKAI_SCHEDULER`, MAE calibration
+ `fabric/status` report, per-hop `revalidate_hop_placement`, worker-loss via
in-flight failover (no discarded spawn), `WorkerQuarantined` reason,
observed-finish 25%/5% benches, `speculative_race_sessions` + AJR
`cancel_session` late-loser reject, tail-latency speculation predicate. See
sprint AC table.

**M6-3 Complete** — `CoordinatorObservedTiming` on Infer failover/race,
worker-local durations consumed from signed results, verification Instant on
accept, `record_stage_segments` + critical-path report, speculation cost
metrics, transfer stages, always-retain result rejects, sink storage gate,
`emit_safe_metric`. See `lokai-telemetry` README and sprint AC table.

## Tests

```bash
cargo test -p lokai-broker
cargo test -p lokai-inference --lib
```

Covers admission matrix, scheduler matrix, scheduler benches (25%/5% observed),
hop placement, leased failover/additional attempts, Pooled preferred /
`fallback_order` / Secret placement, H1-1 outbound redaction (PEM plant,
remote typed refusal, audit fail-closed), local hop skipping the remote
capability cache, and CreateRun+StartRun when hop lease finds no journal run.
