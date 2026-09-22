#!/usr/bin/env python3
"""Raw LLM-layer throughput probe (independent of the lokai engine).

Hits Ollama's /api/chat directly (stream=false) and reads the timing counters
it returns (`prompt_eval_count`, `eval_count`, `*_duration`) to characterize the
two phases that dominate agent latency:

  * prefill (prompt_eval): tokens/sec the model ingests context at  -> sets the
    cost of a big system prompt + tools + retrieved code (our context budget).
  * decode (eval): tokens/sec the model generates at               -> sets the
    cost of long answers / tool-call arguments.

This is the "LLM side" baseline; the agent end-to-end harness (run_suite.py)
layers tool loops and verification on top. Models must be tool-capable for the
agent suite, but throughput here is measured tools-free for a clean number.

Usage:
    python llm_throughput.py [--models a,b,c] [--ollama URL]
"""
from __future__ import annotations

import argparse
import json
import time
import urllib.request

SHORT = "Reply with exactly the word: ready."
# ~ a few hundred tokens of context to exercise prefill.
LONG = (
    "You are reviewing a Rust module. Summarize in one sentence what it does.\n\n"
    + ("pub fn process(items: &[i64]) -> i64 { items.iter().filter(|x| **x > 0).sum() }\n"
       "// a representative line of code that adds prefill tokens\n") * 80
    + "\nOne sentence only."
)


def chat(ollama: str, model: str, prompt: str, num_predict: int) -> dict:
    body = json.dumps({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "stream": False,
        "options": {"temperature": 0.0, "num_predict": num_predict},
    }).encode("utf-8")
    req = urllib.request.Request(f"{ollama}/api/chat", data=body,
                                 headers={"Content-Type": "application/json"})
    t0 = time.perf_counter()
    with urllib.request.urlopen(req, timeout=600) as resp:
        data = json.loads(resp.read().decode("utf-8"))
    wall = time.perf_counter() - t0
    data["_wall_s"] = wall
    return data


def rate(count, duration_ns):
    if not count or not duration_ns:
        return 0.0
    return count / (duration_ns / 1e9)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--models", default="qwen3.6:latest,qwen3.5:latest,mistral:latest")
    ap.add_argument("--ollama", default="http://127.0.0.1:11434")
    args = ap.parse_args()

    models = [m.strip() for m in args.models.split(",") if m.strip()]
    print(f"# raw LLM throughput via {args.ollama}")
    print(f"{'model':<20}{'case':<8}{'prefill tok':>12}{'pf tok/s':>10}{'gen tok':>9}{'gen tok/s':>11}{'wall s':>9}")
    print("=" * 79)

    for m in models:
        # Warm the model (load weights) so the first timed call isn't penalized.
        try:
            chat(args.ollama, m, "hi", 1)
        except Exception as e:
            print(f"{m:<20} SKIP ({e})")
            continue
        for case, prompt, npred in (("short", SHORT, 64), ("long", LONG, 128)):
            try:
                d = chat(args.ollama, m, prompt, npred)
            except Exception as e:
                print(f"{m:<20}{case:<8} ERROR {e}")
                continue
            pf_c = d.get("prompt_eval_count", 0)
            pf_d = d.get("prompt_eval_duration", 0)
            ev_c = d.get("eval_count", 0)
            ev_d = d.get("eval_duration", 0)
            print(f"{m:<20}{case:<8}{pf_c:>12}{rate(pf_c, pf_d):>10.0f}"
                  f"{ev_c:>9}{rate(ev_c, ev_d):>11.1f}{d['_wall_s']:>9.2f}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
