# Configurable workspace execution limits

Date: October 8, 2026. Readiness prerequisite for OCT-202; OCT-203 remains open.

## Problem and delivered behavior

The UI host previously capped ordinary agents at 120 seconds and four steps
without a workspace folder, or eight with one, regardless of the inference
provider. Plan readiness separately limited coordination to 4096 tokens and
coordinator definitions to 16 steps. The agent editor correctly reflected these
ceilings, so changing a saved agent could not overcome the fixed host settings.

The existing `HostConfiguration` now accepts `workspace_execution` with worker
step/time/reported-token ceilings and coordination step/token ceilings. Startup
validates them before opening storage. Omitted settings preserve old defaults.
The [example configuration](../../../../architecture/examples/workspace-execution.json)
permits 32 worker steps, 900 seconds, 32000 reported tokens and 24 coordination
steps with up to 10000 coordination tokens inside the plan total.

The existing catalog and editor show worker ceilings. Saved agents retain their
own budgets, identity, model and grants across host restarts and increases.
Agents exceeding a reduced host ceiling cannot start new work until adjusted;
their definitions are not silently clamped. The Guide's observation and explicit
brief-to-plan prompt both use the current coordination allowance. Continuation
proposals preserve a prior coordination allocation within today's ceiling.

## Integration and scope

- Configuration: `tetonic-app::host::WorkspaceExecutionConfiguration`.
- Bootstrap: existing `local_workspace/bootstrap.rs` supplies registered host
  settings and retains coordination policy in scoped `WorkspaceServices`.
- Enforcement: existing agent limit validation, registered preparation and managed
  execution. Existing workspace allowances and plan budget reservations still apply.
- Plans: current readiness, pinned coordinator definition and continuation proposal
  paths consume the configured allowance. No second scheduler or accounting system.
- UI: existing `AgentCreateForm` / `AgentAdvancedSettings` catalog-driven limits;
  no layout or interaction changes.

Operator ceilings are not grants, a monetary cap, model context sizes or proof a
model can perform useful work. Per-job explicit settings are unchanged. No live
workspace was restarted or reconfigured and no paid inference was used.

## Validation

- Configuration defaults, partial overrides, invalid bounds and authority-field
  rejection are covered.
- The documented example drives tests of catalog values, saved-agent preparation,
  restart persistence and rejection after host ceiling reduction without inference.
- A real managed team runs against a local scripted provider with an 8000-token
  coordination allocation, above the former 4096 limit. Both assignments finish,
  the coordinator revision pins the configured limits and held tokens settle.
- Plan readiness rejects excessive coordination allocations and continues honoring
  a smaller workspace allowance. The Guide sees current coordination ceilings.
- Brief-to-plan generation is checked for the configured allowance in the actual
  inference input, retaining the private-conversation isolation checks.

Command results:

- `cargo test -p tetonic-app --lib`: **167 passed, four ignored**, no failures.
  Ignored tests require manual model/proof environments or are subprocess fixtures.
- The two `execution_configuration` tests and
  `planning_uses_only_pinned_brief_and_survives_restart_without_dispatch` also
  passed after binding the tests to the documented example and extending the
  planning-prompt assertion.
- `cargo run -p tetonic-arch-gate -- verify package`: **passed** formatting,
  workspace/all-target Clippy with warnings denied, architecture and static quality.
- `git diff --check` and relative file links in these updated architecture/readiness
  documents passed.

These are scripted-provider execution tests, not real-model quality or soak evidence.

## Next boundary: durable team continuation

Raising execution ceilings does not make a human wait durable. Team human handoffs
remain live, bounded by the active deadline. Root-only checkpoint restoration is
not sufficient for a coordinator with existing child runs. Do not remove those
guards to claim team recovery.

The next work must preserve the coordinator's pending dispatch state, restore
child ownership under fresh fences, retain the original shared reservations and
grant lineage, and distinguish active execution deadlines from bounded human
response horizons. Stopping, revoking or expiring work must defeat a late answer;
duplicate answers/restorers must not repeat model/tool effects. The full product
proof is two independent workers, one parked human request, continued unrelated
work, process restart, then exactly one authorized continuation with completed
contributions retained. Real frontier-model trials and the 48-hour soak remain
separate readiness work.
