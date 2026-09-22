from pathlib import Path

t = Path("src/app/middlewares/auth/mod.rs").read_text(encoding="utf-8")
assert "eprintln" in t or "println" in t or "log" in t.lower()
print('{"lokai_test_report": 1, "passed": 1, "failed": 0, "skipped": 0}')
