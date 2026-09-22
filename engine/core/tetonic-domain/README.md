# lokai-domain

Stable **value objects** and **execution-contract types** shared across policy, core, runtime, inference, and persistence boundaries (AC2 / M5-3).

**In scope:** identities, data class, disclosure tier, policy verdict, proposed
actions, capabilities, placement/trust requests and decisions, sink trait
sketches, **secret-scan contract** (`SecretScanner`, `OutboundRedaction`).

**Out of scope:** policy rules, transport wire types, SQLite row shapes, Ollama-specific types.

**Product IDs:** AC2-0 (Done) · M5-3 worker trust and placement types (Done) ·
M5-4 result disposition / verification requirement / behavior signals (Done)

M5-3 `PlacementRequest` carries run/task/attempt identity, job kind, complete
payload classification, digest-bound input artifacts, workspace version,
required capabilities, sandbox and verification requirements, coordinator
policy epoch, candidate worker, project policy, and trace context.

M5-4 adds `ResultDisposition`, `ResultVerificationRequirement`,
`WorkerOperationalState`, `WorkerBehaviorSignals`, and `ArtifactOrigin` for
signed-result acceptance (distinct from placement `VerificationRequirement`).

H1-1 moves `SecretScanner` here so `lokai-secrets` does not depend on
`lokai-context`. `BrokerInferenceProvider` scans Infer content against this
trait before dispatch.

**Test:** `cargo test -p lokai-domain`
