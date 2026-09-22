# Fabric compatibility matrix (release artifact)

**Relates to:** [`fabric-transport-v1.md`](./fabric-transport-v1.md), R7-3  
**Maintained with:** fabric protocol / coordinator / worker releases

Independently versioned schemas are intentional. Compatibility is established at
fabric handshake via `POST /v1/negotiate` (`negotiate_versions` in
`lokai-fabric-protocol`), not by assuming equal crate versions.

## Wire protocol

| Constant | Location | Current |
|----------|----------|--------:|
| `PROTOCOL_VERSION` | `lokai-fabric-protocol` `bounds.rs` | 1 |
| `MIN_SUPPORTED_VERSION` | same | 1 |
| `MAX_SUPPORTED_VERSION` | same | 1 |

Mandatory negotiation features (`IMMUTABLE_SECURITY_FEATURES`):
`task_identity`, `lease_epoch`, `input_digest`, `revocation_epoch`.

Peers with no overlapping version range, or that omit a mandatory feature, fail
closed with `UnsupportedProtocolVersion` / `InvalidEnvelope` at negotiate time.
Infer is refused until a version is recorded on the connection
(`RemoteNodeProvider::negotiated_protocol_version`).

## Coordinator × worker (fabric v1)

| Coordinator fabric | Worker fabric | Result |
|--------------------|---------------|--------|
| min=1 max=1 + immutable features | min=1 max=1 + immutable features | Negotiate `1`; Infer allowed after probe |
| min/max outside peer range | any | Handshake refused; node unhealthy; no Infer |
| missing immutable feature | any | Handshake refused |

## Persistence schemas (orthogonal)

These migrate independently of fabric protocol version. Do not treat equality
with `PROTOCOL_VERSION` as required.

| Store | Current schema | Notes |
|-------|---------------:|-------|
| lokai.db | **v25** | `lokai-memory` `schema.rs` (`migrate_secret_overrides_v25`) |
| `worker.db` | **v6** | `lokai-memory` `worker_store.rs` |

## Related non-fabric versions

| Constant | Crate | Current |
|----------|-------|--------:|
| RPC `protocol_version` | `lokai-rpc` | 1 (editor/daemon stdio) |
| `CANONICAL_SCHEMA_VERSION` | `lokai-domain` | 1 |
| Capacity `SCHEMA_VERSION` | `lokai-capacity` | 1 |

## JobKind support (worker ingress)

| `JobKind` | Worker executor |
|-----------|-----------------|
| `Infer` | Implemented (`/v1/jobs`) |
| `Embed`, `AnalyzeCode`, `IndexShard`, `TestShard`, `ReviewArtifact` | Explicit `UnsupportedJobKind` at ingress |

**V2 policy:** Non-Infer job kinds are **cancelled** until the owner explicitly reopens them. The enum stays for extensibility; workers must keep fail-closed refuse (not silent fallthrough). Do not implement executors for these kinds in V2 remainder work.

## Release checklist

When bumping fabric `MIN`/`MAX` or shipping a new coordinator/worker:

1. Update this matrix row for the new version pair.
2. Confirm `negotiate_versions` conformance tests cover the skew case.
3. Confirm probe records `negotiated_protocol_version` on `NodeInfo`.
4. Note any `lokai.db` / `worker.db` migration that must ship together.
