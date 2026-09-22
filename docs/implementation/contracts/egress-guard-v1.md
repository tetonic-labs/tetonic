# Egress Guard — v1 (clean-slate design)

**Status:** Proposed (new codebase). The single sanctioned path for **all** outbound network from the daemon.
**Owner:** `engine/crates/lokai-egress` (Rust).
**Enforces:** Privacy invariants **INV-1** (no exfiltration) and **INV-2** (trust boundary = owned compute) from `pivot-agentic-code-editor.md` §3.

## Why this exists

The product's central promise is that **nothing about the user's code leaves hardware they control**, and that the claim is **verifiable**. A promise enforced by code review and good intentions is not verifiable. This contract defines a **structural chokepoint**: a default-deny network layer that *every* byte of outbound traffic must pass through, plus an observable log so the user can watch it work.

The guard is the reason the whole engine is one Rust daemon: by funneling all networking through `lokai-egress`, the auditable surface for "could this leak?" is **one crate**, not the entire codebase.

## Architectural rule (enforced, not advisory)

> **Only `lokai-egress` may open an outbound socket.** No other crate may depend on `reqwest`, `hyper` client, raw `TcpStream::connect`, `UdpSocket`, or any networking transitively used for egress. This is enforced by a CI lint (`cargo-deny` ban + a dependency-graph check) so a future contributor cannot accidentally bypass the guard.

All inference traffic (`lokai-inference`), update checks, and any other outbound call obtain a client **from** `lokai-egress`; they cannot construct their own.

**Subprocess boundary (AR2-3):** `EgressGuard` applies to **Rust HTTP clients only**. Child processes spawned by `run_shell`, verify-at-finish, LSP, and git **do not** pass through the guard. Subprocess network access is an operator acceptance problem, not an egress-log event.

## Default posture

**Deny everything except the explicit allowlist.** On a fresh install the allowlist contains only loopback to the local inference runtime:

| Default allow | Why |
|---|---|
| `127.0.0.0/8`, `::1` on the configured inference port (e.g. `11434`) | Local Ollama / runtime. |
| `localhost`-bound daemon internals | Loopback only. |

Everything else — including DNS to public resolvers, telemetry endpoints, package mirrors, model registries — is **denied** until the user explicitly enrolls a node (see §Enrollment).

## Policy shape

Illustrative Rust types (the JSON schema is generated via `schemars` and shared with the editor over `agent-rpc`):

```rust
pub struct EgressPolicy {
    pub version: u32,                 // additive; consumers tolerate higher
    pub default_action: Action,       // ALWAYS `Deny` in v1
    pub allow: Vec<AllowRule>,        // explicit, ordered; first match wins
    pub log_all: bool,                // log denies AND allows (default true)
}

pub struct AllowRule {
    pub id: String,                   // stable id (e.g. "node_<hex>")
    pub label: String,                // user-facing ("Workstation-2 / vLLM")
    pub host: HostMatch,              // Exact(ip) | Cidr(range) | Name(dns)
    pub ports: PortMatch,             // Any | List(Vec<u16>)
    pub origin: RuleOrigin,           // BuiltinLoopback | UserEnrolled
    pub expires_at: Option<DateTime>, // optional; absent = no expiry
}

pub enum Action { Deny, Allow }
```

- `default_action` is **fixed to `Deny`** in v1. There is no "allow all" mode — that would make the product a different product.
- Rules are evaluated in order; **first match wins**; no match ⇒ `default_action`.
- `HostMatch::Name` (DNS) rules are resolved and **pinned** at connect time (see threat handling) — they exist for enrolled LAN hostnames, not arbitrary internet names.

## Enforcement mechanism

`lokai-egress` exposes the only HTTP client factory in the daemon. It builds a `reqwest`/`hyper` client over a **custom connector** that:

1. **Intercepts DNS.** The connector resolves the destination itself, then checks the **resolved IP + port** against the policy — not just the hostname. This closes DNS-rebinding (a `Name` rule that resolves to a public IP is denied).
2. **Pins the resolved address.** The socket connects to the exact IP that was checked; no re-resolution between check and connect (TOCTOU-safe).
3. **Re-checks every redirect hop.** Redirects are followed only if each hop's resolved address also passes; otherwise the request fails closed.
4. **Ignores ambient proxy config.** `HTTP(S)_PROXY`/`ALL_PROXY` env and system proxies are **not** honored unless the proxy address is itself an allow rule (prevents silent tunneling out).
5. **Fails closed.** Any error in policy evaluation denies the connection.

Defense-in-depth (documented, optional, not primary): OS firewall rules and running the daemon in a restricted network namespace. The in-process connector is the **portable primary** so the guarantee holds identically on every OS.

### Implementation status (current)

`engine/crates/lokai-egress` implements the model above with reqwest's resolution-override mechanism rather than a hand-rolled hyper connector — equivalent for the plain-HTTP local runtimes we talk to today:

1. **Resolve-then-check.** `authorize()` resolves the destination and checks **each resolved address** (IP + port) against the policy, not the hostname. Loopback is always allowed; non-loopback requires an enrolled `AllowRule`. Default-deny on no match.
2. **Pin to the authorized addresses (TOCTOU-safe).** For a hostname target the request is sent through a per-request client built with `resolve_to_addrs(host, &authorized_addrs)`, so reqwest connects **only** to the addresses that passed policy and never performs a second, unvetted lookup at connect time. *All* authorized loopback addresses are kept (e.g. both `::1` and `127.0.0.1`) so address-family fallback still works. IP-literal targets reuse the shared client (no DNS to subvert).
3. **Redirects disabled (fail closed).** The client uses `redirect::Policy::none()`, so a `3xx` cannot bounce a request to an unauthorized host behind the guard's back; the caller sees the redirect instead.
4. **Single socket owner.** `EgressGuard` is the only holder of a `reqwest::Client`; all of `lokai-inference` (chat, generate, embed, capabilities, tags) and any other outbound call go through `post_json` / `post_ndjson_stream` / `get_json`.
5. **Fails closed.** Any resolution or policy error denies.

Known residual (backlog, low risk for loopback-only Phase A): the policy check still triggers a system DNS lookup for a hostname *before* denial, so a name's existence (not its content) can reach the resolver; redirect **re-checking** per hop is moot while redirects are disabled but would return if we ever enable them; ambient-proxy hardening (item 4 of the ideal model) is not yet wired.

## Activity log (the observable proof)

Every decision emits an `EgressEvent`. These stream to the editor's **Network Activity Panel** (via `agent-rpc` notification `event/egress`) and are appended to the local audit DB (`lokai-memory`).

```rust
pub struct EgressEvent {
    pub ts: DateTime,
    pub initiator: String,     // "inference:ollama", "update-check", "extension:<id>", ...
    pub host: String,          // requested host (name or ip)
    pub resolved_ip: Option<IpAddr>,
    pub port: u16,
    pub decision: Action,      // Allow | Deny
    pub matched_rule: Option<String>, // AllowRule.id, if any
    pub reason: String,        // "builtin loopback" | "no matching rule (default deny)" | ...
}
```

- **Denies are logged too** — the user can *see* the app trying (and failing) to reach somewhere it shouldn't, which is exactly what builds trust.
- The panel offers a one-click "this should never happen — show me what asked" drill-down to the initiator.

## Enrollment (cluster nodes)

Adding a node to the trust boundary is the **only** way to widen the allowlist, and it is an explicit, user-driven action. v1 defines the hook; the auth/handshake details are a separate contract (`node-enrollment-v1`, TBD):

- User enrolls a node by address; the daemon performs a mutual-auth handshake proving the node runs the user's own runtime (shared key / cert pinning).
- On success, a `UserEnrolled` `AllowRule` is added (scoped to that node's IP + inference port).
- Enrollment is reversible; removing a node deletes its rule immediately and drops live connections.

## Verifiability

- **Privacy CI gate** (`gates/GATE-privacy-sovereignty.md`): a representative agent session runs under packet capture; the test asserts **zero packets to any non-allowlisted address** and that the `EgressEvent` log accounts for every observed connection.
- **Reproducible builds** let a third party confirm the shipped binary contains this guard unmodified.

## Threat handling summary

| Vector | Handling |
|---|---|
| DNS rebinding | Resolve-then-check, then pin the request's resolution to the authorized addresses (implemented via `resolve_to_addrs`). |
| TOCTOU (resolve vs connect) | reqwest connects only to the addresses that passed policy; no second system lookup at connect time (implemented). |
| HTTP redirects to a new host | Redirect-following disabled (`Policy::none()`), so a `3xx` can't bypass the guard; per-hop re-checking returns if redirects are ever enabled. |
| Ambient/system proxies tunneling out | Proxies ignored unless explicitly allow-listed. |
| A crate opening its own socket | CI dependency-ban lint; only `lokai-egress` may. |
| Extension network calls | Brokered + logged like everything else; networked extensions badged (editor side). |
| Policy evaluation bug | Fail closed (deny). |

**Out of scope (per §3.2 threat model):** a compromised OS/kernel, a user who deliberately enrolls a hostile node, raw traffic-analysis, covert channels. The guard guarantees *the application will not betray you*, not that the machine is unhackable.

## Compatibility & extensibility

- `EgressPolicy` and `EgressEvent` are **additive-only** within v1; new optional fields may be added and consumers MUST tolerate unknown ones.
- `default_action = Deny` and "only `lokai-egress` opens sockets" are **invariants**, not configurable — changing either is a new major version and a product-level decision.

## Mission alignment

| Principle | How honored |
|---|---|
| Sovereignty first | The user's allowlist is the *only* thing that widens reachability. |
| Private by architecture | One mandatory chokepoint; bypass is impossible by construction (CI-enforced). |
| Verifiable | Observable log + packet-capture CI gate + reproducible builds. |
| Capable | Local + enrolled nodes are fully reachable, so a self-built cluster works at full speed inside the boundary. |

---

**Last updated:** 2026-06-27 (added implementation-status note: resolution-pinned requests close the resolve→connect TOCTOU / DNS-rebinding gap; redirect-following disabled).
