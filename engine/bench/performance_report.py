"""Compare graded task runs without rewarding failures or changed quality settings."""
import argparse
import collections
import json
import statistics
from pathlib import Path

CONFIG = ("model", "max_steps", "num_ctx", "timeout", "no_verify", "task_digest", "repeat", "cache_state")


def summarize(run):
    rows = run["results"]
    successes = [r for r in rows if r["pass"] and r["rc"] == 0 and not r["timed_out"]]
    total = sum(r["completion_s"] for r in rows)
    durations = sorted(r["completion_s"] for r in successes)
    phases = collections.defaultdict(list)
    unclosed = collections.Counter()
    for row in rows:
        pending = {}
        for event in row.get("performance_stages", []):
            if "timing_id" in event:
                identity = (event["stage"], event["timing_id"])
                if event["outcome"] == "started":
                    pending[identity] = event["stage"]
                else:
                    pending.pop(identity, None)
            if event["outcome"] == "succeeded":
                phases[event["stage"]].append(event["duration_ms"])
        unclosed.update(pending.values())
    return {
        "attempts": len(rows), "successes": len(successes),
        "success_rate": len(successes) / len(rows) if rows else 0,
        "total_completion_s": total,
        # Includes time spent on unsuccessful attempts; not a per-task retry guarantee.
        "effort_s_per_success": total / len(successes) if successes else None,
        "successful_p50_s": statistics.median(durations) if durations else None,
        "successful_p95_s": durations[max(0, (95 * len(durations) + 99) // 100 - 1)] if durations else None,
        "observed_stages": {stage: {"samples": len(values), "total_ms": sum(values),
                                    "p50_ms": statistics.median(values)}
                            for stage, values in sorted(phases.items())},
        "stage_note": "Nested timings overlap; sampled or missing telemetry is not zero cost. Includes stages from failed attempts.",
        "unclosed_observed_stages": dict(sorted(unclosed.items())),
        "unclosed_note": "Observed starts with no captured terminal event; interruption or missing telemetry, not proof of a backend hang.",
    }


def compare(before, after):
    if any(k not in before or k not in after or before[k] != after[k] for k in CONFIG):
        raise ValueError("Model, context, task definitions, verification, timeout, repetitions and cache state must match")
    if before["no_verify"]:
        raise ValueError("Quality-preserving comparisons require verification enabled")
    def attempts(run):
        return collections.Counter(r["id"] for r in run["results"])
    if not before["results"] or attempts(before) != attempts(after):
        raise ValueError("Both runs must contain the same task attempts")
    def passed(run):
        return collections.Counter(r["id"] for r in run["results"] if r["pass"] and r["rc"] == 0 and not r["timed_out"])
    old, new = passed(before), passed(after)
    regression = any(new[task] < count for task, count in old.items())
    b, a = summarize(before), summarize(after)
    speedup = None
    if not regression and b["effort_s_per_success"] is not None and a["effort_s_per_success"]:
        speedup = b["effort_s_per_success"] / a["effort_s_per_success"]
    return {"baseline": b, "candidate": a, "observed_quality_regression": regression,
            "effort_speedup": speedup,
            "note": "Observed graded tasks only; microbenchmarks and token throughput are not task speedups."}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    args = parser.parse_args()
    try:
        report = compare(json.loads(args.baseline.read_text()), json.loads(args.candidate.read_text()))
    except ValueError as error:
        parser.error(str(error))
    print(json.dumps(report, indent=2))
    raise SystemExit(1 if report["observed_quality_regression"] else 0)
