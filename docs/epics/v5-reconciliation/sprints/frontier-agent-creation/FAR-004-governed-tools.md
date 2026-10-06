# FAR-004 — Selected tools across frontier models

Status: **in progress — scoped OpenAI file reads implemented**. Size: L. Parent: [frontier agent sprint](plan.md).

## Work

Use existing Tools, execution grants, action/process brokers and egress policy for an attempt-scoped tool manifest. Add scoped hosted disclosure authorization before allowing selected file tools; reading a file and sending it to a lab are distinct permissions. Pin schemas and record actual effects/results. Skills and tool packs never grant authority.

## Acceptance

Selected read and reversible write work with a frontier model, while unselected tools, path escape, private-context disclosure and revoked access fail. Actual serialized outgoing data is inspected. Tools and receipts appear in the existing work/map inspector. No placeholder or unconnected service is represented as usable.

## Evidence

The current connected editor allows selected OpenAI file reads (`read_file`, `list_dir`, `grep`, `glob`) where the host permits them. Creation requires explicit disclosure consent, recorded against the canonical configured folder. Submission rechecks that scope, narrows the tool host to the selected tools, and uses existing ResourceService grants, managed execution, action broker, usage reservations and the egress scanner. Changing the host folder invalidates that approval. Prompt-only agents retain no ambient workspace access.

The managed round-trip test uses a fake Responses transport with real temporary files and real tool execution. It proves file contents reach the next model call, original call IDs survive success/denial paths, an unselected write has no effect, parent-directory traversal returns a denial, and a fixture secret blocks further inference. This test found and fixed missing tool-result IDs in regular and failure branches of the existing core loop.

Checks: 28 local-workspace tests passed; three existing live-model scenarios remain ignored. All 47 core library tests and 13 focused editor tests passed; TypeScript passed. The package engineering gate passed (workspace Clippy with warnings denied, formatting, architecture and static quality). Live paid inference, live browser execution and an installed-server restart were not performed.

Still open: reversible writes with integrated approval/finalization, Anthropic tool qualification, real MCP tools, inherited hosted/tool scope for team dispatch, revocation during an active file request, and live provider proof. This is a tested read-only slice, not completion of the acceptance criteria. Preserve the [audit's conformance boundaries](../october-1-coherent-workspace/agent-creation-frontier-audit-2026-10-06.md#10-required-conformance-and-release-evidence).
