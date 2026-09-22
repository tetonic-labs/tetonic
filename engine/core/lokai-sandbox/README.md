# lokai-sandbox (M2-3 / H1-3)

OS process-execution backends for **Windows, Linux, and macOS**. Production
confinement gaps remain tracked in the v1 readiness audit; platform support is not
a blanket guarantee for hostile repository execution.

## Mechanisms

See [`docs/enforcement-matrix.md`](docs/enforcement-matrix.md) for the authoritative per-platform table.

| Control | Windows | Linux | macOS |
|---------|---------|-------|-------|
| Process tree | Job Objects | process group | process group |
| Network denial | Firewall outbound block when **elevated** | user+net namespace | `sandbox-exec` Seatbelt |
| Memory | Job limit | `RLIMIT_AS` | `RLIMIT_AS` |
| FS OS filter | No (broker only) | No | No |

`network_denial: true` is advertised only when the OS path is actually used. Shell/verify profiles request `IsolationLevel::Strict` when denial is available; otherwise they stay `Standard` (H1-2 warn path) so non-elevated Windows desktops are not hard-denied.

## Isolation outcomes

- **Sandboxed** — requested High controls that the platform supports are OS-enforced.
- **BrokeredWithWarning** — process runs with explicit missing-control report (never silent).
  High-risk gaps set `user_approval_required` (H1-2 approval prompt).
- **Denied** — `Strict` + High missing control; no unrestricted fallback.

`preview_confinement(ProcessClass, workspace)` predicts the same gaps without spawning.

## Tests

Unix launches create their own process group before exec. One-shot execution owns
that group through normal exit, errors, deadlines, and future drop. Leader exit
terminates remaining group members before finishing pipe collection, so inherited
pipes cannot keep an otherwise completed command alive. Long-lived service owners
terminate their groups on explicit termination and Drop. Synchronous service
cleanup waits for the direct child; async Drop relies on Tokio's child reaper.

Process groups do not prevent a hostile child from creating a new session/group.
This lifecycle contract also does not solve manager-level cancellation of already
running `spawn_blocking` work. Long-lived Unix stderr is discarded instead of left
in an unread pipe that can fill and stall a service.

The synchronous macOS path uses the same Seatbelt wrapper as asynchronous launches.
Memory-limit installation errors fail spawn instead of being silently ignored.

```text
cargo test -p lokai-sandbox
```

Includes `tests/h1_3_os_confinement.rs` (platform label, enforce-or-report, outbound denial when available, Standard still brokers).

## CI

Ubuntu (workspace), `windows` / `windows-required`, `macos` / `macos-required`.

The independent `unix-lifecycle` matrix runs sandbox unit tests and the seven
native `tests/unix_lifecycle.rs` regressions on Linux and macOS. Cross-target
compilation alone does not verify those operating-system behaviors.
