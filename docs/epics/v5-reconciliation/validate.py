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
baseline_paths = set(subprocess.check_output(
    ['git', '-C', str(ROOT), 'ls-tree', '-r', '--name-only', inventory['baseline']['commit']]
).decode('utf-8').splitlines())
baseline_paths = {p for p in baseline_paths if not p.startswith('docs/epics/v5-reconciliation/')}
assert {r['path'] for r in rows} == baseline_paths, 'Census differs from recorded baseline tree'
for row in rows:
    raw = (ROOT/row['path']).read_bytes()
    assert len(raw) == int(row['bytes']), row['path']
    assert hashlib.sha256(raw).hexdigest() == row['sha256'], row['path']
metadata = json.loads(subprocess.check_output(
    ['cargo', 'metadata', '--no-deps', '--format-version', '1', '--manifest-path', str(ROOT/'engine/Cargo.toml')]))
members = set(metadata['workspace_members'])
workspace_packages = [p for p in metadata['packages'] if p['id'] in members]
assert {p['name'] for p in workspace_packages} == {p['name'] for p in inventory['packages']}
assert len(workspace_packages) == inventory['baseline']['packages']
assert sum(r['path'].endswith('.rs') for r in rows) == inventory['baseline']['rust_files']
for package in inventory['packages']:
    owned = [r for r in rows if r['package'] == package['name']]
    assert len(owned) == package['tracked_files'], package['name']
    assert sum(r['path'].endswith('.rs') for r in owned) == package['rust_files'], package['name']
links = 0
ticket_ids = []
for plan in (HERE/'sprints').glob('mvp-*/plan.md'):
    ticket_ids.extend(re.findall(r'^## (MVP-\d{3}) ', plan.read_text(encoding='utf-8'), re.M))
assert len(ticket_ids) == 16 and len(set(ticket_ids)) == 16, 'Expected 16 unique MVP tickets'
for doc in HERE.rglob('*.md'):
    mentioned = set(re.findall(r'\bMVP-\d{3}\b', doc.read_text(encoding='utf-8')))
    assert mentioned <= set(ticket_ids), (doc, mentioned - set(ticket_ids))
for doc in HERE.rglob('*.md'):
    text = doc.read_text(encoding='utf-8')
    for target in re.findall(r'\]\(([^)]+)\)', text):
        if '://' in target or target.startswith('#'):
            continue
        resolved = (doc.parent/target.split('#')[0]).resolve()
        assert resolved.exists() or resolved == HERE/'validation.json', (doc, target)
        links += 1
    assert all(line.rstrip() == line for line in text.splitlines()), doc
result = dict(files_hashed=len(rows), baseline_tree_coverage='exact', workspace_packages=len(workspace_packages), local_links_checked=links,
              mvp_tickets=len(ticket_ids), mvp_ticket_references='resolved',
              runtime_tests='not run: audit/planning files only',
              semantic_scope='Targeted source-path review; file census is not exhaustive semantic analysis')
(HERE/'validation.json').write_text(json.dumps(result, indent=2)+'\n', encoding='utf-8')
print(json.dumps(result, indent=2))
