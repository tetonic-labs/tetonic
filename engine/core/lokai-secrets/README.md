# lokai-secrets

Secret detection and redaction for outbound Infer, tools, telemetry, and context seal.

## Role

`ScannerEngine` implements `SecretScanner`. Production paths hydrate a stable HMAC key and
durable fingerprint overrides from `lokai.db` (R12). Scoped overrides (`global` / `session` /
`project`) only apply when the context supplied to that scan matches.

Pass `ScanContext` through `SecretScanner::scan_and_redact_in_context` (or the
synchronous scoped API). There is no shared scope setter. Unscoped scans use only
global exceptions. Project scope takes precedence when both identifiers are supplied.
Each scan snapshots its permissions; grants/revocations affect subsequent scans.
Cache keys include the effective permission snapshot, so an older in-flight scan
cannot repopulate an authorization result reusable after revocation.

## Key API

- `ScannerEngine::with_hmac_key`, `grant_override`, `revoke_override`, `hydrate_overrides`, `scan_and_redact_scoped_sync` (immutable per-call context)
- `allow_fingerprint` — global in-memory grant (back-compat)
- `OverrideScope` — scope tags for grants
- `shared_scanner` — process-wide engine for formatters without an `Arc` (not store-hydrated)

## Product plan

| ID | Status |
|----|--------|
| M4-2 Secrets / redaction | **Partial** (FP/FN product gates open; R11 digests Done; R12 overrides Done) |
| R12 Durable scoped overrides | **Done** |

## Tests

```bash
cargo test -p lokai-secrets
cargo test -p lokai-memory -- secret_override
cargo test -p lokai-app -- durable_override
```
