# FAR-003 — Modern inference protocols, streaming and tool continuity

Status: **in progress; OpenAI Responses path implemented**. Size: L. Parent: [frontier agent sprint](plan.md).

## Work

Add OpenAI Responses to the existing guarded inference path; preserve provider call IDs, opaque continuation items, structured outputs and refusal/incomplete states. Stream validated deltas through existing managed events. Upgrade Anthropic message continuity/streaming against its native protocol. Remove synthetic success for missing external tool results. Keep API and subscription request profiles distinct, including their budget limits.

## Acceptance

A streamed model turn calls a tool, receives its actual correlated result, continues and finishes. Parallel calls, missing outputs, split SSE frames, cancel, truncated streams and usage errors have deterministic tests. Reasoning continuation survives without disclosure as shared memory. No failed stream is reported as success.

## Evidence

October 6: the connected OpenAI API-key route uses Responses through the existing hosted broker, egress checks and usage wrapper. The transport incrementally parses bounded SSE and forwards text through the existing token events; partial tool arguments never execute. The final response must be complete and valid. Provider tool IDs now reach the core loop and its actual result messages. Opaque reasoning continuation is retained privately for the in-process conversation, checked against the model/protocol, and excluded from ordinary serialization and Debug output. It is not durable session resumption.

Both older hosted adapters now reject missing external tool results instead of inventing success before a subsequent user message. Only the internal finish transition can receive a synthesized terminal acknowledgement.

Verification: hosted protocol/SSE fixtures, four application/provider tests using fake transport, and 47 core tests. Fixtures cover split UTF-8/CRLF events, incomplete streams, refusal, duplicate IDs, out-of-order correlated results, private continuation and malformed history. No live account inference was run. Anthropic native streaming/thinking, durable continuation, subscription-specific request limits and live protocol proof remain open.
