# GATE — Privacy & data sovereignty

**The defining gate of the product.** Runs for **every phase promotion** and **every MINOR** that touches networking, the editor fork, dependencies, the inference fabric, or extensions. A failure here is a **release blocker**, no exceptions — the entire value proposition is "your code never leaves hardware you control, and you can verify it."

**This gate is the enforcement arm of [`docs/CHARTER.md`](../../CHARTER.md).** Each Charter Right maps to the checks below; this is where the Rights become pass/fail. Enforces the invariants in `pivot-agentic-code-editor.md` §3 (INV-1…INV-5) and is the CI counterpart to `contracts/egress-guard-v1.md`.

### Charter Rights → sections

| Charter Right | Checked in |
|---|---|
| 1. Sanctuary | §1 No-egress, §3 Telemetry abolition |
| 2. Ownership | §6 Data at rest |
| 3. Verify, not trust | §5 Supply chain, §8 Verifiability |
| 4. See what the machine sees | §8 (Network Activity Panel) + Context Inspector (design §1) |
| 5. Consent | §7 Extensions, approval gates (see `agent-rpc-v1`) |
| 6. Leave / be forgotten | §6 Data at rest & deletion |
| 7. Persist | §4 Offline-first |
| 8. Compute on your terms | §1/§2 across enrolled cluster (Phase F) |

---

## 1. No-egress proof (INV-1, INV-2)

The core automated check. A representative agent session (open a project, chat, read/search files, propose + apply an edit, run an approved shell command) runs under observation:

- [ ] **Packet capture** during the session shows **zero packets** to any address outside `{ loopback, enrolled-node addresses }`.
- [ ] The **Egress Guard log** (`EgressEvent` stream) accounts for **every** observed connection; no connection occurred without a corresponding logged decision.
- [ ] With **all enrolled nodes removed**, the same session reaches **only loopback**.
- [ ] A deliberately-injected outbound attempt to a public host is **denied** and **logged** (negative test — proves the guard is live, not absent).

**Gate fail:** any packet to a non-allowlisted host; any connection missing from the log; the guard can be bypassed.

## 2. Egress chokepoint integrity (architectural)

- [ ] **Only `lokai-egress` opens outbound sockets.** Dependency-graph lint / `cargo-deny` ban confirms no other crate pulls in an HTTP/socket client for egress.
- [ ] `EgressPolicy.default_action == Deny` is hardwired (not configurable to "allow all").
- [ ] DNS resolve-then-pin and redirect re-check behaviors have unit tests (rebinding/TOCTOU/redirect negative tests pass).

**Gate fail:** a second crate can reach the network; a config flips default to allow-all; rebinding test passes traffic.

## 3. Telemetry abolition (INV-3)

- [ ] **Editor fork:** built-in VS Code telemetry reporters **removed** (not merely disabled); `telemetryLevel` hardwired off; Microsoft online services / Settings Sync / experiments / auto-fetch endpoints stripped or disabled.
- [ ] Marketplace replaced (Open VSX / offline gallery); **no calls to the Microsoft Marketplace**.
- [ ] **Daemon + crates:** no analytics, crash-reporting, or "anonymous usage" calls. Any diagnostics are local files, opt-in, never auto-transmitted.
- [ ] **Model runtime:** Ollama (or chosen runtime) update-checks / telemetry pings are disabled or blocked by the guard.
- [ ] Network capture during **first launch and idle** shows **zero** outbound (no "phone home on start").

**Gate fail:** any default-on telemetry from the editor, daemon, deps, or runtime.

## 4. Offline-first (INV-4)

- [ ] With the network **physically disconnected** (after models are present), the full session in §1 completes: chat, tools, edits, approved shell, verification loop.
- [ ] No feature on the **critical path** of core use requires the network.

**Gate fail:** any core flow errors or blocks when offline.

## 5. Supply chain (privacy lens)

- [ ] `Cargo.lock` / fork lockfiles committed; **offline build** reproduces the release.
- [ ] `cargo-audit` + `cargo-deny` clean (advisories **and** network-capable-crate review).
- [ ] New dependencies reviewed specifically for **outbound network behavior** and telemetry.
- [ ] **Reproducible build** verified (binary provenance matches audited source); release artifacts signed.

**Gate fail:** unaudited network-capable dependency; non-reproducible release binary.

## 6. Data at rest & deletion

- [ ] Documented, user-facing list of **where data lives** (project, `lokai.db`, index, credentials) — all under the user's control.
- [ ] The local store (`memory-store-v2`) has **no network capability** (does not depend on `lokai-egress`).
- [ ] **One-click purge** deletes session history; node credentials never leave the box and are never bundled into any export.
- [ ] Optional encryption-at-rest verified when enabled (key in OS keychain).

**Gate fail:** project content written somewhere undocumented; DB/credentials included in an export that leaves the machine.

## 7. Extensions (largest residual vector)

- [ ] Extensions run under the **Egress Guard** (their network attempts are brokered + logged).
- [ ] Extensions requesting network are **badged** in the UI.
- [ ] Default gallery is curated/offline; docs state plainly that installing an arbitrary networked extension and allowing it out is the user stepping outside the guarantee.

**Gate fail:** an extension can reach the network without appearing in the guard log.

## 8. Verifiability & honesty (INV-5)

- [ ] **Network Activity Panel** present and showing the live `EgressEvent` feed (allows + denies).
- [ ] Source is open/auditable for the audited release.
- [ ] Threat model (§3.2) is documented and **not overstated** — out-of-scope items (compromised OS, user-enrolled hostile node, etc.) are stated plainly. No "100% unhackable" claims.

**Gate fail:** the privacy claim in marketing/UX exceeds what the architecture + threat model deliver ("privacy theater").

---

## Depth by phase

| Phase | Minimum |
|-------|---------|
| 0 | Egress Guard skeleton (§2) + no-egress negative test (§1) on the daemon |
| A | §1 (CLI session), §2, §4 (offline CLI), §5 |
| B | **Full gate** — adds the fork: §3 (telemetry abolition), §7 (extensions), §8 (network panel) |
| C–D | Full gate + audit of new tools/index for egress (§1, §6) |
| E | Reproducible build + signing (§5); privacy-provenance doc published |
| F | §1/§2 across enrolled cluster: traffic stays in-boundary; non-enrolled hosts unreachable |

---

## Checklist template (copy into release notes)

- [ ] No-egress packet capture clean (incl. offline + enrolled-removed runs)
- [ ] Negative test: injected outbound to public host **denied + logged**
- [ ] Only `lokai-egress` opens sockets (dependency lint)
- [ ] Editor telemetry/online-services removed; no Marketplace calls
- [ ] First-launch + idle capture: zero outbound
- [ ] `cargo-audit` / `cargo-deny` clean; reproducible build verified; artifacts signed
- [ ] Data-at-rest locations documented; one-click purge works
- [ ] Extensions brokered + badged
- [ ] Network Activity Panel live; threat model documented, not overstated

**Owner:** _______________ **Date:** _______________
