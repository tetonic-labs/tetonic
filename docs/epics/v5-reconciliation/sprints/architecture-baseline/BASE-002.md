# BASE-002 — Define system ownership

Date: October 8, 2026. Status: complete.
Scope: step 2 of the architecture tidy-up, based on the integrated `19a9af98`
baseline. This step documents ownership before physical module extraction.

## Delivered

- Replaced the outdated [architecture entry point](../../../../architecture/README.md)
  with the current local product/managed execution path and diagram. Removed
  claims that retired binaries and prototypes are current services.
- Added the [ownership map](../../../../architecture/ownership.md): change routing,
  behavioral owners, authoritative records, public interfaces/internal composition,
  dependencies, boundary tests and the complete 28-package Cargo inventory.
- Added [domain terminology](../../../../architecture/terminology.md), including
  security team versus work roster, persistent agent versus assembled executor,
  work versus run/task/attempt, and product workspace versus filesystem workspace.
- Linked these from the root README and contribution guide. Corrected the
  contributor command to `tetonic-arch-gate`, retired product names and overstated
  claims of universal/static enforcement.

## Ownership decisions

Continue using the existing registry/resource services, work controller, managed
run service, runtime action broker, compute broker, scoped context services and
transactional store. An extraction must move a responsibility with its callers
and tests, not create a competing execution or state owner.

The map explicitly separates saved preferences, current grants and execution
ownership; reusable rosters from security membership; work coordination from
compute scheduling; and remote inference from remote agent execution. Human
questions/approvals/stops, budget reservations, reported usage and UI projections
have named owners and evidence paths.

## Validation

- Traced the current CLI/web entry points and the registered execution chain
  through preparation, assembly, managed execution and durable commit.
- Checked the package inventory and documented production dependency edges using
  `cargo metadata --manifest-path engine/Cargo.toml --no-deps --format-version 1 --offline --locked`.
- Checked relative Markdown links and heading fragments in all six changed
  documents, including source and test targets.
- Checked named entry points against source and reviewed boundaries for private
  composition versus public operations, actual behavior versus future design,
  and retained libraries versus active product paths.
- `git diff --check` passed. Changes are documentation only; no runtime test
  rerun, binary rebuild, server restart or UI modification was necessary.

This records source-backed ownership, not a new proof that every linked test
passes or a claim of full distributed execution. The
[step 1 baseline](BASE-001.md) contains the latest validation run and its limits.

## Next boundary

Step 3 is host construction and configuration: consolidate composition of
storage, policy, logging/telemetry, credentials, runtime and inference dependencies
behind a clear application host boundary. Preserve the current managed/resource
services and product adapters. `Application`, `services`, `TurnBind`,
`job_launch` and `compute_plane` are the starting locations.

No module move, host extraction, schema migration, behavior change or new
architecture enforcement was included in step 2.
