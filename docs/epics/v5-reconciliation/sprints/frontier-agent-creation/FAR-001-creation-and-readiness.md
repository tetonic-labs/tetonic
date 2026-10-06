# FAR-001 — Creation contracts and honest readiness

Status: **verified for current profiles**. Size: M. Parent: [frontier agent sprint](plan.md).

## Work

Reuse the existing agent catalog, create endpoint, registered definitions and managed submission. Publish provider/harness/tool compatibility from the engine. Preserve selected tools when changing provider; explain and block incompatible selections. Reject invalid stored configurations instead of silently choosing a local model. Fix prompt-only hosted execution on hosts that also have a working folder. A catalog profile is compatibility evidence, not proof that a credential or remote model is usable.

## Acceptance

The same supported tool set governs catalog and creation. Provider changes preserve user intent. Hosted requests cannot gain file access from the host or an unused consent flag. A hosted prompt-only run completes on a file-enabled host without advertising files. Old valid definitions and identical retries remain valid; malformed definitions fail explicitly.

## Evidence

October 6 implementation publishes provider/harness/tool profiles from the engine and uses them in creation validation and the existing editor. Provider changes retain tool selections; unsupported selections remain visible and block submission until resolved. Invalid stored preference types, tool lists, schema versions and harnesses fail explicitly rather than silently falling back.

Prompt-only hosted submission now removes ambient host workspace settings. The independent hosted activation guard remains intact. A regression repeats the managed hosted journey for both OpenAI and Anthropic on a file-enabled host, asserting that only finish is advertised, secrets do not reach the transport, retries do not duplicate calls, and cancellation/reopen/key removal retain their behavior.

Checks: 25 local-workspace Rust tests passed (three existing live/manual proofs ignored), nine agent-creation/tool-permission UI tests passed, TypeScript passed. All provider calls use fake transports. Frontier tools, dynamic model discovery, vendor harnesses and the larger readiness/entitlement proof remain in subsequent tickets. Existing prompt-only restrictions are deliberately not advertised as tool support.
