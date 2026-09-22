# Platform enforcement matrix (M2-3 / H1-3)

| Control | Windows | Linux | macOS |
|---------|---------|-------|-------|
| Process-tree containment | Job Objects | process group + kill | process group + kill |
| Child breakaway prevention | Job (no breakaway flag) | Partial — `BrokeredWithWarning` | Partial — `BrokeredWithWarning` |
| Runtime enforcement | Yes | Yes | Yes |
| Memory limits | Job memory limit | `RLIMIT_AS` | `RLIMIT_AS` |
| CPU limits | No | No | No |
| Process-count limits | Job active process | No — `BrokeredWithWarning` | No — `BrokeredWithWarning` |
| Output limits | Yes | Yes | Yes |
| Environment filtering | Yes | Yes | Yes |
| Filesystem read restrictions | Broker validation only | **Yes via Landlock LSM** (Linux 5.13+; otherwise `BrokeredWithWarning`) | **Yes via Seatbelt FS** (`(deny file-read* ...)` on secrets; otherwise `BrokeredWithWarning`) |
| Filesystem write restrictions | Broker validation only | **Yes via Landlock LSM** (Linux 5.13+; otherwise `BrokeredWithWarning`) | **Yes via Seatbelt FS** (`(deny file-write*)` + workspace allow; otherwise `BrokeredWithWarning`) |
| Network denial | **Yes when elevated** — Firewall outbound block per child exe (`New-NetFirewallRule`); otherwise High gap / Standard warn path | **Yes** — unprivileged user+net namespace | **Yes** — `sandbox-exec` Seatbelt `(deny network*)` |
| Network allowlisting | Not implemented | Not implemented | Not implemented |
| Long-lived process support | Yes | Yes | Yes |
| Interactive stdin | Pipe (inheritable ends) | Pipe | Pipe |
| Capability `platform` string | `windows` | `linux` | `macos` (dedicated backend; not the Linux backend) |

**H1-3 contract:** Windows, Linux, and macOS are equal first-class targets. `network_denial: true` only when OS-enforced. Shell/verify profiles use `IsolationLevel::Strict` **when** denial is available; otherwise they stay `Standard` so H1-2 informed consent remains usable on non-elevated Windows desktops.

## Process profiles

| Profile | Network default | Isolation | Typical use |
|---------|-----------------|-----------|-------------|
| InternalService | DenyAll | Standard | LSP long-lived |
| RepositoryTool | DenyAll | Standard | git, grep |
| BuildVerification | DenyAll | Strict iff OS network denial | cargo test, verify scripts |
| ModelRequestedShell | DenyAll | Strict iff OS network denial | `run_shell` |
| HardwareProbe | DenyAll | Standard | capacity probes |

## Unsupported controls (documented)

- OS-level filesystem sandbox on Windows/macOS (AppContainer / Seatbelt FS) — in progress (R21/R23); Linux uses Landlock LSM when supported by kernel.
- Network allowlisting (`AllowDestinations`) — not implemented; High gap if requested
- CPU throttling — not implemented on any platform
- Windows network denial without elevation — not available; capability stays false (enforce-or-refuse via Standard + H1-2 prompt, not a silent Sandboxed claim)

When required controls are missing, the backend returns **BrokeredWithWarning** or **Denied** (Strict + High), never silent **Sandboxed** for High network gaps.
