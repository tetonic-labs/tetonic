# FAR-008 — Conformance, installation and end-to-end product evidence

Status: **in progress — fixture coverage added; live product proof open**. Size: L. Parent: [frontier agent sprint](plan.md).

## Work

Run the shared conformance matrix in the source audit for every advertised profile. Validate the existing agent editor → assignment → tool activity → inspected result journey without manual database repair. Cover fresh install, configuration, credential handling, recovery and unsupported profiles. Use fixture tests by default and separately bounded live trials.

## Acceptance

Publish exact tested provider/harness versions, execution profiles, limits and live evidence. At least one complete frontier model path and one vendor harness path pass tool access, usage, cancellation and recovery gates. Honest limitations remain visible; no release claim based only on mocks.

## Evidence

The first slices exercise creation, discovery, Responses SSE/protocol handling, managed file reads and provider payload scanning. The actual pinned Codex executable also passes the separate offline feasibility probe in FAR-005. These are separate evidence levels: a fake hosted transport with real Tetonic execution, and a real vendor harness with a fake provider outside Tetonic execution. Neither is live provider/product proof.

The package engineering gate passes. Focused checks pass: 28 local-workspace tests (three existing live scenarios ignored), 47 core tests, 26 hosted inference tests, 13 editor tests and TypeScript. The full application suite with default parallelism reported 213 passes, three ignored scenarios and one failure in `registered_workspace_job_uses_production_runtime_broker_tools_and_scoped_audit`: the injected audit-error case returned `Canceled` instead of `Failed`. The isolated test passed. The complete application suite then passed using the engineering gate's serial convention: `cargo test --offline -p tetonic-app --lib -- --test-threads=1` reported 214 passes, three ignored. The parallel race remains open; the serial pass is not a fix.

Follow up on that classification race before release: `AuditedAuthority::revoked_during_execution` treats the failed audit latch as revoked authority, while loop-boundary authorization treats it as a failure. If the revocation watcher wins, execution closes WorkScope and finalizes cancellation. Do not merely broaden the test to accept either state. Distinguish an internal audit failure from human cancellation through managed execution and finalization, preserving quiescence and preventing any subsequent effect or inference.

Live inference, browser demonstration on a rebuilt server, durable frontier continuation, installed vendor execution and mixed-provider teams remain unverified/open. Preserve the [audit's conformance boundaries](../october-1-coherent-workspace/agent-creation-frontier-audit-2026-10-06.md#10-required-conformance-and-release-evidence).
