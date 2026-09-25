# Sprint 2 — One managed local runtime

## REC-201: Local controller and harness boundary

Depends on: sprint 1. Touchpoints: Application, ManagedRunService, AgentAttemptExecutor, EngineRuntime and existing Agent/Conversation assembly.

Reconcile activations into existing run/task/attempt admission and local assignments. Wrap concrete coding execution in a harness adapter while retaining binding/claim checks. Add start/progress/cancel/outcome and declared recovery support. Runtime context resolves capabilities; harnesses cannot invent grants.

Acceptance: API activation reaches actual execution; observed Running follows accepted claim; duplicate activation executes once according to admission contract; canceled pending work never starts; restart enters the declared recovery mode. Test interruption during waiting/inference, not just between loop iterations.

Retirement: D04 independent fleet lifecycle authority when operator/status consumers migrate. Do not replace durable transitions with worker-owned flags.

## REC-202: Move Village execution into the common path

Depends on: REC-201. Extract world behavior from server main/core loop into an approved harness. Use brokered inference and controlled world effects. Preserve context budgets, receipts and scoped memory. Classify coalescible snapshots versus durable messages; implement event cursors/acknowledgments. Bootstrap experiment config as a definition/import, not a special server mode.

Acceptance: actual external Village environment receives authorized actions from an API-created agent; health comes from worker observations; idle wait can be canceled; denied action never reaches adapter; provider usage reaches accounting; restarting the platform yields truthful state. Maintain deterministic fake-world tests plus a documented local live smoke procedure; the visual fallback is not success evidence.

Retirement: D07 special server composition/manual HTTP and D08 alternate lifecycle after parity. Game mechanics remain in Village repository. Exit: coding and world paths share lifecycle, enforcement and event identity.
