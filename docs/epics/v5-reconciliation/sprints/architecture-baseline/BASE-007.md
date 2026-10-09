# BASE-007 — Contributor navigation and frontend boundary enforcement

Date: October 8, 2026. Status: complete.
Scope: step 7 of the architecture tidy-up, based on `771ec0f3`.

## Delivered

- Split the private frontend integration into `engine/contracts`, `client`,
  `connection`, `failure` and pure agent/record/task-state/workspace projections.
  Moved browser hash navigation into `lib/workSelection`. Updated existing
  consumers directly instead of retaining a competing compatibility module.
- Preserved the existing component behavior, map layout, styling, animation,
  context polling, command handling and engine contracts. All 66 moved top-level
  declarations match their prior TypeScript syntax trees; all 55 changed consumer
  and test files match outside imports. No production runtime behavior changed.
- Added a TypeScript import-graph check for current owners, resolved imports,
  transport placement, declaration-only wire contracts, infrastructure direction,
  pure projection dependencies and preview/fixture exclusion. It follows helper,
  reexport and literal dynamic-import paths. Negative fixtures exercise failures
  and legitimate type-only dependencies. Test and build commands run the checks;
  the existing production bundle exclusion remains in place.
- Added current Rust owner-presence and product lifecycle/assembly checks.
  Missing owners now fail visibly. CLI/work/workspace/resource code cannot use
  the checked direct lifecycle constructors/commands or create an independent
  agent loop. The existing registered assembly owner remains the agent constructor.
- Corrected the workspace-mutation check to scan `tetonic-tools` rather than
  only the retired `lokai-tools` path, retaining trusted effect owners and
  excluding inline test fixtures. Added a regression test against the current path.
- Reconciled contributor, root, web, engine and package-group READMEs with the
  current implementation. Added frontend ownership and documentation navigation.
  Replaced outdated quality-gate instructions, labeled historical standing-agent
  and Village proposals, and removed obsolete daemon/eval verification promises.
- Added the web boundary/test/build checks to the existing CI workflow. No
  release packaging or deployment work was introduced.

## Validation

| Check | Result |
|---|---|
| Frontend architecture regression suite | 8 passed, including deliberately invalid import graphs |
| Existing frontend suites | 31 files, 212 tests passed |
| TypeScript and production build | Passed; existing large-chunk advisory remains |
| Rust architecture-gate library | 95 passed, including current-path mutation, missing-owner and lifecycle bypass cases |
| `tetonic-arch-gate verify package` | Passed formatting, workspace/all-target Clippy with warnings denied, architecture and static quality |
| Mechanical move review | 66 declarations unchanged; 55 consumer/test files unchanged outside imports |
| Documentation and diff review | Current entry-point links checked; package inventory matched; `git diff --check` passed |

The listed suites total 315 passing tests. This is not a full Rust workspace,
browser interaction, live-provider or remote CI run. The live engine and user
database were not restarted or modified. No paid inference was used.

## Boundaries of this delivery

These checks detect specific syntax and import violations, not all indirect
bypasses or semantic errors. They cannot prove that a displayed status is truthful,
that every effect is safe, or that future requirements fit the architecture.
Wire declarations remain handwritten; behavior and contract tests remain necessary.

Some historical Rust checks still target retired paths. Those scans are not
presented as evidence for current APIs; the new current-owner checks explicitly
fail on missing owners. This step does not modernize every retained historical
rule, remove all maintenance debt, or introduce a new state store or service.

All seven cleanup steps are now delivered. The resulting architecture is an
integrated local product with clearer ownership, contributor navigation and
regression checks. Distributed deployment and remaining MVP features retain their
separate scope and acceptance criteria.
