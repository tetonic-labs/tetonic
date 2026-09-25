#!/usr/bin/env python3
"""Agentic task suite for the lokai coding agent (end-to-end, verified).

For each task this harness:
  1. materializes a fresh temp workspace from the task's seed files,
  2. runs the `lokai` CLI with the task prompt (real model, real tools),
  3. grades the result with a hidden verifier the agent never sees
     (imports the produced module out-of-tree and asserts behavior),
  4. reads the local audit DB to recover tool-call / step / file-change counts,
  5. parses the CLI's [ctx]/[note]/[done] telemetry from stdout.

The point is to measure real capability, not toy success: tasks escalate from a
one-line bug fix to multi-file packages, a recursive-descent expression
evaluator, a behavior-preserving refactor, and a time-based rate limiter — the
kind of thing that has to work for "production-grade software".

Each task workspace includes `_lokai_verify.py` (hidden grader). The CLI
auto-detects it as the verify-before-finish command so the agent cannot finish
over broken code.

Usage (from engine/):
    python bench/run_suite.py
    python bench/run_suite.py --only T5,T7 --max-steps 24
    python bench/run_suite.py --model qwen3.5:latest --out bench/results_qwen35.json
"""
from __future__ import annotations

DEFAULT_MODEL = "qwen3.6:latest"

import argparse
import hashlib
import json
import math
import os
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from pathlib import Path

# --------------------------------------------------------------------------- #
# Task definitions. Each: id, difficulty, files (seed), prompt, grader source. #
# The grader runs in a SEPARATE process with the workspace on sys.path[0]; it  #
# must print "GRADE: PASS" and exit 0 on success. The agent never sees it.     #
# --------------------------------------------------------------------------- #

TASKS = [
    {
        "id": "T1", "difficulty": "1-easy",
        "files": {"mathx.py": "def add(a, b):\n    return a + b\n\n\ndef mul(a, b):\n    return a + b  # wrong\n"},
        "prompt": "There is a bug in mathx.py: mul() adds instead of multiplies. "
                  "Read the file, fix mul so it returns the product, then finish.",
        "grader": """
import mathx
assert mathx.add(2, 3) == 5, "add regressed"
assert mathx.mul(3, 4) == 12, f"mul(3,4)={mathx.mul(3,4)}"
assert mathx.mul(-2, 5) == -10
print("GRADE: PASS")
""",
    },
    {
        "id": "T2", "difficulty": "2-easy-med",
        "files": {"textutil.py": "def slugify(s):\n    \"\"\"TODO: implement.\"\"\"\n    raise NotImplementedError\n"},
        "prompt": "Implement slugify(s) in textutil.py. It must: lowercase the text; "
                  "replace any run of non-alphanumeric characters with a single hyphen; "
                  "strip leading/trailing hyphens. Example: slugify('  Hello, World! ') == 'hello-world'. "
                  "Edit the file and finish.",
        "grader": """
from textutil import slugify
assert slugify("  Hello, World! ") == "hello-world", slugify("  Hello, World! ")
assert slugify("foo___bar") == "foo-bar"
assert slugify("Already-Slug") == "already-slug"
assert slugify("a  b   c") == "a-b-c"
assert slugify("!!!") == "", repr(slugify("!!!"))
print("GRADE: PASS")
""",
    },
    {
        "id": "T3", "difficulty": "3-med",
        "files": {"lru.py": "class LRUCache:\n    \"\"\"Fixed-capacity LRU cache. TODO: implement.\"\"\"\n    def __init__(self, capacity):\n        raise NotImplementedError\n"},
        "prompt": "Implement an LRUCache class in lru.py with: __init__(self, capacity); "
                  "get(self, key) returning the value or None and marking it most-recently-used; "
                  "put(self, key, value) inserting/updating and evicting the least-recently-used entry "
                  "when over capacity. get and put must be O(1) amortized. Edit the file and finish.",
        "grader": """
from lru import LRUCache
c = LRUCache(2)
c.put(1, "a"); c.put(2, "b")
assert c.get(1) == "a"          # 1 now MRU
c.put(3, "c")                   # evicts 2 (LRU)
assert c.get(2) is None, "should have evicted key 2"
assert c.get(3) == "c"
c.put(1, "A")                   # update existing
assert c.get(1) == "A"
c.put(4, "d")                   # evicts 3
assert c.get(3) is None
assert c.get(1) == "A" and c.get(4) == "d"
print("GRADE: PASS")
""",
    },
    {
        "id": "T4", "difficulty": "4-med-hard-multifile",
        "files": {
            "bank/__init__.py": "",
            "bank/account.py": "class Account:\n    \"\"\"TODO: implement.\"\"\"\n",
        },
        "prompt": "Build a small bank package. In bank/account.py implement an Account class with "
                  "__init__(self, owner, balance=0), deposit(amount), and withdraw(amount) which raises "
                  "ValueError('insufficient funds') if amount exceeds the balance (no overdraft). "
                  "Also implement a module-level function transfer(src, dst, amount) that moves money "
                  "between two accounts atomically: if the withdrawal would overdraw src, it must raise "
                  "ValueError and leave BOTH balances unchanged. Export Account and transfer from bank/__init__.py "
                  "so `from bank import Account, transfer` works. Negative amounts to deposit/withdraw/transfer "
                  "must raise ValueError. Edit the files and finish.",
        "grader": """
from bank import Account, transfer
a = Account("alice", 100)
b = Account("bob", 0)
a.deposit(50); assert a.balance == 150
a.withdraw(30); assert a.balance == 120
try:
    a.withdraw(1000); assert False, "overdraft allowed"
except ValueError: pass
transfer(a, b, 100)
assert a.balance == 20 and b.balance == 100, (a.balance, b.balance)
try:
    transfer(a, b, 1000); assert False, "overdraft transfer allowed"
except ValueError: pass
assert a.balance == 20 and b.balance == 100, ("not atomic", a.balance, b.balance)
for bad in (-1,):
    try:
        a.deposit(bad); assert False
    except ValueError: pass
print("GRADE: PASS")
""",
    },
    {
        "id": "T5", "difficulty": "5-hard-parser",
        "files": {"calc.py": "def evaluate(expr):\n    \"\"\"Evaluate an arithmetic expression string. TODO.\"\"\"\n    raise NotImplementedError\n"},
        "prompt": "Implement evaluate(expr: str) -> float in calc.py: a calculator for arithmetic "
                  "expressions supporting + - * / , parentheses, unary minus, integer and decimal "
                  "literals, and standard operator precedence (* and / bind tighter than + and -). "
                  "Do NOT use Python's eval/exec or ast; write a real parser. Raise ValueError on "
                  "malformed input. Examples: evaluate('1 + 2 * 3') == 7; evaluate('(1 + 2) * 3') == 9; "
                  "evaluate('-3 + 4') == 1; evaluate('10 / 4') == 2.5. Edit the file and finish.",
        "grader": """
import calc, inspect
src = inspect.getsource(calc)
assert "eval(" not in src and "exec(" not in src and "import ast" not in src, "used eval/exec/ast"
assert abs(calc.evaluate("1 + 2 * 3") - 7) < 1e-9
assert abs(calc.evaluate("(1 + 2) * 3") - 9) < 1e-9
assert abs(calc.evaluate("-3 + 4") - 1) < 1e-9
assert abs(calc.evaluate("10 / 4") - 2.5) < 1e-9
assert abs(calc.evaluate("2 * (3 + 4) - 5") - 9) < 1e-9
assert abs(calc.evaluate("((1))") - 1) < 1e-9
for bad in ("1 +", "(1 + 2", "1 2", "* 3", ""):
    try:
        calc.evaluate(bad); assert False, f"accepted bad input {bad!r}"
    except ValueError: pass
print("GRADE: PASS")
""",
    },
    {
        "id": "T6", "difficulty": "6-refactor-keep-green",
        "files": {"orders.py": (
            "def compute_total(items, tax_rate, discount):\n"
            "    t = 0\n"
            "    for it in items:\n"
            "        t = t + it['price'] * it['qty']\n"
            "    if discount > 0:\n"
            "        t = t - t * discount\n"
            "    t = t + t * tax_rate\n"
            "    return round(t, 2)\n"
        )},
        "prompt": "Refactor compute_total(items, tax_rate, discount) in orders.py for readability by "
                  "extracting helper functions (e.g. subtotal, apply_discount, apply_tax). The public "
                  "function compute_total MUST keep the same name, signature, and behavior exactly. "
                  "Do not change rounding. Edit the file and finish.",
        "grader": """
import random
# Reference == the ORIGINAL behavior. A correct refactor must match it exactly,
# float-rounding and all (compare against the algorithm, not brittle literals).
def ref(items, tax_rate, discount):
    t = 0
    for it in items:
        t = t + it['price'] * it['qty']
    if discount > 0:
        t = t - t * discount
    t = t + t * tax_rate
    return round(t, 2)
from orders import compute_total
cases = [
    ([{"price": 10.0, "qty": 2}, {"price": 5.5, "qty": 3}], 0.0, 0.0),
    ([{"price": 10.0, "qty": 2}, {"price": 5.5, "qty": 3}], 0.1, 0.0),
    ([{"price": 10.0, "qty": 2}, {"price": 5.5, "qty": 3}], 0.0, 0.5),
    ([{"price": 10.0, "qty": 2}, {"price": 5.5, "qty": 3}], 0.1, 0.5),
    ([], 0.2, 0.1),
]
rng = random.Random(7)
for _ in range(50):
    items = [{"price": round(rng.uniform(1, 99), 2), "qty": rng.randint(1, 9)}
             for _ in range(rng.randint(0, 5))]
    cases.append((items, round(rng.uniform(0, 0.3), 2), round(rng.uniform(0, 0.6), 2)))
for items, tx, dc in cases:
    got, want = compute_total(items, tx, dc), ref(items, tx, dc)
    assert got == want, f"behavior changed: got {got} want {want} for tax={tx} disc={dc}"
import orders
assert orders.compute_total.__code__.co_argcount == 3, "signature changed"
print("GRADE: PASS")
""",
    },
    {
        "id": "T7", "difficulty": "7-hard-stateful",
        "files": {"ratelimiter.py": (
            "class TokenBucket:\n"
            "    \"\"\"Token-bucket rate limiter with time-based refill. TODO.\"\"\"\n"
            "    def __init__(self, capacity, refill_per_sec, now):\n"
            "        raise NotImplementedError\n"
        )},
        "prompt": "Implement a TokenBucket in ratelimiter.py. Constructor: "
                  "__init__(self, capacity, refill_per_sec, now) where `now` is a zero-arg callable "
                  "returning the current time in seconds (injected for testability). The bucket starts "
                  "full (capacity tokens). allow(self, cost=1) -> bool: first refill tokens based on "
                  "elapsed time since the last check (tokens += elapsed * refill_per_sec, capped at "
                  "capacity), then if there are at least `cost` tokens, subtract cost and return True, "
                  "else return False (do not go negative). Edit the file and finish.",
        "grader": """
from ratelimiter import TokenBucket
clock = {"t": 0.0}
now = lambda: clock["t"]
b = TokenBucket(capacity=2, refill_per_sec=1.0, now=now)
assert b.allow() is True          # 2 -> 1
assert b.allow() is True          # 1 -> 0
assert b.allow() is False         # empty
clock["t"] = 1.0                  # +1 token
assert b.allow() is True          # 1 -> 0
assert b.allow() is False
clock["t"] = 100.0                # refill capped at capacity (2)
assert b.allow(cost=2) is True
assert b.allow() is False
clock["t"] = 0.5                  # time going backwards must not add tokens / crash
b.allow()
print("GRADE: PASS")
""",
    },
]


def find_bin() -> str:
    env = os.environ.get("LOKAI_BIN")
    if env:
        return env
    exe = "tetonic.exe" if os.name == "nt" else "tetonic"
    here = Path(__file__).resolve().parent.parent  # engine/
    for prof in ("release", "debug"):
        p = here / "target" / prof / exe
        if p.exists():
            return str(p)
    raise SystemExit("tetonic binary not found; build it: cargo build --release -p lokai-cli")


def audit_db() -> Path:
    base = os.environ.get("APPDATA") or os.path.expanduser("~/.local/share")
    return Path(base) / "lokai" / "data" / "lokai.db"


def query_audit(ws_tag: str) -> dict:
    """Recover counts for the most recent session whose workspace matches ws_tag."""
    db = audit_db()
    out = {"steps_msgs": 0, "tool_calls": 0, "tools": {}, "file_changes": 0, "status": "?"}
    if not db.exists():
        return out
    try:
        c = sqlite3.connect(str(db))
        row = c.execute(
            "select id, status from sessions where workspace_root like ? order by started_at desc limit 1",
            (f"%{ws_tag}%",),
        ).fetchone()
        if not row:
            return out
        sid, status = row
        out["status"] = status
        out["steps_msgs"] = c.execute(
            "select count(*) from messages where session_id=? and role='assistant'", (sid,)).fetchone()[0]
        for tool, n in c.execute(
            "select tool, count(*) from tool_calls where session_id=? group by tool", (sid,)):
            out["tools"][tool] = n
            out["tool_calls"] += n
        out["file_changes"] = c.execute(
            "select count(*) from file_changes where session_id=?", (sid,)).fetchone()[0]
        c.close()
    except Exception as e:
        out["error"] = str(e)
    return out


CTX_RE = re.compile(r"\[ctx\]\s*~?(\d+)/(\d+)\s*tok\s*\((\d+)%\)")


def parse_stdout(text: str) -> dict:
    max_tok = 0
    max_pct = 0
    for m in CTX_RE.finditer(text):
        max_tok = max(max_tok, int(m.group(1)))
        max_pct = max(max_pct, int(m.group(3)))
    notes = text.count("[note]")
    done = ""
    dm = re.search(r"\[done\]\s*(.+)", text)
    if dm:
        done = dm.group(1).strip()[:40]
    return {"max_ctx_tok": max_tok, "max_ctx_pct": max_pct, "compactions": notes, "stop": done}


def check_capacity_doctor(lokai_bin: str) -> tuple[bool, str]:
    """Run `lokai estate capacity doctor` (exit 0 = healthy)."""
    try:
        p = subprocess.run(
            [lokai_bin, "estate", "capacity", "doctor"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=30,
        )
        lines = (p.stdout or p.stderr or "").strip().splitlines()
        msg = lines[-1][:200] if lines else "capacity doctor failed"
        return p.returncode == 0, msg
    except subprocess.TimeoutExpired:
        return False, "capacity doctor timeout"
    except OSError as e:
        return False, str(e)


def grade(ws: Path, grader_src: str) -> tuple[bool, str]:
    gdir = Path(tempfile.mkdtemp(prefix="lokai-grade-"))
    try:
        g = gdir / "grader.py"
        g.write_text(
            "import sys\n"
            f"sys.path.insert(0, {str(ws)!r})\n"
            + grader_src,
            encoding="utf-8",
        )
        p = subprocess.run(
            [sys.executable, str(g)],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=60,
        )
        ok = p.returncode == 0 and "GRADE: PASS" in p.stdout
        reason = "" if ok else (p.stderr.strip().splitlines() or [""])[-1][:120]
        return ok, reason
    except subprocess.TimeoutExpired:
        return False, "grader timeout"
    finally:
        shutil.rmtree(gdir, ignore_errors=True)


def write_verifier(ws: Path, grader_src: str) -> None:
    # Python places the executed script's directory first on sys.path. Keeping
    # this relative lets the same verifier check the transaction overlay before
    # commit; the independent external grader still checks the delivered files.
    (ws / "_lokai_verify.py").write_text("import sys\n" + grader_src, encoding="utf-8")


def run_task(task: dict, bin_path: str, model: str, max_steps: int, num_ctx: int,
             ollama: str, timeout: int, no_verify: bool, diagnostics_dir: str = "") -> dict:
    tag = f"lokai-{task['id']}-{os.getpid()}-{int(time.time()*1000)%100000}"
    ws = Path(tempfile.gettempdir()) / tag
    for rel, content in task["files"].items():
        f = ws / rel
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(content, encoding="utf-8")

    # Agent-visible regression verifier, auto-detected as verify_cmd (D8).
    write_verifier(ws, task["grader"])

    cmd = [bin_path, "--workspace", str(ws), "--model", model,
           "--max-steps", str(max_steps), "--num-ctx", str(num_ctx),
           "--ollama", ollama]
    if no_verify:
        cmd.append("--no-verify")
    cmd.append(task["prompt"])
    t0 = time.perf_counter()
    timed_out = False
    try:
        p = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
        out, err, rc = p.stdout or "", p.stderr or "", p.returncode
    except subprocess.TimeoutExpired as e:
        out = (
            e.stdout.decode("utf-8", errors="replace")
            if isinstance(e.stdout, bytes)
            else (e.stdout or "")
        )
        err = e.stderr.decode("utf-8", errors="replace") if isinstance(e.stderr, bytes) else (e.stderr or "")
        rc = -1
        timed_out = True
    wall = time.perf_counter() - t0

    # Explicit developer opt-in: transcripts can contain prompts and tool output.
    # Keep them separate from the payload-free performance report.
    if diagnostics_dir:
        diagnostic_path = Path(diagnostics_dir) / tag
        diagnostic_path.mkdir(parents=True, exist_ok=True)
        (diagnostic_path / "stdout.txt").write_text(out, encoding="utf-8")
        (diagnostic_path / "stderr.txt").write_text(err, encoding="utf-8")

    grader_passed, reason = grade(ws, task["grader"])
    passed = grader_passed and rc == 0 and not timed_out
    if grader_passed and not passed:
        reason = "grader passed but agent timed out or exited unsuccessfully"
    completion_s = time.perf_counter() - t0
    audit = query_audit(tag)
    tele = parse_stdout(out or "")
    shutil.rmtree(ws, ignore_errors=True)

    return {
        "id": task["id"], "difficulty": task["difficulty"],
        "pass": passed, "reason": reason, "wall_s": round(wall, 1),
        "rc": rc, "timed_out": timed_out, "grader_pass": grader_passed,
        "completion_s": round(completion_s, 3),
        "performance_stages": parse_performance(err),
        "steps": audit["steps_msgs"], "tool_calls": audit["tool_calls"],
        "tools": audit["tools"], "file_changes": audit["file_changes"],
        "max_ctx_tok": tele["max_ctx_tok"], "max_ctx_pct": tele["max_ctx_pct"],
        "compactions": tele["compactions"], "stop": tele["stop"],
    }


def parse_performance(stderr: str) -> list[dict]:
    """Keep bounded stage measurements, never raw diagnostic/prompt payloads."""
    stages = []
    for line in stderr.splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if not isinstance(event, dict) or event.get("component_name") != "lokai_performance":
            continue
        fields, metrics = event.get("outcome", ""), event.get("metrics", {})
        if not isinstance(fields, str) or not isinstance(metrics, dict):
            continue
        label = re.search(r'stage="(startup|task_turn|context|inference|tool|finalization|inference_load|inference_prefill|inference_decode|inference_backend_total|inference_scan|inference_schedule|inference_discovery|inference_schedule_persist|inference_headers|inference_first_chunk|inference_stream)"', fields)
        outcome = re.search(r'outcome="(started|succeeded|unsuccessful|interrupted)"', fields)
        duration = metrics.get("duration_ms")
        if label and outcome and type(duration) in (int, float) and math.isfinite(duration) and duration >= 0:
            measurement = {"stage": label[1], "outcome": outcome[1], "duration_ms": duration}
            timing_id = re.search(r'\btiming_id=(\d+)\b', fields)
            if timing_id:
                measurement["timing_id"] = int(timing_id[1])
            stages.append(measurement)
    return stages[-1000:]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default=DEFAULT_MODEL)
    ap.add_argument("--suite", choices=("coding", "general", "all"), default="coding")
    ap.add_argument("--only", default="", help="comma list of task ids, e.g. T5,T7")
    ap.add_argument("--max-steps", type=int, default=18)
    ap.add_argument("--num-ctx", type=int, default=8192)
    ap.add_argument("--ollama", default="http://127.0.0.1:11434")
    ap.add_argument("--timeout", type=int, default=420)
    ap.add_argument("--out", default="")
    ap.add_argument("--diagnostics-dir", default="",
                    help="opt-in directory for raw agent stdout/stderr (may contain task data)")
    ap.add_argument("--repeat", type=int, default=1, help="fresh-workspace trials per task")
    ap.add_argument("--cache-state", choices=("unknown", "cold", "warm"), default="unknown",
                    help="record externally controlled residency; does not evict models")
    ap.add_argument("--no-verify", action="store_true", help="disable auto verify-before-finish")
    ap.add_argument("--min-pass", type=int, default=0,
                    help="exit 1 if fewer than N tasks pass (CI gate)")
    ap.add_argument("--require-capacity-doctor", action="store_true",
                    help="exit 1 if capacity doctor is not healthy before tasks")
    ap.add_argument("--skip-capacity-check", action="store_true",
                    help="skip capacity doctor preflight")
    args = ap.parse_args()
    if args.repeat < 1:
        ap.error("--repeat must be positive")

    bin_path = find_bin()
    if not args.skip_capacity_check:
        ok, msg = check_capacity_doctor(bin_path)
        if not ok:
            detail = f"capacity doctor not healthy — {msg}"
            if args.require_capacity_doctor:
                print(f"# FAIL: {detail}", file=sys.stderr)
                return 1
            print(f"# WARN: {detail}", file=sys.stderr)
    only = {s.strip() for s in args.only.split(",") if s.strip()}
    from general_tasks import GENERAL_TASKS
    available = TASKS if args.suite == "coding" else GENERAL_TASKS if args.suite == "general" else TASKS + GENERAL_TASKS
    tasks = [t for t in available if not only or t["id"] in only]
    if not tasks or (only - {t["id"] for t in available}):
        ap.error("--only must select known task IDs")
    task_digest = hashlib.sha256(json.dumps(tasks, sort_keys=True).encode()).hexdigest()
    tasks = tasks * args.repeat

    print(f"# lokai agentic suite — model={args.model} max_steps={args.max_steps} num_ctx={args.num_ctx}")
    print(f"# binary={bin_path}\n")
    hdr = f"{'id':<4}{'difficulty':<22}{'pass':<6}{'wall_s':>7}{'steps':>6}{'tools':>6}{'chg':>5}{'ctx%':>6}{'cmpt':>5}  reason/stop"
    print(hdr)
    print("=" * len(hdr))

    results = []
    for t in tasks:
        r = run_task(t, bin_path, args.model, args.max_steps, args.num_ctx, args.ollama,
                     args.timeout, args.no_verify, args.diagnostics_dir)
        results.append(r)
        mark = "PASS" if r["pass"] else ("TO" if r["timed_out"] else "FAIL")
        tail = r["reason"] if not r["pass"] else r["stop"]
        print(f"{r['id']:<4}{r['difficulty']:<22}{mark:<6}{r['wall_s']:>7}{r['steps']:>6}"
              f"{r['tool_calls']:>6}{r['file_changes']:>5}{r['max_ctx_pct']:>6}{r['compactions']:>5}  {tail}")

    npass = sum(1 for r in results if r["pass"])
    total_wall = sum(r["wall_s"] for r in results)
    print("\n" + "-" * len(hdr))
    print(f"passed {npass}/{len(results)}   total wall {total_wall:.0f}s   "
          f"avg {total_wall/max(len(results),1):.0f}s/task")

    summary = {"model": args.model, "max_steps": args.max_steps, "num_ctx": args.num_ctx,
               "passed": npass, "total": len(results), "results": results,
               "repeat": args.repeat, "timeout": args.timeout, "no_verify": args.no_verify,
               "cache_state": args.cache_state,
               "task_digest": task_digest, "binary": bin_path,
               "binary_sha256": hashlib.sha256(Path(bin_path).read_bytes()).hexdigest(),
               "total_completion_s": sum(r["completion_s"] for r in results)}
    if args.out:
        Path(args.out).write_text(json.dumps(summary, indent=2), encoding="utf-8")
        print(f"# wrote {args.out}")
    if args.min_pass > 0 and npass < args.min_pass:
        print(f"# FAIL: {npass}/{len(results)} passed (need >= {args.min_pass})", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
