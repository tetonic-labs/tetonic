# FAR-002 — Account-aware model discovery and connections

Status: **planned**. Size: L. Parent: [frontier agent sprint](plan.md).

## Work

Replace fixed hosted suggestions with authenticated provider discovery through EgressGuard and the existing OS vault. Provide bounded requests, pagination, explicit errors, refresh and custom IDs. Separate API-key and ChatGPT OAuth routes; record connection identity and billing route. Discovery does not prove tool support. Never borrow the desktop app's credentials or silently change billing route.

## Acceptance

The current editor discovers models available to the connected account, retains the selected model across refresh, and shows useful auth/offline/empty results. Unsupported model/protocol combinations are explicit. Tests use fake transports; an opt-in live read validates a configured account without inference.

## Evidence

Pending implementation and verification. Preserve the [audit's conformance boundaries](../october-1-coherent-workspace/agent-creation-frontier-audit-2026-10-06.md#10-required-conformance-and-release-evidence).

