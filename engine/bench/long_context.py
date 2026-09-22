#!/usr/bin/env python3
"""Longer-context stress tests for the lokai agent.

Two scenarios that a naive "stuff everything into the prompt" agent fails but a
retrieval-aware, context-budgeted one should pass:

  needle : a package of many modules with ONE planted bug, described only by
           symptom. The agent must SEARCH the codebase to locate the function
           (it cannot read every file within the budget), then fix it.
  bigfile: one large (~1500-line) module with a bug in a named function. Reading
           the whole file would blow a small context window, so the agent must
           use outline/targeted reads.

Both run with a deliberately small --num-ctx to put the context manager (budget
trimming + compaction + retrieval tools) under real pressure, and verify the fix
with an out-of-tree grader. Reuses the run_suite harness helpers.

Usage (from engine/):
    python bench/long_context.py --num-ctx 4096
    python bench/long_context.py --only needle
"""
from __future__ import annotations

DEFAULT_MODEL = "qwen3.6:latest"

import argparse
import os
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

from run_suite import find_bin, query_audit, parse_stdout, grade


def gen_needle(ws: Path, n_modules: int = 60) -> None:
    pkg = ws / "bigshop"
    pkg.mkdir(parents=True, exist_ok=True)
    (pkg / "__init__.py").write_text("", encoding="utf-8")
    # Distractor modules: plausible, varied, syntactically valid.
    topics = ["inventory", "catalog", "cart", "payment", "auth", "shipping",
              "reporting", "discount", "tax", "search", "review", "wishlist"]
    for i in range(n_modules):
        topic = topics[i % len(topics)]
        body = (
            f"# module {i}: {topic} helpers\n"
            f"def {topic}_load_{i}(ctx):\n"
            f"    \"\"\"Load {topic} data for module {i}.\"\"\"\n"
            f"    return {{'topic': '{topic}', 'id': {i}, 'ok': True}}\n\n"
            f"def {topic}_summarize_{i}(rows):\n"
            f"    total = 0\n"
            f"    for r in rows:\n"
            f"        total += r.get('amount', 0)\n"
            f"    return total\n\n"
            f"class {topic.capitalize()}Service{i}:\n"
            f"    def __init__(self, repo):\n"
            f"        self.repo = repo\n"
            f"    def run(self, n):\n"
            f"        return [self.repo for _ in range(n)]\n"
        )
        (pkg / f"m_{topic}_{i}.py").write_text(body, encoding="utf-8")
    # The needle: a pricing module whose international surcharge is broken.
    needle = (
        "# pricing rules for orders\n"
        "DOMESTIC_RATE = 0.0\n\n"
        "def base_price(total):\n"
        "    return total\n\n"
        "def international_surcharge(total):\n"
        "    # Surcharge for international orders.\n"
        "    return total * 0.0  # BUG: international orders must add a 15% surcharge\n\n"
        "def final_price(total, international=False):\n"
        "    p = base_price(total)\n"
        "    if international:\n"
        "        p += international_surcharge(total)\n"
        "    return p\n"
    )
    (pkg / "m_pricing.py").write_text(needle, encoding="utf-8")


NEEDLE_PROMPT = (
    "Bug report: international orders are NOT getting their required 15% surcharge — "
    "the surcharge currently comes out as 0. Search this codebase to find the function "
    "responsible for the international surcharge, then fix it so it returns 15% of the "
    "order total (0.15 * total). Do not change the domestic path. Then finish."
)

NEEDLE_GRADER = """
from bigshop.m_pricing import international_surcharge, final_price
assert abs(international_surcharge(100) - 15.0) < 1e-9, international_surcharge(100)
assert abs(final_price(200, international=True) - 230.0) < 1e-9, final_price(200, international=True)
assert abs(final_price(200, international=False) - 200.0) < 1e-9, "domestic path changed"
print("GRADE: PASS")
"""


def gen_bigfile(ws: Path, n_funcs: int = 120) -> None:
    ws.mkdir(parents=True, exist_ok=True)
    lines = ["# analytics.py — large module\n"]
    for i in range(n_funcs):
        if i == 73:
            lines.append(
                "def compute_retention(active, total):\n"
                "    # Retention rate = active / total as a percentage.\n"
                "    return active / total  # BUG: must be expressed as a percentage (x100)\n\n"
            )
        else:
            lines.append(
                f"def metric_{i}(values):\n"
                f"    \"\"\"Compute metric {i}.\"\"\"\n"
                f"    if not values:\n"
                f"        return 0\n"
                f"    return sum(values) / len(values) + {i}\n\n"
            )
    (ws / "analytics.py").write_text("".join(lines), encoding="utf-8")


BIGFILE_PROMPT = (
    "In the large file analytics.py, the function compute_retention is wrong: it returns a raw "
    "ratio but it must return a percentage (the ratio multiplied by 100). Use the code tools to "
    "locate compute_retention without reading the whole file, fix it, and finish."
)

BIGFILE_GRADER = """
from analytics import compute_retention
assert abs(compute_retention(50, 200) - 25.0) < 1e-9, compute_retention(50, 200)
assert abs(compute_retention(1, 4) - 25.0) < 1e-9
print("GRADE: PASS")
"""

SCENARIOS = {
    "needle": (gen_needle, NEEDLE_PROMPT, NEEDLE_GRADER),
    "bigfile": (gen_bigfile, BIGFILE_PROMPT, BIGFILE_GRADER),
}


def run_scenario(name: str, bin_path: str, model: str, num_ctx: int,
                 max_steps: int, ollama: str, timeout: int) -> dict:
    gen, prompt, grader = SCENARIOS[name]
    tag = f"lokai-lc-{name}-{os.getpid()}-{int(time.time()*1000)%100000}"
    ws = Path(tempfile.gettempdir()) / tag
    gen(ws)
    n_files = sum(1 for _ in ws.rglob("*.py"))

    cmd = [bin_path, "--workspace", str(ws), "--model", model,
           "--max-steps", str(max_steps), "--num-ctx", str(num_ctx),
           "--ollama", ollama, prompt]
    t0 = time.perf_counter()
    timed_out = False
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        out = p.stdout
    except subprocess.TimeoutExpired as e:
        out = e.stdout.decode() if isinstance(e.stdout, bytes) else (e.stdout or "")
        timed_out = True
    wall = time.perf_counter() - t0

    passed, reason = grade(ws, grader)
    audit = query_audit(tag)
    tele = parse_stdout(out)
    shutil.rmtree(ws, ignore_errors=True)
    return {
        "scenario": name, "files": n_files, "pass": passed, "reason": reason,
        "wall_s": round(wall, 1), "timed_out": timed_out,
        "steps": audit["steps_msgs"], "tool_calls": audit["tool_calls"],
        "tools": audit["tools"], "file_changes": audit["file_changes"],
        "max_ctx_tok": tele["max_ctx_tok"], "max_ctx_pct": tele["max_ctx_pct"],
        "compactions": tele["compactions"], "stop": tele["stop"],
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default=DEFAULT_MODEL)
    ap.add_argument("--only", default="", help="needle and/or bigfile")
    ap.add_argument("--num-ctx", type=int, default=4096)
    ap.add_argument("--max-steps", type=int, default=20)
    ap.add_argument("--ollama", default="http://127.0.0.1:11434")
    ap.add_argument("--timeout", type=int, default=600)
    args = ap.parse_args()

    bin_path = find_bin()
    want = [s.strip() for s in args.only.split(",") if s.strip()] or list(SCENARIOS)

    print(f"# lokai long-context — model={args.model} num_ctx={args.num_ctx} max_steps={args.max_steps}")
    hdr = f"{'scenario':<10}{'files':>6}{'pass':<6}{'wall_s':>8}{'steps':>6}{'tools':>6}{'chg':>5}{'ctx%':>6}{'cmpt':>5}  detail"
    print(hdr)
    print("=" * len(hdr))
    for name in want:
        r = run_scenario(name, bin_path, args.model, args.num_ctx,
                         args.max_steps, args.ollama, args.timeout)
        mark = "PASS" if r["pass"] else ("TO" if r["timed_out"] else "FAIL")
        tail = r["reason"] if not r["pass"] else r["stop"]
        print(f"{r['scenario']:<10}{r['files']:>6}{mark:<6}{r['wall_s']:>8}{r['steps']:>6}"
              f"{r['tool_calls']:>6}{r['file_changes']:>5}{r['max_ctx_pct']:>6}{r['compactions']:>5}  {tail}")
        toolstr = ", ".join(f"{k}:{v}" for k, v in sorted(r["tools"].items()))
        print(f"           tools used: {toolstr}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
