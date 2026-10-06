# FAR-003 — Modern inference protocols, streaming and tool continuity

Status: **planned**. Size: L. Parent: [frontier agent sprint](plan.md).

## Work

Add OpenAI Responses to the existing guarded inference path; preserve provider call IDs, opaque continuation items, structured outputs and refusal/incomplete states. Stream validated deltas through existing managed events. Upgrade Anthropic message continuity/streaming against its native protocol. Remove synthetic success for missing external tool results. Keep API and subscription request profiles distinct, including their budget limits.

## Acceptance

A streamed model turn calls a tool, receives its actual correlated result, continues and finishes. Parallel calls, missing outputs, split SSE frames, cancel, truncated streams and usage errors have deterministic tests. Reasoning continuation survives without disclosure as shared memory. No failed stream is reported as success.

## Evidence

Pending implementation and verification. Preserve the [audit's conformance boundaries](../october-1-coherent-workspace/agent-creation-frontier-audit-2026-10-06.md#10-required-conformance-and-release-evidence).

