from pathlib import Path

t = Path("src/main.rs").read_text(encoding="utf-8")
assert "UNSTAGED CHANGES HERE" in t
assert "route" in t.lower()
print('{"lokai_test_report": 1, "passed": 2, "failed": 0, "skipped": 0}')
