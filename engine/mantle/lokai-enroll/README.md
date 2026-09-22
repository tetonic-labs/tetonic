# lokai-enroll

Worker enrollment handshake: one-time code + shared-secret HMAC proof + Ed25519 request signatures + pinned TLS + mTLS fabric identity per [node-enrollment-v1](../../../docs/implementation/contracts/node-enrollment-v1.md).

## Role in the stack

Coordinator runs `run_enrollment_server` during `lokaid --node --enroll`. Worker completes via `lokai estate enroll`. Successful enrollment adds egress allow rules and persists TLS cert to `lokai-memory`.

## Key API

- `EnrollmentCode`, `encode_enrollment_code`, `decode_enrollment_code`
- `KeyPair`, `complete_enrollment`
- `run_enrollment_server`, `EnrollmentServerConfig`
- `allow_fabric_workers` — sync enrolled workers into `EgressGuard`
- `advertise_host` — `LOKAI_ADVERTISE_HOST` or `127.0.0.1` (binaries only)
- Ports: enrollment **9470**, fabric **9471** (`DEFAULT_FABRIC_PORT`, canonical in this crate)

## Dependencies

- `lokai-egress` — enrollment HTTP/HTTPS via guarded client (`post_json` / `post_json_pinned`)

## Tests

`cargo test -p lokai-enroll` — 25 tests (code, crypto, HMAC, signatures, HTTP, TLS e2e via `EgressGuard`, server race/bad-proof).

## Related docs

- [node-enrollment-v1](../../../docs/implementation/contracts/node-enrollment-v1.md)
- [estate-v1](../../../docs/implementation/contracts/estate-v1.md)

## Security note

Enrollment uses HTTPS with the worker fabric cert pinned in the code when TLS identity is available. Plain HTTP is retained for unit tests without a cert. Fabric listeners use `lokai_node::resolve_listen_host()` — default **`127.0.0.1`**.
