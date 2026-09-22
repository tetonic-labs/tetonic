#!/usr/bin/env python3
"""Live smoke test for the lokaid daemon.

Spawns `lokaid`, speaks the agent-rpc-v1 stdio JSON-RPC protocol
(Content-Length framing), drives a real one-turn agent run against the local
Ollama, and prints the streamed notifications. Use it to eyeball the daemon
end-to-end; the deterministic, Ollama-free coverage lives in the Rust tests
(`cargo test -p lokaid`).

Usage:
    python smoke_lokaid.py <workspace_dir> [prompt]

Env:
    LOKAI_MODEL    model to use (default: the daemon's default)
    LOKAI_OLLAMA   Ollama base url (default: http://localhost:11434)
    LOKAID_BIN     path to the lokaid binary (default: ../target/debug/lokaid[.exe])
"""
import json
import os
import sys
import threading
import queue
import subprocess
from pathlib import Path


def frame(obj: dict) -> bytes:
    body = json.dumps(obj).encode("utf-8")
    return b"Content-Length: %d\r\n\r\n%s" % (len(body), body)


def read_frame(stream) -> dict | None:
    """Read one Content-Length-framed JSON message, or None at EOF."""
    length = None
    while True:
        line = stream.readline()
        if not line:
            return None  # EOF
        line = line.rstrip(b"\r\n")
        if line == b"":
            break
        if line.lower().startswith(b"content-length:"):
            length = int(line.split(b":", 1)[1].strip())
    if length is None:
        return None
    body = stream.read(length)
    return json.loads(body.decode("utf-8"))


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    workspace = str(Path(sys.argv[1]).resolve())
    prompt = sys.argv[2] if len(sys.argv) > 2 else "List the files in this workspace, then finish."

    here = Path(__file__).resolve().parent
    exe = "lokaid.exe" if os.name == "nt" else "lokaid"
    bin_path = os.environ.get("LOKAID_BIN", str(here.parent / "target" / "debug" / exe))
    if not Path(bin_path).exists():
        print(f"daemon not found at {bin_path}; build it with `cargo build -p lokaid`", file=sys.stderr)
        return 1

    print(f"# spawning {bin_path}\n# workspace: {workspace}\n")
    proc = subprocess.Popen(
        [bin_path],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=sys.stderr,
        bufsize=0,
    )

    # Background reader: every inbound frame goes onto a queue so the main
    # thread can both match request responses (by id) and watch notifications.
    q: "queue.Queue[dict | None]" = queue.Queue()

    def reader():
        while True:
            msg = read_frame(proc.stdout)
            q.put(msg)
            if msg is None:
                break

    threading.Thread(target=reader, daemon=True).start()

    def send(obj: dict):
        proc.stdin.write(frame(obj))
        proc.stdin.flush()

    def wait_for_response(req_id):
        """Drain frames, printing notifications, until the response to req_id."""
        while True:
            msg = q.get()
            if msg is None:
                raise RuntimeError("daemon closed the pipe unexpectedly")
            if msg.get("id") == req_id and ("result" in msg or "error" in msg):
                return msg
            print_notification(msg)

    def print_notification(msg: dict):
        method = msg.get("method", "")
        p = msg.get("params", {})
        agent = p.get("agent_id", "?")
        if method == "event/token":
            sys.stdout.write(p.get("delta", ""))
            sys.stdout.flush()
        elif method == "event/run_status":
            print(f"\n[{agent}] run: {p.get('status')}" + (f" ({p.get('error')})" if p.get("error") else ""))
        elif method == "event/tool_call":
            print(f"\n[{agent}] -> {p.get('tool')}({json.dumps(p.get('args', {}))[:100]})")
        elif method == "event/tool_result":
            mark = "ok" if p.get("ok") else f"ERR({p.get('error_kind')})"
            print(f"[{agent}] <- [{mark}] {p.get('summary','')[:200]}")
        elif method == "event/diff":
            print(f"[{agent}] diff {p.get('kind')} {p.get('path')}")
        elif method == "event/approval_request":
            print(f"[{agent}] APPROVAL NEEDED: {p.get('kind')} -> {p.get('detail')}")
        elif method == "event/egress":
            print(f"[{agent}] egress {p.get('decision')} {p.get('host')}:{p.get('port')} ({p.get('reason')})")
        elif method == "event/context":
            print(f"[{agent}] ctx {p.get('total_tokens')}/{p.get('budget')} tok")
        elif method:
            print(f"\n[{agent}] {method}: {json.dumps(p)[:200]}")

    # 1) initialize
    send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
          "params": {"protocol_version": 1, "workspace_root": workspace,
                     "client_info": {"name": "smoke", "version": "0"}}})
    init = wait_for_response(1)
    if "error" in init:
        print(f"initialize failed: {init['error']}", file=sys.stderr)
        return 1
    caps = init["result"]["capabilities"]
    print(f"# initialized: streaming={caps['streaming']} approvals={caps['approvals']}")
    print(f"# tools={caps['tools']}")
    print(f"# reserved orchestration tools={caps['orchestration_tools']}\n")

    # 2) model/list (sanity)
    send({"jsonrpc": "2.0", "id": 2, "method": "model/list", "params": {}})
    models = wait_for_response(2)["result"]
    print(f"# default model={models['default_model']} tool_capable={models['tool_capable']}")
    if not models["tool_capable"]:
        print("# WARNING: selected model is not tool-capable; the run will likely fail.")

    # 3) session/start
    send({"jsonrpc": "2.0", "id": 3, "method": "session/start", "params": {}})
    sid = wait_for_response(3)["result"]["session_id"]
    print(f"# session={sid}\n")

    # 4) chat/send (work streams as notifications)
    send({"jsonrpc": "2.0", "id": 4, "method": "chat/send",
          "params": {"session_id": sid, "text": prompt}})
    wait_for_response(4)  # the immediate { accepted: true }

    # 5) follow the stream until the run reaches a terminal status
    print("# --- run stream ---")
    while True:
        msg = q.get()
        if msg is None:
            print("\n# daemon closed pipe")
            break
        if msg.get("method") == "event/run_status" and msg["params"].get("status") != "started":
            print_notification(msg)
            break
        print_notification(msg)

    # 6) shutdown
    send({"jsonrpc": "2.0", "id": 99, "method": "shutdown", "params": {}})
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
    print("\n# done")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
