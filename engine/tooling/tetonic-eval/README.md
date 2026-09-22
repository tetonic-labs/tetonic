# tetonic-eval

Deterministic and statistical evaluation harness for the Lokai agent loop.

## Purpose

`tetonic-eval run` executes the same `RunService::run_turn` path as the CLI against
a versioned corpus under `engine/corpus/`. Graders inspect real workspace diffs
and whether secrets reached the model. The default model is a **recorded
fixture** (scripted tool turns), so CI is deterministic. A live-Ollama default
is a later ratchet of the same floor.

H1-1 outbound scanning uses the same `redact_outbound` function as
`BrokerInferenceProvider`. `--no-scan` is only for negative tests.

## Product-plan / audit

| ID | Status |
|----|--------|
| H4-2 behavioral eval gate | **Done**: PR subset + security subset + compare vs baseline |
| R5-1 real agent loop | **Done**: no mock orchestrator on the default `run` path |
| R5-2 versioned corpus | **Partial**: 12 class fixtures exist as small synthetic stubs; recorded scripts cover all classes (see `engine/corpus/README.md`) |
| F1 (audit) | **Partial** as a live-model quality gate; **Done** as a recorded-fixture merge gate |
| M0-4 crash recovery | **Partial**: R06 proves one `after_turn_plan` crash->restart contract; full matrix is R27+ |
| R06 Eval recovery honesty | **Done**: no greenwash probe; durable journal + `recovery_required` |
| R07 Interrupt -> incomplete | **Done**: Incomplete never counts as pass |
| R30 Harness limits | **Done**: wall / token / attempt -> Incomplete non-pass |
| R17 Corpus scripts batch A | **Implemented**: 02, 03 and 04 have independent pinned acceptance tests |
| R18 Corpus scripts batch B | **Implemented**: 05/08 pin their Python graders; 06 pins an independent API acceptance test |
| R19 Corpus scripts batch C | **Implemented**: 09 uses mutation grading; 10 has pinned acceptance tests; 11 retains no-op FileBoundary |

## Recovery honesty (R06)

Inject point `after_turn_plan` (`LOKAI_FAULT_INJECT=after_turn_plan` exits after durable plan).
Restart contract: run journal present; `resume_state=recovery_required`; never silent success.
See `recovery.rs` and `AFTER_TURN_PLAN_CONTRACT`.

## Gate (audit Q6)

Recorded on 2026-08-20:

| Subset | Scenarios | Floor |
|--------|-----------|-------|
| `quality` | `01-small-single-lang` | **1.0** |
| `security` | `07-synthetic-sensitive`, `12-prompt-injection` | **1.0** |
| `pr` | quality ∪ security | **1.0** |

Statistical mode runs the same `pr` subset **10 times** on `schedule` /
`workflow_dispatch` and uploads `eval-stat.json`. Ratchet the floor when the
default switches from recorded fixtures to a pinned live model.

Baseline update is **explicit**: `--write-baseline corpus/ci_baseline.json`.
`compare` never rewrites the baseline.

## Commands

```bash
cd engine
cargo run -p tetonic-eval -- integrity --corpus corpus
cargo run -p tetonic-eval -- run --corpus-id 01-small-single-lang --corpus corpus
cargo run -p tetonic-eval -- run --subset pr --corpus corpus --out target/eval-pr.json
cargo run -p tetonic-eval -- run --subset security --corpus corpus
cargo run -p tetonic-eval -- compare --baseline corpus/ci_baseline.json --candidate target/eval-pr.json
cargo run -p tetonic-eval -- run --subset pr --write-baseline corpus/ci_baseline.json
```

## Tests

```bash
cargo test -p tetonic-eval
```

Covers: integrity tamper, wrong-edit FileBoundary fail, H1-1 on/off for 07 and
12, injection-follow FileBoundary fail, stable verdict, compare regression vs
improvement, R06 crash-after-plan recovery honesty, R17–R19 recorded script batches.


## Grader evidence and execution limits

`ProtectedFiles` pins trusted grading scripts or configuration to SHA-256 digests
in the manifest. Keep that manifest outside the task's writable workspace. For
example, add a grader with `"type": "ProtectedFiles"` and a `"sha256"` object
mapping `"_grade.py"` to `"sha256:<64 lowercase hexadecimal digits>"`. Generate
the digest from the trusted fixture, never from task output. Paths must be literal
workspace-relative files; aliases and parent traversal are rejected. Pins are
checked before any grader command and after each command, regardless of the
ProtectedFiles entry's position in the list. Missing/changed input blocks a pass.

Checks allow at most 64 pinned files, 16 MiB per file and 128 MiB total. They detect
persistent changes; they do not isolate an executing grader from a concurrent
process replacing and restoring its inputs. Undeclared imported modules, build
configuration and inline tests are not automatically protected. Declare the full
trusted input set. Command/TestExecution graders without ProtectedFiles fail with
`grader_integrity_unconfigured`; they do not execute or qualify. MutationTest also
requires pins. Standalone Python
grading inputs in scenarios 03, 05 and 08 are pinned. Scenarios 02, 04, 06 and 10 pin
independent Rust acceptance tests and Cargo configuration, allowing mutations only
to the implementation files. File boundaries are checked before commands run.
Scenario 09 uses mutation grading, described below. Scenario 02 retains its historical `large-monorepo` ID but is a small
synthetic workspace/logging fixture, not evidence of performance at monorepo scale.

`MutationTest` accepts a command, an explicit `input_files` list, a pinned
`source_path` and trusted `mutant_source` text. It captures at most 64 regular files
(16 MiB each, 128 MiB total), rejects aliases/duplicate paths and materializes two
separate temporary workspaces. The original must pass at least one unskipped test;
the mutant must exit unsuccessfully with actual failed-test evidence and no skips.
Compilation failure, missing reports, timeouts and generic process crashes do not
prove a mutant was detected. Both trials retain the standard 60-second runtime and
256 KiB-per-stream output limits. All captured inputs are checked for modification
after each trial. The live candidate workspace is never mutated by the harness.

The scenario-09 mutant changes only division by zero to return zero; ordinary
division remains unchanged. This prevents unrelated division tests from earning
credit. The candidate may change only tests/divide_by_zero.rs. These checks do not
establish a hostile-code isolation boundary: test code still executes within the
platform sandbox, and malicious test output/process behavior needs broader grader
isolation beyond pre/post hashes and textual test reports.

`TestExecution` requires a successful process exit and evidence of at least one
passing test. `expected_pass_count` is a minimum (never a way to allow zero).
`fail_if_skipped` rejects reported skipped/ignored tests. The legacy
`require_success` field is accepted for manifest compatibility, but setting it to
false no longer allows failed commands to qualify.

Supported evidence is Cargo/libtest summary lines, or exactly one JSON line from
a trusted grader adapter:

```json
{"lokai_test_report":1,"passed":2,"failed":0,"skipped":0}
```

Emit the JSON only after the checks execute; failed checks must report failure
and/or return nonzero. Missing fields, duplicate JSON reports, mixed JSON/Cargo
reports, failed tests, and insufficient counts do not qualify. Logs and reports
are trusted grader inputs, not cryptographic proof of test execution: agent-
editable graders can still lie. Protect graders when evaluating untrusted work.

`CommandExecution` checks successful exit without claiming tests ran. Use it for
compile/static checks such as `cargo check`; never as a replacement for required
functional tests. Scenario 06 now names its existing compile check honestly.
Scenario 02's current stub contains no tests and intentionally fails its existing
one-test requirement; its fixture needs substantive coverage before qualification.

ManualReview returns `Incomplete` / `manual_review_required` and never approves
unattended. There is no authenticated review-submission mechanism yet. Empty
lists of graders also fail (`no_graders`).

Commands use the shared sandbox backend with a 60-second limit per grader and
256 KiB per output stream. Truncated output, timeout, nonzero exit, invalid argv,
or execution errors fail qualification. Commands use the existing verify argv
parser; shell pipelines/redirections are not accepted. Toolchain discovery paths
are explicitly allowed in the filtered environment. Runtime/output enforcement
must be available; other confinement limits are those of the platform backend,
not a promise of full isolation. Limits are per command, not a deadline for the
entire scenario. Native Linux/macOS runtime acceptance remains outstanding.
