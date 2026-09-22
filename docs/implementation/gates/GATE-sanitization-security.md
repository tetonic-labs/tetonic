# GATE — Sanitization & prompt security

**Status:** Draft (E6 — runs with privacy gate on phases touching tools, MCP, or remote inference).  
**Complements:** [`GATE-privacy-sovereignty.md`](./GATE-privacy-sovereignty.md) (network boundary) — this gate covers **in-process** injection, exfiltration via tools, and unsafe model output handling.

**Charter mapping:** Right 5 (Consent), Right 1 (Sanctuary — indirect exfil via tools).

---

## 1. Tool argument validation

- [ ] Every tool call validates args against **JSON Schema** from `coding-tools-v1` before execution.
- [ ] Path tools resolve under **workspace root**; `..` and symlink escape attempts denied (T11).
- [ ] `run_shell` never executes without approval (daemon) or explicit flag (CLI); command string logged.

**Fail:** schema bypass; path traversal; silent shell execution.

## 2. Prompt / tool injection resistance

- [ ] User content and tool results are **delimited** in message assembly (no raw concatenation that blurs role boundaries).
- [ ] System prompt and tool schemas live in **stable prefix**; retrieved chunks marked as untrusted context.
- [ ] Constrained decoding (D7) enforced for structured tool calls when provider supports `format` grammar.

**Fail:** model can invoke tools via crafted file content without user intent; unconstrained JSON tool calls on supported backends.

## 3. Remote inference honesty (Circle / estate)

- [ ] GPU worker receives the **same prompt slice** the coordinator logged for the job id (honesty clause — V*).
- [ ] `data_class: private` jobs **never** appear in remote `FabricJob` (policy + CI negative test).
- [ ] Disclosure tier ≥ `auditable` requires audit envelope when peer agreement demands it.

**Fail:** coordinator sends different content than disclosed; private class routed remotely on full policy mode.

## 4. Output handling

- [ ] Diffs applied through validated edit paths only (`edit_file` / `write_file`), not raw shell redirects from model text.
- [ ] No automatic execution of model-generated shell without approval gate.

**Fail:** auto-run of model-suggested destructive commands.

## 5. Extension / MCP surface (when shipped)

- [ ] Third-party tools registered only through D1 policy; default deny.
- [ ] Extension host cannot open network sockets (delegated to egress guard).

**Fail:** unvetted extension exfil path.

---

## Phase applicability

| Phase | Minimum bar |
|-------|-------------|
| A | §1–2 partial (schema + paths; D7 pending) |
| B.1 | §2 delimiter tests in agent loop |
| N2+ Circle | §3 required |
| T10 MCP | §5 required |

---

**Last updated:** 2026-06-27 (E6 initial spec)
