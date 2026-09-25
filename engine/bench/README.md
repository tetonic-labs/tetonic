# Lokai agentic benchmark

End-to-end graded tasks (T1–T7) that measure real agent capability: read → edit → verify loops on a live model.

## Run locally

From `engine/` (requires built `lokai` binary and Ollama with a tool-capable model):

```bash
cargo build --release -p tetonic-cli
python bench/run_suite.py --out bench/results.json
```

Default model: **qwen3.6:latest** (matches CLI/daemon). Override with `--model` for regression against other tiers.

Each task workspace includes `_lokai_verify.py` (hidden grader). The CLI **auto-detects** it as the verify-before-finish command (D8), so the agent cannot `finish` over broken code.

## Sprint A results (qwen3.5, post–loop v2 — historical baseline)

Recorded in `results_sprint_a.json` (2026-06-27):

| Task | Before | After | Notes |
|------|--------|-------|-------|
| T1 | pass | pass | 11 steps |
| T2 | pass | pass | 7 steps (−3) |
| T3 | **fail** | **pass** | verify gate caught broken finish |
| T4 | pass | pass | 6 steps |
| T5 | fail | fail | parser class; no-progress stop @ 8 steps |
| T6 | **fail** | **pass** | verify gate |
| T7 | fail @ 2 steps | fail @ 18 steps | no early prose exit (L-2) |

**5/7 pass** — meets Sprint A gate (`--min-pass 5`).

CI gate (when Ollama is available):

```bash
python bench/run_suite.py --min-pass 5 --out bench/results_ci.json
```

## Flags

- `--repeat 3` — repeat tasks in fresh workspaces; keep the model and all quality settings identical for comparisons.
- `--cache-state cold|warm|unknown` — label externally controlled model residency; does not unload models.
- `--only T5,T7` — subset of tasks
- `--no-verify` — disable auto verify (debug baseline behavior)
- `--min-pass N` — exit 1 if fewer than N tasks pass

## Task completion performance

Select `--suite general` for non-coding document reconciliation (G1), constraint
planning (G2), and data reconciliation (G3), or `--suite all` to include coding.
These tasks use files as evidence and answer transport through the existing CLI;
they do not require implementing application code. Their external graders check
answers and constraints. They are small objective fixtures, not an evaluation of
open-ended research quality. The in-workspace verification script is accessible
to the agent; these are regression checks, not hidden benchmark evidence.

```bash
python bench/run_suite.py --suite general --repeat 3 --out bench/general.json
```

Success requires the external grader to pass, a zero agent exit code, and no
timeout. `completion_s` includes CLI startup, execution, finalization, and external
grading. Failure time is included in total effort. `wall_s` retains the historical
process-only measurement. Reports record the task definition and binary hashes.
`performance_stages` retains numeric developer stage timings from stderr when
available, including partial stderr on timeout. No raw diagnostics are stored by
default. `--diagnostics-dir target/bench-diagnostics` explicitly saves raw stdout
and stderr in a separate directory per attempt; these can contain task data.
Local inference additionally reports `inference_load`, `inference_prefill`,
`inference_decode`, and `inference_backend_total`, when supplied by the runtime.
The comparison reports sample counts, totals and medians by stage. These timers
overlap; do not sum backend totals with their children or treat missing telemetry
as zero cost. Stage totals include work performed in unsuccessful attempts.
Detailed phases also include `inference_scan`, `inference_schedule`,
`inference_discovery`, `inference_schedule_persist`,
`inference_headers`, `inference_first_chunk`, and `inference_stream`. The header
timer includes time waiting for the backend to send HTTP headers (which can
include loading and prompt evaluation). First-chunk time starts after headers;
it measures the first parsed JSON record, not the first visible answer token.
These scopes emit a `started` event and a process-local numeric `timing_id`.
`unclosed_observed_stages` counts starts without a captured terminal event per
attempt. It can reflect process termination or sampled/missing telemetry; it
does not prove a hang and is excluded from successful duration statistics.

Compare two reports from the current harness:

```bash
python bench/performance_report.py baseline.json candidate.json
```

The comparison rejects changed model/context/step limits/task definitions/timeout,
disabled verification, different trial counts, or mismatched residency labels.
It reports successful-completion p50/p95 and total effort per success. A per-task
pass-count regression suppresses the speedup claim and produces a failing exit
code. Repeated graded tasks are evidence, not a universal quality guarantee.
Use the same hardware and control competing load and residency outside the
harness; `unknown` does not prove equivalent cache conditions.

## Related

The agent-visible `_lokai_verify.py` runs against its own script directory,
including when copied into the transaction verification overlay. The independent
external grader checks delivered workspace files after the process ends. Reports
made before this overlay correction are not comparable success baselines: their
internal verifier could read the original files instead of staged edits.

- Harness: `run_suite.py`
- Long-context needle test: `long_context.py`
- Throughput probe: `llm_throughput.py`
