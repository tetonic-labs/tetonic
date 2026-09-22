"""Trusted fixture tests; the candidate edits scripts/parse.py, not this file."""
import importlib.util
import json
from pathlib import Path

spec = importlib.util.spec_from_file_location("candidate_parse", Path(__file__).parent / "scripts" / "parse.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

assert module.parse('{"ok": true}') == {"ok": True}
assert module.parse('{"items": [1, 2], "nested": {"value": null}}') == {"items": [1, 2], "nested": {"value": None}}
assert module.parse('{"label": "caf\\u00e9"}') == {"label": "caf\u00e9"}
try:
    module.parse("not JSON")
except json.JSONDecodeError:
    pass
else:
    raise AssertionError("invalid JSON must be rejected")
print(json.dumps({"lokai_test_report": 1, "passed": 4, "failed": 0, "skipped": 0}))
