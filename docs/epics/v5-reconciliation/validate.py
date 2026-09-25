"""Validate the report inventory and local links; does not verify runtime claims."""
from pathlib import Path
import csv
import hashlib
import json
import re
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
inventory = json.loads((HERE/'inventory.json').read_text(encoding='utf-8'))
rows = list(csv.DictReader((HERE/'file-inventory.csv').open(encoding='utf-8', newline='')))
assert len(rows) == inventory['baseline']['tracked_files']
assert len({r['path'] for r in rows}) == len(rows)
for row in rows:
    assert hashlib.sha256((ROOT/row['path']).read_bytes()).hexdigest() == row['sha256'], row['path']
metadata = json.loads(subprocess.check_output(
    ['cargo', 'metadata', '--no-deps', '--format-version', '1', '--manifest-path', str(ROOT/'engine/Cargo.toml')]))
assert {p['name'] for p in metadata['packages']} == {p['name'] for p in inventory['packages']}
links = 0
for doc in HERE.rglob('*.md'):
    text = doc.read_text(encoding='utf-8')
    for target in re.findall(r'\]\(([^)]+)\)', text):
        if '://' in target or target.startswith('#'):
            continue
        resolved = (doc.parent/target.split('#')[0]).resolve()
        assert resolved.exists() or resolved == HERE/'validation.json', (doc, target)
        links += 1
    assert all(line.rstrip() == line for line in text.splitlines()), doc
result = dict(files_hashed=len(rows), workspace_packages=len(metadata['packages']), local_links_checked=links,
              runtime_tests='not run: audit/planning files only',
              semantic_scope='Targeted source-path review; file census is not exhaustive semantic analysis')
(HERE/'validation.json').write_text(json.dumps(result, indent=2)+'\n', encoding='utf-8')
print(json.dumps(result, indent=2))
