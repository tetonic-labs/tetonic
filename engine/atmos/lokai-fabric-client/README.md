# lokai-fabric-client

Coordinator-side pinned-mTLS client for worker fabric control traffic and Infer
transport (`/v1/jobs` typed primary, `/v1/chat` legacy).

## R7-1 typed Infer transport

`RemoteNodeProvider` caches probed `WorkerCapabilityAdvertisement` (starts
legacy until first successful `/v1/capabilities` probe). Non-legacy workers use
`POST /v1/jobs` with continuous `/v1/jobs/lease` renewals (`LOKAI_FABRIC_LEASE_TTL_MS`) and fail closed on 404/501/405
(no chat fallback). Probe POSTs `/v1/negotiate` first (R7-3); agreed version is
stored on the connection and exposed on `NodeInfo.negotiated_protocol_version`.
Infer fails closed until negotiation succeeds.

## R7-2 capability probes

`probe_node` (production refresh) GETs `/v1/capabilities` then `/v1/models/verified`,
compares advertised inventory names to verified Ollama tags, upserts
`CapabilityRegistry` with `models_verified` + `verified_model_names` (mismatch
quarantines). Tool-bearing Infer sets `network_deny_all` on the job so placement
skips workers whose sandbox `network_denial` is not Enforced.

## M5-3 placement boundary

`RemoteNodeProvider::chat_on_fabric` is the lowest production job-send
boundary. It builds a typed `JobEnvelope`, requires a live dispatch guard and
fresh capability registry, runs typed trust/capability/model/limit placement,
validates the protocol offer, then re-captures workspace state and checks every
input artifact digest against the coordinator artifact store before writing to
the network.

Lease renewal requires the original job and reruns typed placement against the
current trust, policy epoch, and capability state.

## M5-4 result integrity boundary

Every successful remote chat result must include a signed `ResultEnvelope`.
Before the coordinator uses the payload:

1. Channel worker identity must match the envelope
2. Result-signing key is registered/looked up (unknown/revoked keys fail closed)
3. Live `ActiveJobRegistry` attempt flags bind cancel/supersede/winner state
4. Optional `RemoteResultRunBridge` loads RunSupervisor snapshot + lease proof for the **same** attempt id as the job (unleased ids reject when a bridge is bound)
5. Envelope validation binds job/task/attempt/lease/input/workspace digests; revocation epoch is compared to the **coordinator** `policy_epoch`, not the envelope field copied onto itself
6. Artifacts enter quarantine (`Sealed + Unverified + RemoteOrigin`)
7. Verification policy runs (structural / local / redundant)
8. `ActiveJobRegistry::validate` (AJR settle) runs **before** durable `Accepted`; settle failure persists `RejectedStaleAttempt`, not Accepted
9. Dispositions persist durably (`StoreDispositionPersist`) with audit/trace
10. `CompleteAttempt` applies only through RunSupervisor when a live bridge/proof exists
11. Behavior-signal violations degrade or quarantine future scheduling eligibility
12. NDJSON `/v1/chat` and `/v1/jobs` buffer token deltas and do not invoke `on_token` until signed accept succeeds (`deliver_stream_after_accept`)

Signatures prove provenance and tamper detection only — never correctness.
Remote payloads cannot issue capabilities, invoke tools/processes, or mutate the
workspace. Fabric does not expose a public remote-patch commit door.

Remote typed embed, compute, artifact-transfer, cache-upload, and diagnostic
transports do not exist yet. Non-Infer `JobKind` values remain on the wire enum
for extensibility but are **cancelled for V2** — workers refuse them with
`UnsupportedJobKind` until the owner reopens that work (see
`fabric-compatibility-matrix.md`).

## Dependencies

Uses `lokai-fabric-protocol`, `lokai-inference`, `lokai-domain`, `lokai-run`,
`lokai-artifact`, `lokai-transaction`, `lokai-memory`, `lokai-egress`, and
`lokai-enroll`.

## Tests

`cargo test -p lokai-fabric-client` covers protocol adaptation, lifecycle
gates, result identity, signed-result acceptance (coordinator epoch, unleased
reject, AJR settle-before-Accepted, stream tokens withheld on failed accept),
quarantine adversarial cases (including workspace-version mismatch and
sensitive-task malice), patch commit pipeline, trust downgrade during lease
renewal, worker revocation, dispatch-time workspace/artifact revalidation,
and R7-1 endpoint selection / fail-closed helpers, R7-2 verified-inventory
probe paths, and R7-3 negotiate record / Infer-without-handshake refusal.

Product-plan status: M5-1–M5-4 Done; R7-1/R7-2/R7-3 Done.
