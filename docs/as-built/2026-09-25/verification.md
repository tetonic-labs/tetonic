# Coverage, verification and unresolved areas

[Overview](README.md) · [File ledger](coverage.csv) · [Evidence index](evidence.md)

## Scope and evidence discipline

The analyzed baseline is `0f722054c691ee90f57391fe8e9fafbe9a99a108`, branch `feature/domain-pack-decoupling`. `analyze.py` enumerates the baseline Git tree and reads the corresponding working-tree files. Tracked implementation files were unchanged from that baseline. New documentation is outside the baseline inventory. Pre-existing untracked `docs/design/` and the sprint-6 integration directory were left untouched and excluded from implementation evidence.

Existing architecture documents, READMEs, plans and the previous audit were not used to establish behavior. Documentation/instruction files are inventoried by bytes/hash and explicitly excluded from behavioral evidence. Repository working instructions were read as instructions. Existing comments were useful navigation, but the operational claims refer to implementation and composition/call sites. The historical isolated audit probe is inventoried as a probe, not production code or a newly run test.

## Coverage meaning

All **959 tracked files** have a ledger row. The inventory scans 784 applicable text artifacts for declarations/imports/control/storage markers, extracts 9811 lexical declarations and records 69 SQL table declarations. These figures include tests, configurations and fixture code. They are not counts of production-reachable functions, deployed tables or independently verified systems.

Each row includes category, subsystem, byte count, SHA-256, line count, examination method, review depth and extraction counts. A row marked mechanical means exactly that. Source-cited files received selected implementation/call-path review, **not a guarantee that every statement in that file was manually verified**. `evidence.json` identifies the actual cited scopes; the ledger deliberately does not upgrade entire files to “fully verified.”

| File class | Handling |
|---|---|
| Rust source and scripts | Entire text scanned mechanically; selected entrypoints and implementation paths substantively traced; remaining leaf behavior explicitly unresolved. |
| Tests | Indexed separately where path/name permits; inline tests remain within source files. Selected tests executed below. A test name or declaration is not evidence of passing integration. |
| Fixtures | Inventoried as input/adversarial fixtures, not deployed code. Dangerous fixtures were not executed. |
| Cargo manifests and lockfiles | Metadata resolves 32 workspace packages and eight binary targets; lockfiles are generated dependency inputs. The manifest category contains 34 files: 32 package manifests, the workspace root and the historical probe manifest. Nine additional Cargo manifests are classified as fixtures. |
| Generated TypeScript protocol | Explicitly generated client declarations; generator/schema/dispatch have different roles. Generation was not used to infer handler reachability. |
| Configuration, workflows and JSON data | Structured/lexical scan; selected CI/configuration paths traced. Not all environment-dependent script branches executed. |
| Documentation/instructions/license | Hash/classification; excluded from implementation evidence. |
| Ignore/rule/placeholder files | Individually dispositioned resources; no runtime behavior inferred. |
| Binary and vendored content | No binary files detected by the UTF-8/NUL classifier and no separately identified vendored source tree in this baseline. Cargo registry dependencies are external and were not reverse engineered. This is a classification result, not a license/provenance audit. |

## Entrypoint coverage

All eight Cargo binary entrypoints are explicitly traced in [entrypoints](entrypoints.md), including pure worker, combined, supervisor, schema-only and offline CLI branches. Helper binaries are distinguished from product services. Script and fixture entry surfaces are inventoried and categorized separately. Their dynamic branches, external services and destructive/publishing operations have not all been exercised. This limitation is deliberate and recorded; it is not disguised as full script verification.

## Checks executed for this package

| Command/check | Observed result | What it supports / does not support |
|---|---|---|
| `cargo metadata --no-deps --format-version 1 --manifest-path engine/Cargo.toml` | Passed | Workspace/target/local dependency inventory; not macro-expanded call reachability. |
| `cargo test -p tetonic-domain --lib -j 1` (in `engine`) | **30 passed**, none failed/ignored/filtered | Selected domain serialization, classification, canonicalization, cancellation and contracts. Does not prove service wiring. |
| `cargo test -p tetonic-server -j 1` (in `engine`) | **13 passed**, none failed/ignored/filtered | Context budgets, experience/intention scoping, parse/truncation rejection, event acknowledgement, idle wakeup and trace bounds. Uses controlled providers; not a live Village/Ollama test. |
| `cargo run -p tetonic-arch-gate -- arch` (in `engine`) | Passed, zero findings | Implemented static architecture rules. This was `arch`, not the full formatting/clippy/test verification tier. |
| Baseline comparison | Tracked implementation unchanged | Documentation-only task; no production fixes hidden in the package. |
| `validate.py` | See [validation-results.json](validation-results.json) | Ledger completeness/hashes, source anchors, local document links and fenced-diagram inventory. |
| Mermaid parser | See [mermaid-results.json](mermaid-results.json) | Syntax acceptance by Mermaid 11.12.0; does not validate architectural truth or guarantee ideal rendered layout. |

The current task did not run a fresh whole-workspace test suite, cross-platform builds, destructive sandbox probes, worker fault-injection matrix, or live Village integration. Prior turn outcomes are not presented as newly executed checks. No inference service or game server was restarted for this documentation work.

## Reproduction

From the repository root:

```text
python docs/as-built/2026-09-25/analyze.py
python docs/as-built/2026-09-25/build_docs.py
python docs/as-built/2026-09-25/validate.py
```

`analyze.py` is pinned to the baseline tree but reads working bytes; keep that source revision checked out to reproduce hashes. `build_docs.py` resolves literal source anchors to one-based lines, generates Markdown and the package atlas, and marks source-cited coverage scopes. Hand-authored supporting text is in that script and `subsystems.md.in`; `verification.md` is maintained directly. Neither script modifies production code. Capture artifacts describe the environment at analysis time, so untracked-file lists are not byte-for-byte stable across runs.

For Mermaid syntax, install `mermaid@11.12.0` and `jsdom@26.1.0` into a temporary directory with lifecycle scripts disabled, then invoke `validate_mermaid.mjs` with that directory as its first argument. The dependencies are validation tools only, not repository/runtime dependencies. The script writes a per-diagram result file. No `node_modules` or package install changes belong to this commit.

## Unresolved areas and the limit of this deliverable

1. **Semantic leaf coverage:** the ledger is exhaustive; manual statement-level review is not. Uncited files and uncited portions of cited files remain mechanically examined. A resolved Rust call graph across macros, trait implementations, features and platform cfgs was not produced.
2. **Script and platform behavior:** script families/entry surfaces are mapped, but publishing/install/destructive fixture paths and all external-tool branches were not run. Linux/macOS sandbox behavior was read only to the documented interface boundary, not certified on those operating systems.
3. **Live world contract:** The Village repository is outside this source snapshot. Visibility radius, physics, collision, receipt durability and renderer truth need source/dynamic evidence from that separate repo. Tetonic's transport cannot prove them.
4. **Distributed faults:** remote inference dispatch and validation exist, but this task did not execute network partitions, process-kill windows, storage corruption, lease races, multi-coordinator contention or reassignment under load. Current code is not evidence of consensus or transparent agent migration.
5. **Conditional/unused libraries:** negative wiring findings are based on the inspected composition roots and source searches. External downstream consumers are unknown; exports alone neither prove use nor prove impossibility of use.
6. **Security strength:** source controls are described at their actual boundaries. This package is not a penetration test, complete dependency audit or proof of secret detection for every encoding/payload.
7. **Diagram quality:** diagrams are split by scope and checked against the adjacent traces and source anchors. Parser acceptance supports syntax; no claim of automated proof of arrow semantics or comprehensive rendered-page visual QA is made.

This is a source-backed as-built documentation package with exhaustive file disposition and explicit verification limits. It is **not “100% verified” documentation of every possible execution**. These unresolved areas are part of the result, not assumed successes.
