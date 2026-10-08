# tetonic-orchestrator

Retained coding and session strategy library: routing, specialist selection,
critic passes, briefing and bounded child-agent strategies.

Despite its broad name, this crate is **not the current product's team-work
controller**. Product coordination lives in
[`tetonic-app/work`](../../litho/tetonic-app/src/work/mod.rs) and
[`team_work_controller`](../../litho/tetonic-app/src/team_work_controller.rs).
Managed attempt lifetime and accepted outcomes belong to
[`tetonic-run`](../tetonic-run/src/managed/mod.rs).

The application's [`coding_pack`](../../litho/tetonic-app/src/coding_pack.rs)
retains integration with these strategies. General registered-agent execution
uses [`resources/registered`](../../litho/tetonic-app/src/resources/registered/mod.rs).
Do not build a second product scheduler or lifecycle authority here.

## Retained modules

| Modules | Responsibility |
|---|---|
| `host`, `briefing` | Coding session setup and project context |
| `router`, `router_llm` | Routing strategies and model-assisted routing |
| `specialist`, `critic`, `turn` | Role/tool subsets, coding review and routed-turn strategies |
| `spawn_host`, `spawn_budget`, `spawn_session`, `spawn` | Child-agent integration, budget contracts and spawn bookkeeping |
| `handoff`, `run` | Compact handoff data, limits and turn tracking |

Library exports and tests are not a promise that a corresponding CLI flag,
HTTP endpoint or product feature is available. The retired daemon/chat commands
in older documents are not current entry points. Consult the current
[architecture overview](../../../docs/architecture/README.md) and
[execution boundaries](../../../docs/architecture/execution-boundaries.md).

Run library tests from `engine` with `cargo test -p tetonic-orchestrator`.
