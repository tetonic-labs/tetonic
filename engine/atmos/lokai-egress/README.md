# lokai-egress

Default-deny egress guard — the only crate that owns outbound HTTP clients.

## Role in the stack

Every network byte (Ollama, fabric workers, embeddings) flows through `EgressGuard`. The daemon streams decisions from `activity_log()` as RPC `event/egress` notifications.

## Key API

| Type / fn | Purpose |
|-----------|---------|
| `EgressGuard` | `ensure_allowed`, `post_json`, `get_json`, `activity_log` |
| `allow_node` | Pin enrolled worker IP + port after enrollment |
| `allow_hosted_endpoint`, `revoke_hosted_endpoint` | Explicit grant for one complete HTTPS API URL, separate from workers |
| `post_hosted_json`, `BearerCredential` | Bounded authenticated JSON POST with redacted credentials and pinned public DNS |
| `EgressEvent`, `Action` | Allow/deny audit records |

## Rules (invariant)

- Loopback always allowed
- Public internet denied by default
- Enrolled workers allowed only via explicit `allow_node` rules
- Hosted inference requires an exact endpoint grant; no redirects, inherited proxies, or private-address resolution
- No “allow all” shortcuts

**Shell gap:** `run_shell` and verify subprocesses are **not** brokered through this crate. Egress guarantees the **engine** will not open arbitrary HTTP sockets; a user-approved shell command can still reach the network. Closing that gap is D2 / execution-environment work.

## Dependencies

None (foundation crate).

## Product plan

Charter Right 1; **P2** egress activity log (`EgressEvent`). See [egress-guard-v1](../../../docs/implementation/contracts/egress-guard-v1.md).

## Tests

`cargo test -p lokai-egress` — loopback allow, default deny, enrolled node, client pinning.

Hosted adapter integration and limitations: [HOSTED.md](../../compute/lokai-inference/HOSTED.md).

## Related docs

- [egress-guard-v1](../../../docs/implementation/contracts/egress-guard-v1.md)
- `.cursor/rules/network-safety.mdc`
