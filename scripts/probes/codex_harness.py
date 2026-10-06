"""Offline Codex app-server feasibility probe; no Tetonic runtime adapter implied.

Uses an isolated home/workspace and a loopback Responses fixture. Does not read
personal Codex configuration, credentials, or repositories. Requires Python 3.11
and codex-cli 0.160.0. Run from the repository root:
    python scripts/probes/codex_harness.py
"""

import argparse
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


PIN = "codex-cli 0.160.0"
MODEL = "tetonic-fixture-model"
TOOL = "tetonic_fixture_read"
RESULT = "The orchard has seventeen trees."


class Fixture(BaseHTTPRequestHandler):
    calls = []
    errors = []
    case = "success"

    def log_message(self, *_):
        pass

    def do_GET(self):
        self.send_error(404)

    def do_POST(self):
        try:
            if self.path != "/v1/responses":
                raise ValueError("unexpected provider endpoint")
            size = int(self.headers.get("Content-Length", "0"))
            if not 0 < size <= 1_000_000:
                raise ValueError("unexpected request size")
            if self.headers.get("Authorization"):
                raise ValueError("fixture must not receive credentials")
            body = json.loads(self.rfile.read(size))
            self.calls.append(body)
            if len(self.calls) > 2:
                raise ValueError("unexpected extra inference")
            if len(self.calls) == 1:
                name = {"denial": "unselected_fixture_tool", "native-denial": "exec_command"}.get(self.case, TOOL)
                arguments = {"cmd": "echo UNSELECTED_NATIVE_PROBE"} if self.case == "native-denial" else {}
                item = {
                    "type": "function_call", "id": "fc_fixture", "status": "completed",
                    "call_id": "fixture-call", "name": name, "arguments": json.dumps(arguments),
                }
            else:
                results = [i for i in body["input"] if i.get("type") == "function_call_output"]
                if not any(i.get("call_id") == "fixture-call" for i in results):
                    raise ValueError("tool result lost its call ID or contents")
                result = str(next(i["output"] for i in results if i.get("call_id") == "fixture-call"))
                if self.case == "success" and RESULT not in result:
                    raise ValueError("selected tool result missing")
                if self.case in ["denial", "native-denial"] and (RESULT in result or not any(word in result.lower() for word in ["unknown", "unsupported"])):
                    raise ValueError(f"unselected tool did not receive a rejection: {result}")
                item = {
                    "type": "message", "id": "msg_fixture", "role": "assistant",
                    "status": "completed", "content": [
                        {"type": "output_text", "text": RESULT if self.case == "success" else "Tool unavailable.", "annotations": []}
                    ],
                }
            response = {
                "id": f"resp_{len(self.calls)}", "object": "response", "model": MODEL,
                "status": "completed", "output": [item],
                "usage": {"input_tokens": 20, "output_tokens": 10, "total_tokens": 30},
            }
            events = [
                {"type": "response.created", "response": {**response, "status": "in_progress", "output": []}},
                {"type": "response.output_item.done", "output_index": 0, "item": item},
                {"type": "response.completed", "response": response},
            ]
            payload = "".join(f"event: {e['type']}\ndata: {json.dumps(e)}\n\n" for e in events).encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
        except Exception as exc:
            self.errors.append(str(exc))
            self.send_error(400)


class Client:
    def __init__(self, executable, directory, port):
        directory = Path(directory)
        work = directory / "work"
        work.mkdir()
        codex_home = directory / "codex"
        codex_home.mkdir()
        self.work = work
        # Explicit child environment: personal provider keys and configuration
        # cannot silently enter a supposedly offline conformance fixture.
        env = {k: v for k, v in os.environ.items() if k.upper() in {
            "SYSTEMROOT", "WINDIR", "PATH", "PATHEXT", "COMSPEC", "TEMP", "TMP",
        }}
        env.update({"CODEX_HOME": str(codex_home), "HOME": str(directory),
                    "USERPROFILE": str(directory), "APPDATA": str(directory / "roaming"),
                    "LOCALAPPDATA": str(directory / "local")})
        config = {
            "model_provider": '"tetonic_probe"', "model": json.dumps(MODEL),
            "approval_policy": '"never"', "sandbox_mode": '"read-only"',
            "features.shell_tool": "false", "features.apply_patch_freeform": "false",
            "features.multi_agent": "false", "features.apps": "false",
            "web_search": '"disabled"', "project_doc_max_bytes": "0",
            "model_providers.tetonic_probe.name": '"Offline fixture"',
            "model_providers.tetonic_probe.base_url": json.dumps(f"http://127.0.0.1:{port}/v1"),
            "model_providers.tetonic_probe.wire_api": '"responses"',
            "model_providers.tetonic_probe.requires_openai_auth": "false",
            "model_providers.tetonic_probe.supports_websockets": "false",
            "model_providers.tetonic_probe.request_max_retries": "0",
            "model_providers.tetonic_probe.stream_max_retries": "0",
        }
        args = [executable, "app-server", "--stdio", "--strict-config"]
        for key, value in config.items():
            args.extend(["-c", f"{key}={value}"])
        self.process = subprocess.Popen(
            args, cwd=work, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, encoding="utf-8",
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
        )
        self.inbox = queue.Queue()
        self.errors = []
        self.events = []
        self.callbacks = []
        self.request_id = 0
        self.readers = [threading.Thread(target=self._read, daemon=True),
                        threading.Thread(target=self._errors, daemon=True)]
        for reader in self.readers:
            reader.start()

    def _read(self):
        for line in self.process.stdout:
            try:
                self.inbox.put(json.loads(line))
            except ValueError:
                self.inbox.put({"probe_error": "non-JSON protocol output"})
        self.inbox.put({"probe_error": "app-server exited"})

    def _errors(self):
        for line in self.process.stderr:
            # The environment and input contain only this script's fixture.
            self.errors.append(line.strip())

    def send(self, message):
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def receive(self, deadline):
        message = self.inbox.get(timeout=max(0.01, deadline - time.monotonic()))
        if "probe_error" in message:
            raise RuntimeError(f"{message['probe_error']}: {self.errors[-4:]}")
        if "method" in message:
            self.events.append(message)
        return message

    def request(self, method, params):
        self.request_id += 1
        request_id = self.request_id
        self.send({"id": request_id, "method": method, "params": params})
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            message = self.receive(deadline)
            if message.get("id") == request_id and "method" not in message:
                if "error" in message:
                    raise RuntimeError(f"{method}: {message['error']}")
                return message["result"]
            if "id" in message and "method" in message:
                raise RuntimeError(f"unexpected request while waiting for {method}")
        raise TimeoutError(method)

    def run(self, case):
        self.request("initialize", {
            "clientInfo": {"name": "tetonic_probe", "version": "0.1"},
            "capabilities": {"experimentalApi": True},
        })
        self.send({"method": "initialized"})
        thread = self.request("thread/start", {
            "cwd": str(self.work), "ephemeral": True, "model": MODEL,
            "baseInstructions": "Use the supplied fixture tool and report its result.",
            "approvalPolicy": "never", "sandbox": "read-only",
            "environments": [], "runtimeWorkspaceRoots": [],
            "dynamicTools": [{"type": "function", "name": TOOL,
                              "description": "Read the harmless in-memory test fixture.",
                              "inputSchema": {"type": "object", "properties": {}, "additionalProperties": False}}],
        })["thread"]["id"]
        turn = self.request("turn/start", {"threadId": thread, "input": [
            {"type": "text", "text": "Read the fixture.", "text_elements": []}
        ]})["turn"]["id"]
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            message = self.receive(deadline)
            if message.get("method") == "item/tool/call":
                params = message["params"]
                assert params["threadId"] == thread and params["turnId"] == turn
                assert params["tool"] == TOOL and params["arguments"] == {}
                assert params["callId"] == "fixture-call"
                assert not self.callbacks, "duplicate effect callback"
                self.callbacks.append(params["callId"])
                if case == "cancel":
                    # Cancel while the host tool is awaiting admission. No tool
                    # result/effect is issued and no next provider request is allowed.
                    self.request_id += 1
                    self.send({"id": self.request_id, "method": "turn/interrupt", "params": {
                        "threadId": thread, "turnId": turn,
                    }})
                else:
                    self.send({"id": message["id"], "result": {
                        "contentItems": [{"type": "inputText", "text": RESULT}], "success": True,
                    }})
            elif "id" in message and "method" in message:
                self.send({"id": message["id"], "error": {"code": -32601, "message": "Not allowed by fixture"}})
                raise RuntimeError(f"unexpected effect request: {message['method']}")
            elif message.get("method") == "turn/completed":
                expected = "interrupted" if case == "cancel" else "completed"
                assert message["params"]["turn"]["status"] == expected, (case, Fixture.errors, message["params"]["turn"])
                assert self.callbacks == ([] if case in ["denial", "native-denial"] else ["fixture-call"])
                assert len(Fixture.calls) == (1 if case == "cancel" else 2)
                assert not Fixture.errors, Fixture.errors
                usage = [m["params"] for m in self.events if m["method"] == "thread/tokenUsage/updated"]
                assert usage and all(m["threadId"] == thread and m["turnId"] == turn for m in usage)
                total = usage[-1]["tokenUsage"]["total"]
                assert total["inputTokens"] == 20 * len(Fixture.calls), total
                assert total["outputTokens"] == 10 * len(Fixture.calls), total
                return
        raise TimeoutError("turn/completed")

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=3)
        for reader in self.readers:
            reader.join(timeout=3)
        self.process.stdout.close()
        self.process.stderr.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", default=shutil.which("codex"))
    parser.add_argument("--scratch-root", type=Path,
                        default=Path(tempfile.gettempdir()) / "tetonic-harness-probes")
    args = parser.parse_args()
    if not args.codex:
        raise SystemExit("Install the pinned Codex executable or pass --codex.")
    version = subprocess.check_output([args.codex, "--version"], text=True).strip()
    if version != PIN:
        raise SystemExit(f"Expected {PIN}; found {version}. Requalify before changing the pin.")
    server = ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        args.scratch_root.mkdir(parents=True, exist_ok=True)
        reports = []
        for case in ["success", "denial", "native-denial", "cancel"]:
            # Retained outside the repository by default for inspection. No
            # recursive deletion, personal home reuse, or credential copying.
            directory = tempfile.mkdtemp(prefix=f"codex-{case}-", dir=args.scratch_root.resolve())
            Fixture.case, Fixture.calls, Fixture.errors = case, [], []
            client = Client(args.codex, directory, server.server_port)
            try:
                client.run(case)
                tools = Fixture.calls[0].get("tools", [])
                names = [tool.get("name", tool.get("type")) for tool in tools]
                assert set(names) == {TOOL, "request_user_input", "get_goal", "create_goal", "update_goal"}, names
                usage = [m["params"] for m in client.events if m["method"] == "thread/tokenUsage/updated"]
                reports.append({"case": case, "provider_requests": len(Fixture.calls),
                                "tool_callbacks": len(client.callbacks), "advertised_tools": names,
                                "usage_events": len(usage), "scratch_directory": directory,
                                "event_types": sorted({m["method"] for m in client.events})})
            finally:
                client.close()
        print(json.dumps({"version": version, "managed_tetonic_execution": False,
                          "cases": reports}, indent=2))
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
