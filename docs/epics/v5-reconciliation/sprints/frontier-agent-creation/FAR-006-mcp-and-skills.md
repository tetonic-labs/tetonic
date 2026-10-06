# FAR-006 — Real MCP connections, tool attachments and skills

Status: **planned**. Size: L. Parent: [frontier agent sprint](plan.md).

## Work

Implement approved stdio/HTTP MCP connection lifecycle, auth, discovery, manifest pinning and invocation through the common tool gateway. Distinguish consuming remote MCPs from exposing Tetonic tools to a harness. Add versioned skill create/import/load and disclose requirements without granting them. Add capabilities in the existing Tools and agent editor surfaces.

## Acceptance

One neutral real test MCP with a read and reversible mutation works through Tetonic and the first vendor harness. Expired auth, changed schemas, missing tools and cancellation are visible. New tools are not automatically granted. Imported skills preserve provenance and cannot escape policy.

## Evidence

Pending implementation and verification. Preserve the [audit's conformance boundaries](../october-1-coherent-workspace/agent-creation-frontier-audit-2026-10-06.md#10-required-conformance-and-release-evidence).

