# Tetonic documentation

For the system that runs today, start with the
[current architecture](architecture/README.md) and
[contribution guide](../CONTRIBUTING.md). For running the local product, use the
[repository quick start](../README.md#try-the-local-preview).

| Need | Read |
|---|---|
| Where to make a change | [Ownership map and all 28 engine packages](architecture/ownership.md) |
| Meaning of an agent, team, work item or attempt | [Domain terminology](architecture/terminology.md) |
| Startup, storage and diagnostics | [Host configuration](architecture/host-configuration.md) |
| Work and agent use cases | [Scoped application services](architecture/work-services.md) |
| Run lifecycle, harnesses, tools and inference | [Execution boundaries](architecture/execution-boundaries.md) |
| Authoritative state and public payloads | [Durable state and contracts](architecture/durable-state-and-contracts.md) |
| UI integration without duplicate state | [Frontend boundaries](architecture/frontend-boundaries.md) |
| Local API behavior | [Local UI v1 contract](implementation/contracts/local-ui-v1.md) |
| Required checks and documented exceptions | [Quality gate](engineering/QUALITY-GATE.md), [quality debt](engineering/QUALITY-DEBT.md) |

`docs/epics/` contains plans and dated delivery evidence. A completed ticket is
evidence of its stated scope, not proof that an entire product milestone shipped.
The [architecture tidy-up record](epics/v5-reconciliation/sprints/architecture-baseline/README.md)
links its seven deliveries.

Older architecture audits, V1–V4 migration documents and Village proposals are
historical material. Read their dates and the
[retirement record](epics/v5-reconciliation/retirement.md) before using an old
path or command. The current architecture index and source code take precedence
when those documents disagree with today's implementation.
