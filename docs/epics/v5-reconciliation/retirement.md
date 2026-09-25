# Retirement and relocation register

No production deletions were performed by this audit. These are concrete proposed dispositions. A deletion must include its consumers, tests, exports, manifests, release scripts and data compatibility implications. Test-only reachability is evidence for review, not proof of universal dead code.

## Implementations that should die after replacement

| ID | Current code/behavior | Why it should go | Replacement / prerequisite | Deletion proof |
|---|---|---|---|---|
| D01 | `tetonic-app/src/fleet_api.rs`: in-memory authoritative org/squad/agent maps and REST-shaped dispatcher | Disconnected lifecycle, volatile records, no transport auth | Durable control services + authenticated routes | Restart retains resources; create reaches real attempt; no production imports of old manager |
| D02 | `create_agent`: fixed 1,000-token creation charge and unconditional Running result | Invented usage and false observed state | Broker reservations/settlement and worker-derived state | Duplicate create has no charge; metadata-only creation costs no inference tokens; Running requires claim |
| D03 | `tetonic-orchestrator/src/fleet.rs`: AtomicU64 hourly accounting and automatic in-memory squad workpad | Not a durable accounting window or authorized shared memory | Tenant budget ledger; explicit scoped memory capability | Window/denial/concurrent-admission tests; workpad access and retention tests |
| D04 | `fleet_supervisor.rs` as independent production lifecycle authority | Competes with run/attempt state; stored handles may be absent | Controller projections over managed execution | Stop/steer reach actual work; status cannot be changed by map mutation |
| D05 | `operator_control.rs` as separate supervisor command path | Dashboard/steering not joined to managed execution | Authorized durable commands and projections | Requested/accepted/effective distinctions; no bypass route |
| D06 | `tetonic-node/src/role.rs`: standalone KeeperRegistry/RunnerClient authority | Process-local registry/epochs; no production lifecycle caller found | Durable assignment/membership service integrated with leases | Restart generations persist; stale worker cannot commit or obtain new mediated effects |
| D07 | `tetonic-server/src/main.rs`: direct world-agent composition and manual HTTP handler | Parallel platform bypass; special-purpose server | Common server bootstrap + world harness + real API framework | Village parity through API; no direct unmanaged production launch |
| D08 | `Agent::run_in_world` as an alternate core-owned lifecycle | Direct effect dispatch bypasses common action broker | Managed world harness using capability gateway | All world actions have authorization and outcome records; idle cancellation works |
| D09 | Duplicate server/node configuration parsers and unsupported storage-mode promises | Divergent schemas; enum values imply unimplemented product shapes | One validated schema + import/migration | Unsupported modes fail; imported experiment config works |
| D10 | ThoughtStreamHub/TraceStore as independent lifecycle truth | Volatile parallel events and incomplete correlation | Unified event envelope, durable transitions, bounded live projections | Resume cursor/retention/redaction/isolation tests; authoritative state survives stream loss |
| D11 | Universal fixed `id_coding_production` identity | Definition recipe confused with an individual actor | Per-agent identity + reusable coding definition revision | Two agents same definition maintain distinct state/grants/history |
| D12 | Independent CLI/daemon bootstrap and legacy product naming in supported entrypoints | Multiple composition roots drift | Client/stdio compatibility over common services; delivery migration | Existing supported commands pass parity; release artifacts cover new server/client |

D03 does not prohibit collaborative memory. It removes an unbounded, in-process workpad as the implicit collaboration model. D10 does not remove live streaming: rings/broadcast are useful delivery mechanisms behind a common event contract.

## Remove from the platform core; preserve as optional product capabilities

| Code | Destination | Reason |
|---|---|---|
| `tetonic-app/definition.rs`, `coding_pack.rs`, verify/commit finalization rules | Coding harness/pack | Role prompts and software-edit completion rules should not govern every agent |
| Orchestrator router/critic/specialist/briefing policies | Coding or explicitly selected planning harness | A strategy choice, not the cluster scheduler |
| `tetonic-tools`, `tetonic-lsp`, `tetonic-index`, repository-oriented context code | Coding capabilities | Useful customer functionality, inappropriate mandatory platform dependency |
| Workspace transaction engine | Workspace mutation capability | Preserve safe file mutation without requiring every agent to have a repository |
| Village-specific prompts, experiences/config examples and game assumptions | Village integration assets or external example | Tetonic supplies runtime contracts, Village supplies world semantics |
| Inference capacity optimization and model-routing strategies | Optional inference subsystem | Existing useful work; not required to define agent lifecycle |

Do not move generic perception/event contracts to the Village solely because the experiment first used them. Classify by semantic responsibility and reuse, not original sprint name.

## First deletion candidates after a focused consumer check

- `DualProcessBrain`: urgency-based dual-brain strategy has test-only construction in inspected searches. Remove from the default runtime exports or move to an example if no committed workload needs it. Preserve SingleModelBrain while the world harness uses it.
- `ScriptedBrain`: move to test support if all uses are test fixtures; check external/library compatibility before hiding exports.
- Composite/stream world adapter variants: require a live integration owner; retain contract tests and one supported transport, remove redundant variants only after confirming no supported client depends on their protocol.
- Unimplemented configuration alternatives: reject/stop advertising now; remove schema variants with a versioned config migration. They are not working backends to preserve.

## Explicitly not approved for blind deletion

- `legacy*` fabric modules: RemoteNodeProvider is on a live inference path. Migrate callers and peer versions first.
- Run replay/migration/backups: historical data remains valuable even if product entrypoints change.
- Lease, attestation, result-integrity, side-effect and cancellation tests: these protect the future product goal.
- Sandbox, egress, secret scanning and approval checks: disconnected world code should adopt them, not remove them for convenience.
- Coding corpus/evaluations: retain for the coding harness; add infrastructure-focused scenarios rather than replacing all tests.
- Architecture gates: change obsolete rules with new boundary checks; do not disable gates wholesale.

## Removal procedure

For each D item: capture callers and dependency edges; identify durable/wire data; implement replacement and negative tests; switch supported callers; run focused suites and architecture gate; remove obsolete implementation/exports/tests that assert only its existence; update packaging/docs; inspect remaining references. Commit replacement and removal in reviewable stages. Never delete user data to make a migration pass.
