# Dogfood gate (DF0)

Manual checklist for **PL1 / product loop** sprint. Run from `engine/` with Ollama.

**Models:** qwen3.6 (default production + bench) · qwen3.5 (legacy parity baseline, `results_sprint_a.json`)

## Setup

```powershell
cargo build -p tetonic-cli
.\target\debug\lokai.exe --index --workspace .
```

Banner should show **hundreds** of files/symbols (not "2 files, 0 symbols").

## DF4 — Retrieval honest

- [ ] `lokai --index-status --workspace .` shows realistic file/symbol counts
- [ ] `lokai --search "orchestrator" --workspace .` returns hits
- [ ] Startup banner says `code retrieval on` or warns if index is thin/empty

## DF1 — Read-only explain (≤12 steps, visible answer)

Use `--orchestrate auto` for planner routing on explain prompts.

```powershell
.\target\debug\lokai.exe --orchestrate auto --workspace .
```

Prompts (each should print `--- answer ---` block):

1. "What are the main Rust crates in this workspace? Read only — do not edit."
2. "What does lokai-egress do? Read only."
3. "How does lokai-orchestrator route tasks? Read only."

Pass: plain-text answer visible, no effort-cap exit, no spurious `cargo test` verify on finish.

## DF3 — Multi-turn

Same session as DF1:

1. Ask question 1, wait for answer.
2. Follow-up: "Which of those crates handles the code index?"
3. Follow-up should reference prior context without re-listing the entire tree blindly.

## Regression

```powershell
cargo test --workspace
```

Optional bench (does not block DF0):

```powershell
python bench/run_suite.py
```
