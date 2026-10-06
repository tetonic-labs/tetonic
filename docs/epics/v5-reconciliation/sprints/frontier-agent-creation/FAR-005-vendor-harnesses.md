# FAR-005 — Frontier harness adapters inside managed execution

Status: **planned**. Size: L. Parent: [frontier agent sprint](plan.md).

## Work

Extend AgentAttemptExecutor and managed assembly rather than introducing another run store. First certify a pinned Codex app-server profile with selected Tetonic tools via callbacks/MCP; then Claude Agent SDK and a chosen Google runtime. One harness owns each assignment loop. Gate or isolate vendor built-ins, MCPs and internal subagents. Own subprocesses, credentials and sessions; normalize events/usage and stop lineage.

## Acceptance

A managed vendor-harness assignment uses a selected Tetonic tool, obeys denial and inherited cancellation, and settles truthful usage. Test subprocess cleanup, session recovery, credential rotation and unknown effects. Prove egress and native-tool restrictions before advertising a profile. Merely adding a dropdown does not complete this ticket.

## Evidence

Pending implementation and verification. Preserve the [audit's conformance boundaries](../october-1-coherent-workspace/agent-creation-frontier-audit-2026-10-06.md#10-required-conformance-and-release-evidence).

