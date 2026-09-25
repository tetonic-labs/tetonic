# Security regression fixtures (AR2-6 / SEC2-E2-031)

Offline fixtures for CI and manual audits. No network required.

| Path | Exercises |
|------|-----------|
| `malicious-repo/README.md` | Prompt injection in repo content — briefing must delimit, not execute |
| `malicious-repo/.lokai/project.md` | Repo-provided project memory — must be labeled untrusted |
| `malicious-repo/verify_evil.py` | Hostile verify script — must fail argv allowlist / approval |
| `malicious-repo/trap_link` | Created at test time — symlink escape must be blocked on read |

Run targeted tests:

```bash
cargo test -p lokai-orchestrator security_fixture
cargo test -p lokai-orchestrator briefing_delimits
cargo test -p lokai-tools split_verify_rejects
cargo test -p lokai-memory zstd_rejects
cargo test -p lokai-inference tls13_rejects
cargo test -p tetonicd strict_rpc
```
