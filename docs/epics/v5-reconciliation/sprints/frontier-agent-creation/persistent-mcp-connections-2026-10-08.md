# Persistent MCP connections — October 8, 2026

## Product slice

The workspace owner can connect an MCP service from Tools & MCPs or the existing
inline capability library in agent creation/editing. The sequence is connect,
discover, review read tools, then select them for an agent. No separate screen or
map redesign was introduced, and the agent draft survives the setup flow.

The same connection detail supports tool review, token replacement and explicit
disconnect. Connection failures explain authentication versus general transport
failure without exposing response bodies or tokens. A disconnected or changed
connection does not erase saved agent selections.

## Integration and limits

- Schema 69 stores scoped definitions and exact approved manifests in the existing
  Store. Only the team owner may read/write these control records. Writes compare
  revisions; conflicting/uncertain saves require a reload. No plaintext credential
  or credential hash is saved in the record, browser storage, or returned catalog.
- Existing KeyStorage holds service tokens. A token replacement changes the
  credential generation, clears approved manifests and changes the versioned tool
  IDs. Tools cannot silently switch accounts. Endpoints are immutable.
- The existing MCP registry merges startup-configured local services and durable
  workspace services. Duplicate IDs fail rather than overriding operator settings.
  Current storage authority is checked on invocation and by the managed execution
  authority watcher. No separate scheduler, grant system or tool runtime was added.
- EgressGuard supports exact public HTTPS endpoints and the existing numeric
  loopback HTTP profile. Remote DNS is checked and pinned per request; TLS remains
  verified; redirects and ambient proxies are disabled. Tokens are sent in the
  Authorization header only to the configured endpoint. No arbitrary header editor.
- Read-only annotations are compatibility hints, not proof that a tool is safe.
  Only explicitly reviewed manifests enter the available tool catalog. Agent grants
  and provider disclosure approval remain separate. Every call re-lists the server
  manifests before dispatch; changed schemas cannot silently inherit authority.
- Disconnect denies further execution and clears the token reference. Waiting on
  an in-flight call is interrupted, but remote termination/rollback is not promised.
  Removing one reviewed tool also revokes that capability without deleting the
  saved agent definition. Agent executions continue using existing budgets and
  managed stop lineage.

This local-owner profile supports service tokens and unauthenticated services;
it does not implement OAuth discovery/consent/refresh or universal hosted MCP
compatibility. MCP mutations, stdio processes, background tasks, resource/media
results, native vendor harness attachment and automatic blocked-work resumption
remain separate work. The supported protocol behavior follows the
[MCP Streamable HTTP transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports);
a manually supplied service token is not a claim of implementing the
[OAuth authorization lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization).

## Verification

- Application library: 253 passed, 3 opt-in live-model demos ignored. Memory:
  181 passed. Egress: 27 passed, 1 benchmark ignored. After the final revocation
  changes, all 9 targeted application MCP tests passed again. These cover actual
  bearer-header transport, discovery/review, restart, managed agent invocation,
  manifest/account changes, authentication errors and in-flight revocation.
- Strict Clippy for app, memory, egress and CLI (all targets, warnings denied),
  architecture gate, quality gate, formatting and the CLI build passed.
- Web: all 197 tests passed. After the final presentation adjustments, 11 targeted
  connection/tool-permission tests and the production build passed. The existing
  bundle-size advisory remains (approximately 591 kB before compression).
- An isolated engine and browser workspace connected to the public
  [Microsoft Learn MCP endpoint](https://learn.microsoft.com/en-us/training/support/mcp-developer-reference)
  over verified HTTPS. All three read tools were discovered with no initial
  approval. Explicitly approving one made only that tool available. The same
  connect/discover/review flow passed through the real UI; refresh preserved the
  selection. This live check exercised discovery/review, not a frontier-model
  execution. Authenticated execution is covered by the local protocol fixture.
- The existing manual engine on port 3004 was updated after checking that no work
  was active and taking a SQLite backup. All 27 task IDs and all four complete
  agent definitions were preserved. The browser was reconnected on port 5177 and
  returned to the existing project. No test services were added to that workspace.

OAuth and remote writes remain explicit follow-up work; this slice does not
claim compatibility with services that require browser authorization.
