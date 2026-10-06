# FAR-008 — Conformance, installation and end-to-end product evidence

Status: **in progress — fixture coverage added; live product proof open**. Size: L. Parent: [frontier agent sprint](plan.md).

## Work

Run the shared conformance matrix in the source audit for every advertised profile. Validate the existing agent editor → assignment → tool activity → inspected result journey without manual database repair. Cover fresh install, configuration, credential handling, recovery and unsupported profiles. Use fixture tests by default and separately bounded live trials.

## Acceptance

Publish exact tested provider/harness versions, execution profiles, limits and live evidence. At least one complete frontier model path and one vendor harness path pass tool access, usage, cancellation and recovery gates. Honest limitations remain visible; no release claim based only on mocks.

The October 6 provider-independence clarification additionally requires a shared ordinary-tool/MCP conformance matrix for OpenAI, Anthropic, Google and local adapters. Record each profile as implemented, fixture-verified, live-verified or unsupported separately. Exercise the same grant and result semantics across adapters, provider changes with preserved selections and destination checks, delegated team execution, and the first vendor harness. The historical results below do not establish this parity.

## Evidence

### Provider-independent direct-agent increment — October 6, 2026

| Profile | Implemented and fixture-verified | Live evidence |
|---|---|---|
| Ollama/general | Existing managed file tools and local HTTP MCP; same host tool selection as hosted profiles | No new live trial in this increment |
| OpenAI/general | Responses streaming, selected files including staged writes, selected local HTTP MCP | Not run |
| Anthropic/general | Buffered Messages, actual call IDs, private signed/redacted continuation, same file/MCP execution | Not run |
| Google/general | Account model discovery, buffered generateContent, function schemas/results, private signatures, same file/MCP execution | Not run |
| Vendor harness and agreed-plan children with these tools | Still restricted; separate integration work remains | Not established |

The shared managed tests exercise 15 file cases and 12 MCP cases across the three hosted adapters. File cases cover actual reads, staged/finalized writes, unselected writes, path escape and secret-result denial. They also check folder/destination binding and conservative legacy approval. MCP cases cover selected/unselected calls, changed manifests and cancellation with a real local HTTP server. Results are correlated back to the model's IDs and surfaced through the existing task history. Provider tests cover prompt isolation, keys, restart, discovery, cancellation and usage reservation release; the Google catalog test covers pagination, generation filtering and unsafe model IDs. UI tests cover exact selection preservation and fresh data approval across providers.

Validation:

| Command | Result |
|---|---|
| `cargo test --offline -p tetonic-app -p tetonic-inference -p tetonic-core -p tetonic-egress --lib -- --test-threads=1` | Passed: app 220, inference 149, core 47, egress 25; six existing ignored scenarios/benchmarks across packages |
| `cargo run --offline -p tetonic-arch-gate -- verify package` | Passed formatting, workspace Clippy, architecture and quality checks |
| `npm test -- --maxWorkers=2` | 145 passed across 25 files |
| `npm run build` | Passed TypeScript and production build; existing >500 KiB bundle warning remains |

An initial application run found a newly introduced tool-order normalization change. Creation now preserves the original tool order, keeping existing immutable definitions/retries stable; only the disclosure set is normalized. The complete rerun passed. Later malformed-part validation in the Google/Anthropic decoders is covered by the focused hosted protocol rerun. The previously recorded parallel audit-failure classification race remains open.

No paid inference, subscription credential use, external MCP installation, running-engine restart or publication was performed. An existing running engine needs rebuilding/restarting before these backend changes appear. This is source/fixture evidence, not confirmation that every model in an account catalog supports these APIs. Google/Anthropic token streaming, inherited team data/tool grants, installed harness execution, durable provider continuation, remote/authenticated/write MCP and skills remain open.

### Historical evidence

The first slices exercise creation, discovery, Responses SSE/protocol handling, managed file reads and provider payload scanning. The actual pinned Codex executable also passes the separate offline feasibility probe in FAR-005. These are separate evidence levels: a fake hosted transport with real Tetonic execution, and a real vendor harness with a fake provider outside Tetonic execution. Neither is live provider/product proof.

The package engineering gate passes. Focused checks pass: 28 local-workspace tests (three existing live scenarios ignored), 47 core tests, 26 hosted inference tests, 13 editor tests and TypeScript. The full application suite with default parallelism reported 213 passes, three ignored scenarios and one failure in `registered_workspace_job_uses_production_runtime_broker_tools_and_scoped_audit`: the injected audit-error case returned `Canceled` instead of `Failed`. The isolated test passed. The complete application suite then passed using the engineering gate's serial convention: `cargo test --offline -p tetonic-app --lib -- --test-threads=1` reported 214 passes, three ignored. The parallel race remains open; the serial pass is not a fix.

Follow up on that classification race before release: `AuditedAuthority::revoked_during_execution` treats the failed audit latch as revoked authority, while loop-boundary authorization treats it as a failure. If the revocation watcher wins, execution closes WorkScope and finalizes cancellation. Do not merely broaden the test to accept either state. Distinguish an internal audit failure from human cancellation through managed execution and finalization, preserving quiescence and preventing any subsequent effect or inference.

Live inference, browser demonstration on a rebuilt server, durable frontier continuation, installed vendor execution and mixed-provider teams remain unverified/open. Preserve the [audit's conformance boundaries](../october-1-coherent-workspace/agent-creation-frontier-audit-2026-10-06.md#10-required-conformance-and-release-evidence).

## Local MCP read-tool increment — October 6, 2026

The [FAR-006 profile](FAR-006-mcp-and-skills.md) adds actual MCP HTTP transport behind the existing registered executor and action capability broker. Its four application tests include real managed creation → selected tool → inference continuation/history, denial before an unselected call, coexistence with a filesystem grant, manifest-change rejection and managed cancellation. Three egress tests exercise framing and transport failures. Four editor tests exercise explicit discovery and exact selections. These fixtures do not prove a vendor MCP implementation or live model behavior.

Final checks on this increment:

| Command | Result |
|---|---|
| `cargo test --offline -p tetonic-app -p tetonic-core -p tetonic-egress --lib -- --test-threads=1` | App: 218 passed, three live scenarios ignored; core: 47 passed; egress: 25 passed, one benchmark ignored |
| `cargo run --offline -p tetonic-arch-gate -- verify package` | Passed formatting, Clippy, architecture and quality static checks |
| `npm test -- --maxWorkers=2` | 142 tests passed across 25 files |
| `npm run build` | TypeScript and production bundle passed; existing >500 KiB bundle warning remains |
| `git diff --check` | Passed |

The prior default-parallel audit-failure/cancellation classification race remains open; serial results do not resolve it. No running engine was restarted, and no paid inference, persistent OAuth grant, external MCP server installation or publication was performed. The operator must rebuild/restart with `--mcp-config` to expose this profile in an existing local session. Remote/authenticated/write MCP, hosted MCP disclosure, vendor harness attachment, delegated child tools and skills remain incomplete.
