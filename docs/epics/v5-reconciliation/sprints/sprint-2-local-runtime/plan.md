# Sprint 2 — One managed local runtime

Historical REC work package. Scheduling and scope are superseded by the [MVP sequence](../README.md); retain applicable technical safeguards as reference.

## REC-201: Local controller and harness boundary

Depends on: sprint 1. Touchpoints: Application, ManagedRunService, AgentAttemptExecutor, EngineRuntime and existing Agent/Conversation assembly.

Reconcile activations into existing run/task/attempt admission and local assignments. Generalize injection of the existing LocalAgentAttemptExecutor while retaining binding/claim checks, worker affinity and WorkScope quiescence. Add start/progress/cancel/outcome and declared recovery support. Runtime context resolves capabilities; harnesses cannot invent grants.

Acceptance: API activation reaches actual execution; observed Running follows accepted claim; duplicate activation executes once according to admission contract; canceled pending work never starts; restart enters the declared recovery mode. Test interruption during waiting/inference, not just between loop iterations.

Retirement: D04 replacement is introduced here; deletion waits for sprint-3 operator/status consumer migration. Do not replace durable transitions with worker-owned flags.

Activation gate: bounded concurrency, output/time limits, required execution-policy configuration and existing broker admission must work before activating agents. Default generic effects to one active attempt; do not inherit coding speculation settings. Preserve manager completion claims/result sealing while moving coding-specific finalization choices.

## REC-202: Move Village execution into the common path

Depends on: REC-201. Extract world behavior from server main/core loop into an approved harness. Use brokered inference and controlled world effects. Preserve context budgets, receipts and scoped memory. Classify coalescible snapshots versus durable messages; implement event cursors/acknowledgments. Bootstrap experiment config as a definition/import, not a special server mode.

Acceptance: actual external Village environment receives authorized actions from an API-created agent; health comes from worker observations; idle wait can be canceled; denied action never reaches adapter; provider usage reaches accounting; restarting the platform yields truthful state. Maintain deterministic fake-world tests plus a documented local live smoke procedure; the visual fallback is not success evidence.

Retirement: D07 special server composition/manual HTTP and D08 alternate lifecycle after parity. Game mechanics remain in Village repository. Exit: coding and world paths share lifecycle, enforcement and event identity.
