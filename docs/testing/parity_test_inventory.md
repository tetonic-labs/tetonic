# V1 Behavioral Parity Test Inventory

## Overview
This inventory lists the deterministic system tests required to prove that the new `lokai-app` Application Kernel preserves the exact workflow logic of the monolithic `lokaid` implementation.

## Test Inventory

### [x] T1: Session Lifecycle Parity (app-layer)
- **Input:** Session start → turn plan → turn complete → session end via `Application`.
- **Assertion:** Normalized `SemanticEffect` sequence matches M0-4 lifecycle model.
- **Tests:** `lokai-eval::parity::kernel_lifecycle_semantic_effects`, `lokai-app::session_turn_lifecycle_event_order`

### [x] T2: CLI vs. Daemon Equivalence (app-layer lifecycle)
- **Input:** Same workspace scenario via headless CLI kernel path and daemon RPC path (mock provider).
- **Assertion:** Identical normalized semantic-effect sequence from `ApplicationEvent` recorders.
- **Test:** `lokaid::daemon::tests::parity::cli_daemon_kernel_semantic_effect_parity`

### [ ] T3: Egress Blocking Parity
- **Input:** An agent attempts an unauthorized network request.
- **Assertion:** Both the legacy and `lokai-app` paths emit the exact same policy denial reason and `ApprovalRequested` event.

### [ ] T4: Crash Recovery Continuity
- **Input:** The daemon is killed mid-turn.
- **Assertion:** Upon restart, both V1 and `lokai-app` resume from the exact same sequence number and execute the remaining tool calls identically.

### [ ] T5: RPC Event Ordering
- **Input:** A series of rapid tool calls and token streams.
- **Assertion:** The internal kernel event bus translates to the exact same order of `event/token` and `event/tool_call` notifications over the JSON-RPC wire.
