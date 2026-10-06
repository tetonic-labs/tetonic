# FAR-005 — Frontier harness adapters inside managed execution

Status: **in progress — pinned Codex protocol probe passed; product adapter not enabled**. Size: L. Parent: [frontier agent sprint](plan.md).

## Work

Extend AgentAttemptExecutor and managed assembly rather than introducing another run store. First certify a pinned Codex app-server profile with selected Tetonic tools via callbacks/MCP; then Claude Agent SDK and a chosen Google runtime. One harness owns each assignment loop. Gate or isolate vendor built-ins, MCPs and internal subagents. Own subprocesses, credentials and sessions; normalize events/usage and stop lineage.

## Acceptance

A managed vendor-harness assignment uses a selected Tetonic tool, obeys denial and inherited cancellation, and settles truthful usage. Test subprocess cleanup, session recovery, credential rotation and unknown effects. Prove egress and native-tool restrictions before advertising a profile. Merely adding a dropdown does not complete this ticket.

## Evidence

The reusable [offline probe](../../../../../scripts/probes/codex_harness.py) exercises the actual installed `codex-cli 0.160.0` executable with a loopback Responses fixture and an isolated home/workspace. No account credentials, real provider requests or subscription usage are required. The probe refuses other versions and retains its scratch folders for inspection.

Four cases passed on Windows:

| Case | Observed |
|---|---|
| Selected dynamic tool | One `item/tool/call` callback, matching thread/turn/call ID; returned text reaches the second model request |
| Unselected custom tool | No host callback; the harness returns an unsupported-tool result |
| Unselected native `exec_command` | No host callback or command event; the harness returns an unsupported-tool result |
| Interrupt during pending tool callback | Turn reports `interrupted`; no tool result/effect or second provider request |

Usage events matched the fixture's input/output counts, accumulated across provider requests. With shell, freeform patch, multi-agent and apps feature flags disabled, the observed manifest still contains `request_user_input`, `get_goal`, `create_goal`, `update_goal`, plus the selected dynamic tool. These internal tools are not Tetonic work resources. This is evidence for this version and tested requests, not proof against every native effect or subprocess escape.

The generated protocol schema for this installed version requires `type: "function"` in each flat dynamic tool definition. The callback response uses `contentItems` and `success`. Correlation includes thread, turn and call IDs; a bare tool name is insufficient. Reference: [official app-server API](https://learn.chatgpt.com/docs/app-server).

## Integration sequence from the probe

1. Extend the existing `AgentAttemptExecutor` boundary through `ManagedRunService::execute_attempt` and admission/finalization. It currently accepts a concrete `tetonic_core::Agent`; do not nest Codex inside that agent's inference loop or create a second work registry.
2. Extract the existing brokered tool-execution path into a reusable attempt-bound host. Dynamic tool callbacks must invoke that path with the stored grant, current authority, cancellation scope and audit receipt. Tool manifests alone are not authorization.
3. Route Codex's custom Responses provider through a per-attempt authenticated local gateway that reuses EgressGuard and usage reservations for each request. The probe proves custom-provider routing, not a production gateway. Never give the harness an upstream credential or allow ambient config/MCPs to bypass that path.
4. Bind the harness process tree and temporary state to the existing WorkScope. Implement interrupt, forced termination, quiescence and stale-callback rejection. The probe demonstrates interruption while awaiting a callback; it does not prove termination of arbitrary owned process trees or recovery after a host crash.
5. Normalize complete output, usage and tool events into the current work inspector. Reconcile cumulative usage rather than charging each cumulative event as a new amount. Carry unknown/in-flight usage honestly.
6. Enable the creation profile only after managed-run conformance passes. Keep API-key and ChatGPT-plan auth as separate routes; this probe certifies neither account entitlement nor live model availability.

Claude and Google adapters, durable harness resumption, live provider proof and the managed Codex product path remain open. Preserve the [audit's conformance boundaries](../october-1-coherent-workspace/agent-creation-frontier-audit-2026-10-06.md#10-required-conformance-and-release-evidence).
