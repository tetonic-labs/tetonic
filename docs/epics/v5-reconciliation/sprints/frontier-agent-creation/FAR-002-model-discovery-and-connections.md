# FAR-002 — Account-aware model discovery and connections

Status: **in progress; API-key discovery verified**. Size: L. Parent: [frontier agent sprint](plan.md).

## Work

Replace fixed hosted suggestions with authenticated provider discovery through EgressGuard and the existing OS vault. Provide bounded requests, pagination, explicit errors, refresh and custom IDs. Separate API-key and ChatGPT OAuth routes; record connection identity and billing route. Discovery does not prove tool support. Never borrow the desktop app's credentials or silently change billing route.

## Acceptance

The current editor discovers models available to the connected account, retains the selected model across refresh, and shows useful auth/offline/empty results. Unsupported model/protocol combinations are explicit. Tests use fake transports; an opt-in live read validates a configured account without inference.

## Evidence

October 6: API-key model discovery for OpenAI and Anthropic now uses the existing OS vault and EgressGuard. The authenticated local API exposes bounded catalog reads, cursor pagination, model validation/deduplication and explicit errors. No model call is made to discover availability. The connected editor has account-backed choices, refresh, preserved selections, credential-aware request cancellation and manual model IDs; fixed hosted suggestions are removed.

Verified with four application/provider fixtures, six egress tests, twelve UI tests, TypeScript and CLI compilation. No live account was queried. Discovery explicitly does not certify tool/protocol compatibility. OAuth, durable multi-account identity, current capability qualification and live account validation remain open, so the ticket is not complete.
