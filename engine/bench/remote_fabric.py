#!/usr/bin/env python3
"""Remote fabric metrics — wire size + local Ollama baseline (N1.2).

Estimates bytes shipped per agent step (FabricJob JSON) and compares local
Ollama throughput so homelab remote inference regressions are visible before
Circle work lands.

Usage (from engine/):
    python bench/remote_fabric.py
    python bench/remote_fabric.py --steps 8 --out bench/results_remote_fabric.json

For end-to-end remote tok/s, enroll a worker and run the agent suite with
LOKAI_FABRIC_FORCE_REMOTE=1 (requires live Ollama on the worker).
"""
from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

DEFAULT_MODEL = "qwen3.6:latest"


def synthetic_fabric_job_bytes(
    *,
    num_messages: int,
    msg_chars: int,
    num_tools: int,
    tool_chars: int,
) -> int:
    """Approximate POST /v1/chat body size (UTF-8 JSON)."""
    messages = [
        {
            "role": "user" if i % 2 == 0 else "assistant",
            "content": "x" * msg_chars,
        }
        for i in range(num_messages)
    ]
    tools = [
        {
            "type": "function",
            "function": {
                "name": f"tool_{i}",
                "description": "d" * tool_chars,
                "parameters": {"type": "object", "properties": {}},
            },
        }
        for i in range(num_tools)
    ]
    job = {
        "job_id": "job_bench",
        "estate_id": "estate_local",
        "session_id": "sess_bench",
        "agent_id": "a0",
        "step_index": num_messages,
        "model": DEFAULT_MODEL,
        "messages": messages,
        "tools": tools,
        "options": {"temperature": 0.2, "num_ctx": 8192},
        "priority": "OwnerInteractive",
        "data_class": "personal",
        "disclosure_tier": "metadata_only",
        "policy_epoch": 0,
        "turn_affinity": "worker_gpu_box",
    }
    return len(json.dumps(job, separators=(",", ":")).encode("utf-8"))


def ollama_generate(ollama: str, model: str, prompt: str, num_predict: int) -> dict:
    url = f"{ollama.rstrip('/')}/api/generate"
    body = json.dumps(
        {
            "model": model,
            "prompt": prompt,
            "stream": False,
            "options": {"num_predict": num_predict},
        }
    ).encode()
    req = urllib.request.Request(url, data=body, method="POST")
    req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, timeout=120) as resp:
        return json.loads(resp.read())


def local_tok_per_sec(ollama: str, model: str, runs: int) -> dict | None:
    try:
        urllib.request.urlopen(f"{ollama.rstrip('/')}/api/tags", timeout=3)
    except (urllib.error.URLError, TimeoutError):
        return None

    samples = []
    prompt = "Summarize in one sentence: remote fabric ships context each agent step."
    for _ in range(runs):
        t0 = time.perf_counter()
        data = ollama_generate(ollama, model, prompt, num_predict=64)
        elapsed = time.perf_counter() - t0
        eval_count = int(data.get("eval_count") or 0)
        if elapsed > 0 and eval_count > 0:
            samples.append(eval_count / elapsed)
    if not samples:
        return None
    return {
        "runs": runs,
        "tok_per_sec_mean": round(sum(samples) / len(samples), 2),
        "tok_per_sec_min": round(min(samples), 2),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description="Remote fabric wire-size + local baseline")
    ap.add_argument("--model", default=DEFAULT_MODEL)
    ap.add_argument("--ollama", default="http://127.0.0.1:11434")
    ap.add_argument("--steps", type=int, default=8, help="Simulated agent steps")
    ap.add_argument("--msg-chars", type=int, default=400, help="Chars per message")
    ap.add_argument("--tools", type=int, default=12, help="Tool schema count")
    ap.add_argument("--tool-chars", type=int, default=120)
    ap.add_argument("--local-runs", type=int, default=3)
    ap.add_argument("--out", type=Path, default=None)
    args = ap.parse_args()

    wire = []
    for step in range(1, args.steps + 1):
        # Context grows each step (system + history + tools).
        n_msgs = 2 + step * 2
        wire.append(
            {
                "step_index": step - 1,
                "message_count": n_msgs,
                "fabric_job_bytes": synthetic_fabric_job_bytes(
                    num_messages=n_msgs,
                    msg_chars=args.msg_chars,
                    num_tools=args.tools,
                    tool_chars=args.tool_chars,
                ),
            }
        )

    report = {
        "model": args.model,
        "wire_per_step": wire,
        "wire_bytes_total": sum(w["fabric_job_bytes"] for w in wire),
        "notes": [
            "Each remote agent step sends a FabricJob (messages slice + tool schemas).",
            "Turn affinity keeps steps of one user turn on the same worker (KV warm).",
            "Tool schemas repeat each step in v1 — prefix stability reduces churn.",
        ],
        "local_ollama": local_tok_per_sec(args.ollama, args.model, args.local_runs),
    }

    text = json.dumps(report, indent=2)
    print(text)
    if args.out:
        args.out.write_text(text + "\n", encoding="utf-8")
        print(f"wrote {args.out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
