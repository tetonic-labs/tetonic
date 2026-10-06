# FAR-003 — Modern inference protocols, streaming and tool continuity

Status: **in progress; OpenAI Responses streaming plus buffered Anthropic/Google tool continuity implemented**. Size: L. Parent: [frontier agent sprint](plan.md).

## Work

Add OpenAI Responses to the existing guarded inference path; preserve provider call IDs, opaque continuation items, structured outputs and refusal/incomplete states. Stream validated deltas through existing managed events. Upgrade Anthropic message continuity/streaming against its native protocol. Remove synthetic success for missing external tool results. Keep API and subscription request profiles distinct, including their budget limits.

Provider-independent follow-up: add a Google/Gemini native protocol adapter under the existing inference boundary and qualify Anthropic's ordinary-tool/MCP result round trips. Reuse the same engine tool manifest and executor across OpenAI, Anthropic, Google and local routes. Preserve each protocol's required private continuation and actual call identifiers; execute only validated complete calls and return genuine results/errors. Verify current official protocol requirements before implementation. Provider-native encoding must not become a separate permission model.

## Acceptance

A streamed model turn calls a tool, receives its actual correlated result, continues and finishes. Parallel calls, missing outputs, split SSE frames, cancel, truncated streams and usage errors have deterministic tests. Reasoning continuation survives without disclosure as shared memory. No failed stream is reported as success.

## Evidence

October 6 follow-up: Anthropic now retains actual `tool_use` IDs and complete signed/redacted blocks privately, correlates out-of-order results and rejects missing, incomplete or refused tool turns. Google generateContent is a native adapter inside the existing hosted boundary, with function declarations, optional native IDs, internal correlation when older models omit IDs, private thought signatures and complete-response validation. Thought tokens contribute to reported output usage. Provider-native search/code execution is not enabled. Both adapters remain buffered; this change does not claim Google/Anthropic streaming, durable continuation or live model qualification.

Protocol fixtures exercise same-name parallel calls, real result IDs, missing/unknown results, private continuation, cross-model rejection, malformed/truncated calls and the internal finish transition. Shared managed file/MCP parity fixtures are in FAR-004/FAR-008. Official references: [Anthropic client tools](https://platform.claude.com/docs/en/agents-and-tools/tool-use/handle-tool-calls), [Google content wire schema](https://github.com/googleapis/googleapis/blob/master/google/ai/generativelanguage/v1beta/content.proto), [Google usage schema](https://github.com/googleapis/googleapis/blob/master/google/ai/generativelanguage/v1beta/generative_service.proto). These are API-key profiles, separate from subscription/vendor-harness work.

### Historical first slice

October 6: the connected OpenAI API-key route uses Responses through the existing hosted broker, egress checks and usage wrapper. The transport incrementally parses bounded SSE and forwards text through the existing token events; partial tool arguments never execute. The final response must be complete and valid. Provider tool IDs now reach the core loop and its actual result messages. Opaque reasoning continuation is retained privately for the in-process conversation, checked against the model/protocol, and excluded from ordinary serialization and Debug output. It is not durable session resumption.

Both older hosted adapters now reject missing external tool results instead of inventing success before a subsequent user message. Only the internal finish transition can receive a synthesized terminal acknowledgement.

Verification: hosted protocol/SSE fixtures, four application/provider tests using fake transport, and 47 core tests. Fixtures cover split UTF-8/CRLF events, incomplete streams, refusal, duplicate IDs, out-of-order correlated results, private continuation and malformed history. No live account inference was run. Anthropic native streaming/thinking, durable continuation, subscription-specific request limits and live protocol proof remain open.
