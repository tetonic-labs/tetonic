# lokai-artifact

Local sealed artifact store, quarantine validation, garbage collection / quota, and thin provenance (M3-3 / R4-2 / R24).

## Role

`LocalArtifactStore` backs turn attestation (`AcceptArtifact`) and remote fabric quarantine. Content is immutable after seal; digests are hex-encoded SHA-256 of sealed bytes. Construction requires a `ScanPolicy` (`Scan` or `Refuse` — there is no silent-skip variant). `seal` scans on the write path; `open` verifies the seal digest before returning a reader (M6).

Each seal has an opaque, unique occurrence ID; the digest identifies content.
Equal content from different producers has separate metadata and object files, so
pins, classification and acceptance cannot overwrite each other. Existing digest-
based IDs remain readable. This deliberately trades deduplication for simple,
independent ownership; shared blob storage requires a separate reference design.

After `AcceptArtifact`, the app calls `mark_accepted` so lifecycle is `Accepted`. `reconstruct_provenance` / `ArtifactProvenanceBundle` returns producer attempt, digests, data class, verification state, and a trust label (`LocalSealed` | `RemoteUnverified` | `Accepted`) without a CAS graph.

## Thin provenance (R24)

| API | Behavior |
|-----|----------|
| `mark_accepted` | Sealed → Accepted (local Unverified → LocallyVerified) |
| `reconstruct_provenance` | Bundle from stored metadata; missing id → `NotFound` |
| `persist_quarantined_remote` | Validate ID, digest and quota; reject existing IDs; force Sealed / Unverified |
| `provenance_trust_label` | Remote quarantined stays `RemoteUnverified` until Accepted |

Full distributed CAS / cross-coordinator dedupe remains out of scope (epic 8).

## GC and quota (R4-2)

| API | Behavior |
|-----|----------|
| `enforce_at_startup` | Cleans abandoned `tmp/` writes; preserves sealed artifacts; logs quota pressure |
| `collect_garbage` / `ArtifactGcRoots` | Requires explicit completed-run evidence and no active reference before deleting Ephemeral / UntilRunCompletes |
| `ensure_quota_for_write` | Checks physical object usage + incoming against budget without running GC |
| `ArtifactGcConfig` | Default 1 GiB; override with `LOKAI_ARTIFACT_QUOTA_BYTES` |

Production caller: `DefaultInitializationService::bootstrap_runtime` (shared by CLI and `lokaid` initialize).

`ProjectHistory` / `SecurityAudit` / `UserPinned` are never auto-deleted.
A missing liveness inventory authorizes no sealed-object deletion. A future run/
recovery owner must supply a complete reference inventory and coordinate it with
lifecycle changes; startup does not yet perform automatic completed-run reclamation.
Sealed data can accumulate to quota, at which point new writes fail closed.

An OS file lock serializes publication, acceptance, deletion and quota checks
across cooperating processes. A separate lease on each temporary write lets GC
skip live writers and reclaim writes abandoned after process exit. Synchronous GC
returns a lock-contention error rather than waiting; async operations retry for up
to five seconds. Interrupted object publications count toward quota even without
metadata and are retained for explicit repair. These locks require cooperating
versions of the application and a filesystem that supports OS file locking.

## Product plan

| ID | Status |
|----|--------|
| M3-3 Artifact lifecycle | **Complete** for thin provenance (R24); CAS / full provenance graph still out of scope |
| R4-2 Seal + GC/quota | Quota and conservative startup cleanup wired; lifecycle-authorized automatic reclamation pending |
| R24 Thin provenance | **Done** |

## Tests

```bash
cargo test -p lokai-artifact
cargo test -p lokai-app turn_attestation
```
