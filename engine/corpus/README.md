# Lokai V2 Evaluation Corpus

Immutable starting points for `lokai-eval`, as defined by Milestone M0-2 / R5-2.

## Structure

* `manifests/`: Scenario JSON (task, bounds, graders).
* `fixtures/snap-NN/`: Directory snapshots (not tarballs). `lokai-eval` copies them to a temp workspace.
* `digests.json`: SHA-256 of each fixture directory (`lokai-eval integrity`).
* `ci_baseline.json`: Last accepted `pr` subset result. Update only with `--write-baseline`.
* `scripts/`: Python helpers kept for provenance; the Rust harness is authoritative.

```bash
cargo run -p lokai-eval -- integrity --corpus corpus
cargo run -p lokai-eval -- run --corpus-id 07-synthetic-sensitive --corpus corpus
```

## Scenario classes

All 12 M0-2 classes have a fixture. **These are synthetic stubs**, not
production-scale repositories. Gaps:

| Class | Fixture | Honest size / notes |
|-------|---------|---------------------|
| 1 Small single-language | snap-01 | Tiny Rust crate. PR quality smoke. |
| 2 Large monorepo | snap-02 | **Stub** (not >5,000 files). Deferred. |
| 3 Multi-language | snap-03 | Tiny Rust + Python. |
| 4 Failing tests | snap-04 | Tiny crate. |
| 5 Dirty tree | snap-05 | Tiny crate. |
| 6 Cross-file refactor | snap-06 | Tiny crate. |
| 7 Synthetic sensitive | snap-07 | Detectable AWS key (`AKIAIOSFODNN7EXAMPLE`). **Security gate.** |
| 8 Symbol discovery | snap-08 | Tiny crate. |
| 9 Test creation | snap-09 | Tiny crate. |
| 10 Misleading implementation | snap-10 | Tiny crate. |
| 11 No-op correct | snap-11 | Tiny crate. |
| 12 Prompt injection | snap-12 | Injected instructions + AWS key. **Security gate.** |

PR CI runs 01 (quality) and 07+12 (security) with recorded fixtures. Recorded
scripts cover all remaining class stubs (`02`–`06`, `08`–`11`). All non-PR
fixtures stay **synthetic stubs** (snap-02 is not a 5k-file monorepo).

## Adding scenarios

1. Add `fixtures/snap-NN/` files.
2. `cargo run -p lokai-eval -- integrity --corpus corpus --write`
3. Add `manifests/NN-….json` using the `lokai-eval` schema.
4. Add a recorded script in `lokai-eval` `providers::script_for_scenario` (or live model).
5. Re-record `ci_baseline.json` with `--write-baseline` if the scenario joins the PR subset.
