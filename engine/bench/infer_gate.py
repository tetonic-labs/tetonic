#!/usr/bin/env python3
"""Inference gate probe — raw-short (+ optional long) with capacity-profile JSON out.

Validates floor-tier gates from capacity-profile-v1 (default: raw_short wall ≤30s,
GPU processor share ≥30% when VRAM used ≥10GB). Used for CP3 preflight (ES5-3)
and manual P40 placement checks.

Usage:
    python infer_gate.py --model qwen3.6:latest
    python infer_gate.py --model qwen3.6-estate --json-out gate_report.json
    python infer_gate.py --model qwen3.6:latest --fail-on-warn
"""
from __future__ import annotations

import argparse
import json
import re
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from typing import Any

SCHEMA_VERSION = 1
SHORT = "Reply with exactly the word: ready."
LONG = (
    "You are reviewing a Rust module. Summarize in one sentence what it does.\n\n"
    + ("pub fn process(items: &[i64]) -> i64 { items.iter().filter(|x| **x > 0).sum() }\n"
       "// representative line for prefill\n") * 80
    + "\nOne sentence only."
)

DEFAULT_GATES = {
    "raw_short_warn_s": 15.0,
    "raw_short_fail_s": 30.0,
    "gpu_pct_warn": 50.0,
    "gpu_pct_fail": 30.0,
    "gpu_gate_min_vram_mb": 10_000,
}


def http_json(url: str, body: dict | None = None, timeout: float = 600) -> dict:
    data = None if body is None else json.dumps(body).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json"},
        method="GET" if body is None else "POST",
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read().decode("utf-8"))


def chat(ollama: str, model: str, prompt: str, num_predict: int) -> dict[str, Any]:
    body = {
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "stream": False,
        "options": {"temperature": 0.0, "num_predict": num_predict},
    }
    t0 = time.perf_counter()
    out = http_json(f"{ollama}/api/chat", body)
    out["_wall_s"] = time.perf_counter() - t0
    return out


def rate(count: int | None, duration_ns: int | None) -> float:
    if not count or not duration_ns:
        return 0.0
    return count / (duration_ns / 1e9)


def suite_result(data: dict) -> dict[str, Any]:
    pe = data.get("prompt_eval_count") or 0
    ev = data.get("eval_count") or 0
    return {
        "wall_s": round(data.get("_wall_s", 0.0), 3),
        "prompt_tokens": pe,
        "eval_tokens": ev,
        "prefill_tps": round(rate(pe, data.get("prompt_eval_duration")), 2),
        "decode_tps": round(rate(ev, data.get("eval_duration")), 2),
    }


def ollama_ps(ollama: str) -> dict[str, Any]:
    try:
        ps = http_json(f"{ollama}/api/ps", timeout=30)
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError):
        return {}
    models = ps.get("models") or []
    if not models:
        return {}
    m = models[0]
    size_vram = m.get("size_vram") or 0
    return {
        "processor_split": m.get("processor", "unknown/unknown"),
        "vram_used_mb": int(size_vram // (1024 * 1024)) if size_vram else 0,
        "resident_model": m.get("name", ""),
        "load_wall_s": 0.0,
        "measured_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    }


def gpu_pct(processor_split: str) -> float | None:
    parts = processor_split.split("/")
    if len(parts) < 2:
        m = re.match(r"^\s*(\d+(?:\.\d+)?)\s*%", processor_split)
        return float(m.group(1)) if m else None
    m = re.match(r"^\s*(\d+(?:\.\d+)?)\s*%", parts[1])
    return float(m.group(1)) if m else None


def evaluate_gates(report: dict, gates: dict) -> dict[str, Any]:
    failures: list[dict[str, str]] = []
    suites = report.get("suites") or {}
    short = suites.get("raw_short")
    if not short:
        failures.append({"gate": "raw_short", "message": "suite not run", "severity": "fail"})
    else:
        wall = short["wall_s"]
        if wall > gates["raw_short_fail_s"]:
            failures.append({
                "gate": "raw_short",
                "message": f"wall {wall:.1f}s exceeds fail {gates['raw_short_fail_s']:.1f}s",
                "severity": "fail",
            })
        elif wall > gates["raw_short_warn_s"]:
            failures.append({
                "gate": "raw_short",
                "message": f"wall {wall:.1f}s exceeds warn {gates['raw_short_warn_s']:.1f}s",
                "severity": "warn",
            })

    obs = report.get("observed")
    if obs and obs.get("vram_used_mb", 0) >= gates["gpu_gate_min_vram_mb"]:
        pct = gpu_pct(obs.get("processor_split", ""))
        if pct is None:
            failures.append({
                "gate": "gpu_processor",
                "message": f"could not parse processor_split `{obs.get('processor_split')}`",
                "severity": "warn",
            })
        elif pct < gates["gpu_pct_fail"]:
            failures.append({
                "gate": "gpu_processor",
                "message": (
                    f"GPU share {pct:.0f}% below fail {gates['gpu_pct_fail']:.0f}% "
                    f"({obs.get('processor_split')})"
                ),
                "severity": "fail",
            })
        elif pct < gates["gpu_pct_warn"]:
            failures.append({
                "gate": "gpu_processor",
                "message": (
                    f"GPU share {pct:.0f}% below warn {gates['gpu_pct_warn']:.0f}% "
                    f"({obs.get('processor_split')})"
                ),
                "severity": "warn",
            })

    passed = not any(f["severity"] == "fail" for f in failures)
    return {"passed": passed, "failures": failures}


def main() -> int:
    ap = argparse.ArgumentParser(description="Inference gate probe (capacity-profile-v1)")
    ap.add_argument("--model", default="qwen3.6:latest")
    ap.add_argument("--ollama", default="http://127.0.0.1:11434")
    ap.add_argument("--long", action="store_true", help="Also run raw-long prefill suite")
    ap.add_argument("--json-out", help="Write bench-report-v1 JSON to path")
    ap.add_argument("--fail-on-warn", action="store_true")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    try:
        http_json(f"{args.ollama}/api/tags", timeout=10)
    except Exception as e:
        print(f"ollama unreachable at {args.ollama}: {e}", file=sys.stderr)
        return 2

    # Warm load
    try:
        chat(args.ollama, args.model, "hi", 1)
    except Exception as e:
        print(f"warmup failed for {args.model}: {e}", file=sys.stderr)
        return 2

    suites: dict[str, Any] = {}
    try:
        suites["raw_short"] = suite_result(chat(args.ollama, args.model, SHORT, 64))
    except Exception as e:
        print(f"raw_short failed: {e}", file=sys.stderr)
        return 2

    if args.long:
        try:
            suites["raw_long"] = suite_result(chat(args.ollama, args.model, LONG, 128))
        except Exception as e:
            print(f"raw_long failed: {e}", file=sys.stderr)
            return 2

    observed = ollama_ps(args.ollama)
    report = {
        "schema_version": SCHEMA_VERSION,
        "model": args.model,
        "ollama_base": args.ollama,
        "suites": suites,
        "observed": observed or None,
    }
    verdict = evaluate_gates(report, DEFAULT_GATES)
    report["gates"] = verdict

    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as f:
            json.dump(report, f, indent=2)
            f.write("\n")

    if not args.quiet:
        short = suites["raw_short"]
        print(f"model={args.model} raw_short wall={short['wall_s']:.1f}s decode={short['decode_tps']:.1f} tok/s")
        if observed:
            print(f"  placement: {observed.get('processor_split')} vram={observed.get('vram_used_mb')}MB")
        for f in verdict["failures"]:
            print(f"  [{f['severity']}] {f['gate']}: {f['message']}")
        print(f"  gates_passed={verdict['passed']}")

    if not verdict["passed"]:
        return 1
    if args.fail_on_warn and any(f["severity"] == "warn" for f in verdict["failures"]):
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
