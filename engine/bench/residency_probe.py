"""Local timing probe, not a task-quality benchmark. Does not unload other models."""
import argparse
import json
import time
import urllib.request


def get(base, route):
    with urllib.request.urlopen(base + route, timeout=10) as response:
        return json.load(response)


def probe(base, model, context, repeats):
    print(json.dumps({"phase": "before", "residency": get(base, "/api/ps")}), flush=True)
    for repeat in range(repeats):
        body = {"model": model, "messages": [{"role": "user", "content": "Reply with exactly the word ready."}],
                "stream": True, "options": {"num_ctx": context, "num_predict": 128, "temperature": 0}}
        request = urllib.request.Request(base + "/api/chat", data=json.dumps(body).encode(),
                                         headers={"Content-Type": "application/json"})
        start = time.perf_counter()
        times = {}
        done = False
        with urllib.request.urlopen(request, timeout=120) as response:
            times["headers_s"] = time.perf_counter() - start
            for line in response:
                chunk = json.loads(line)
                if "error" in chunk:
                    raise RuntimeError(chunk["error"])
                times.setdefault("first_chunk_s", time.perf_counter() - start)
                for field in ("thinking", "content"):
                    if chunk.get("message", {}).get(field):
                        times.setdefault("first_" + field + "_s", time.perf_counter() - start)
                if chunk.get("done"):
                    done = True
                    times.update({key: chunk.get(key) for key in (
                        "load_duration", "prompt_eval_duration", "eval_duration", "total_duration",
                        "prompt_eval_count", "eval_count", "done_reason")})
        times["wall_s"] = time.perf_counter() - start
        print(json.dumps({"phase": "request", "repeat": repeat, "context": context,
                          "complete_stream": done, "metrics": times}), flush=True)
        print(json.dumps({"phase": "after", "repeat": repeat,
                          "residency": get(base, "/api/ps")}), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", default="http://localhost:11434")
    parser.add_argument("--model", required=True)
    parser.add_argument("--context", type=int, required=True)
    parser.add_argument("--repeats", type=int, default=2)
    args = parser.parse_args()
    probe(args.base, args.model, args.context, args.repeats)
