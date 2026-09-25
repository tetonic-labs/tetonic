# Reuse, reshape and removal for the team MVP

This is a product-aligned disposition, not permission for blind deletion. See the [existing D01–D12 register](retirement.md) for source references and migration gates. The original mechanical inventory remains a baseline census, not current per-file semantic proof.

| Existing asset | Decision | New value / required correction |
|---|---|---|
| tetonic-run supervisor, managed service, claims, replay, attestation | Preserve and generalize | One execution authority; inject harness executors while retaining result/effect/cancellation invariants |
| Domain identity/job spec and identity store | Evolve | Stable agents, immutable admitted revisions, trusted org scope; avoid overwriting definitions of admitted work |
| IntentCharter | Reuse intent fields; replace implied enforcement | Goal/success/constraint representation; compile hard limits into real policy and budget services |
| Run DAG helpers | Reuse for task dependencies | Huddle plans can create dynamic work; durable team backlog must survive individual runs rather than keeping one immortal run |
| SpawnBudgetGate and app budget adapter | Generalize | Child admission and release with team/goal/origin attribution; no reset through peer requests |
| WorkScope and manager quiescence | Preserve | Local admission closure and effect lifetime; add persisted scope-wide stop generations and process-tree control |
| Policy/action broker, egress, sandbox, secrets | Integrate and extend | Worker-local enforcement plus destination checks; explicit OS/transport gaps; no trust in a role prompt |
| Broker/provider/fabric | Preserve live inference path | Shared inference accounting and approved endpoints; inference worker stays distinct from agent execution worker |
| Artifact/context/memory primitives | Reuse under scoped interfaces | Private/team knowledge retrieval, provenance and explicit publication; isolate contexts before model invocation |
| ExperienceMemory | Keep provenance principle, replace volatile authority | Source-tagged evidence helps; incoming JSON scope keys and bounded in-process evidence are not tenant authorization or durable memory |
| FleetManager, FleetSupervisor, OperatorController prototypes | Replace, then delete | Persist teams/resources; drive real lifecycle; operator commands target actual managed scopes |
| KeeperRegistry/RunnerClient prototype | Replace, then delete | Durable assignment ownership integrated with existing leases; no restart-reset authority |
| ThoughtStreamHub / experiment TraceStore | Consolidate schemas; retain useful bounded streaming | One inspectable product with committed lifecycle truth and separate high-volume telemetry |
| Coding prompts, critic/router, LSP/index, workspace transactions | Extract as optional harness/capabilities | Keep valuable coding ability without dictating how every team operates |
| Special Village server bootstrap | Replace, then delete | External integration through the same managed harness and effect boundary; keep game logic in Village |
| CLI/stdio/config shells | Adapt then converge | Existing users retain a migration path; supported server/client install replaces duplicate composition |
| Experimental dual-brain and adapter variants | Optionalize or retire after consumer check | No ongoing default dependency without a required workload; different transports are not automatically redundant |

## Additional source checks during MVP planning

- [charter.rs](../../../engine/core/tetonic-domain/src/charter.rs): `evaluate_action` handles forbidden verbs and namespace checks, but PathFilter and ResourceCap fall through; a verb without a dot also avoids the namespace branch. Treat the charter as declarative intent until compiled into enforced policy. Do not reuse its name/comment as security evidence.
- [dag.rs](../../../engine/mantle/tetonic-run/src/dag.rs): dependency cycle and readiness helpers are useful foundations, not an implemented persistent team scheduler.
- [spawn_budget.rs](../../../engine/litho/tetonic-app/src/spawn_budget.rs): existing child reservations are tied to run/session accounting. Extend attribution across requests to existing agents as well as new children.
- [work_scope.rs](../../../engine/core/tetonic-domain/src/work_scope.rs): cancellation closes admission and tracks quiescence in-process; it is not a distributed kill signal or OS process-group terminator.
- [experience.rs](../../../engine/mantle/tetonic-server/src/experience.rs): provenance-tagged world evidence is bounded and volatile. Preserve the distinction between statements and observations, not its assumption that caller-supplied scope values establish authorization.

## Removals added to the original register

| ID | Remove or change | Gate |
|---|---|---|
| D13 | Any claim that IntentCharter alone enforces path/resource boundaries | Replace with typed enforcement decisions; denial tests at actual effect/admission boundary |
| D14 | Session-bound delegation as sole source of budget/stop lineage | Persist goal/team/origin lineage; children and peer requests cannot escape budget or stop scope |
| D15 | Any single agent-wide context automatically reused across personal and team work | Scoped execution contexts, histories, caches and retrieval; explicit publication only; privacy tests |

D14/D15 are migration prohibitions and design corrections, not findings that every current code path exhibits the behavior. Inventory their concrete consumers before deletion.

## Removal order

1. Immediately correct unsupported behavior/claims and freeze new integrations with prototype fleet/Keeper authorities. Do not remove a working product entrypoint before replacement.
2. Persist identity/team resources; route all new work through managed execution and scoped contexts. D02/D11 retire early.
3. Replace fleet/operator consumers together; close D01/D03/D04/D05 only when their tests and UI/service callers use the new path.
4. Move world and coding composition behind supported harness/capability contracts; close D07/D08/D10 without losing diagnostics or world controls.
5. Integrate remote ownership; close D06. Keep live legacy inference transport until supported peers migrate.
6. Consolidate config, packaging and compatible client surfaces; close D09/D12. Delete obsolete architecture assertions only when replacement behavior tests exist.

Each implementation ticket records destination, live callers, compatibility, replacement evidence and removal commit. A feature flag must not leave two authorities writing the same state. Net lines removed is not the goal; fewer independent owners and bypass paths is.
