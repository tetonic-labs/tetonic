"""Tracked-file census and workspace dependency inventory; no dead-code claims.

Run from any directory. Outputs exclude this report's own directory to avoid
self-referential hashes. TOML dependency edges are declarations, not call graphs.
"""
from pathlib import Path
import csv
import hashlib
import json
import subprocess
import tomllib

OUT = Path(__file__).resolve().parent
ROOT = OUT.parents[2]

def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args]).decode('utf-8')

ROLES = {
    'tetonic-domain': ('reshape', 'Shared identity, execution, authorization and adapter contracts'),
    'tetonic-core': ('reshape', 'Built-in harness; remove universal coding/world lifecycle assumptions'),
    'tetonic-runtime': ('reshape', 'Worker assembly and capability services; separate optional harness strategies'),
    'tetonic-policy': ('keep-integrate', 'Execution policy; add trusted tenant and principal context'),
    'tetonic-transaction': ('isolate', 'Workspace mutation facility for coding capabilities'),
    'tetonic-sandbox': ('keep-integrate', 'Process isolation with explicit supported guarantees'),
    'tetonic-secrets': ('keep-integrate', 'Credential handling and outbound scanning'),
    'tetonic-telemetry': ('keep-integrate', 'Correlated and redacted operational telemetry'),
    'tetonic-run': ('keep-generalize', 'Authoritative durable run/task/attempt state and managed execution'),
    'tetonic-broker': ('keep-integrate', 'Compute admission and budget accounting; not agent registry'),
    'tetonic-capacity': ('isolate', 'Optional inference capacity optimization'),
    'tetonic-orchestrator': ('split', 'Move fleet domain to control services; keep coding strategy optional'),
    'tetonic-node': ('reshape', 'Existing inference worker transport; replace detached role registry'),
    'tetonic-enroll': ('keep-integrate', 'Node trust/bootstrap; distinct from employee authorization'),
    'tetonic-server': ('replace-composition', 'General server bootstrap; migrate world harness out of main'),
    'tetonic-memory': ('split-logically', 'Separate platform persistence from agent memory interfaces'),
    'tetonic-artifact': ('keep-integrate', 'Scoped artifact access and provenance'),
    'tetonic-context': ('isolate', 'Context compilation; coding assumptions behind capability/harness boundary'),
    'tetonic-index': ('isolate', 'Optional repository indexing capability'),
    'tetonic-app': ('reshape', 'Control services and composition; extract coding product behavior'),
    'tetonic-tools': ('isolate', 'Coding tool pack; adapt to generic capability boundary'),
    'tetonic-lsp': ('isolate', 'Optional coding capability'),
    'lokaid': ('retire-after-cutover', 'Compatibility transport to common services; retire independent assembly'),
    'lokai-cli': ('reshape', 'Tetonic client; retain compatibility until command migration'),
    'tetonic-rpc': ('adapt', 'Local RPC compatibility; not authenticated remote control API'),
    'tetonic-egress': ('keep-integrate', 'Outbound authorization; not an OS-wide firewall'),
    'tetonic-inference': ('keep-integrate', 'Provider adapters behind broker and egress'),
    'tetonic-fabric-client': ('reshape', 'Retain live transport; retire legacy wire only after migration'),
    'tetonic-fabric-protocol': ('keep-generalize', 'Worker delivery contracts; distinguish inference from agent execution'),
    'tetonic-arch-gate': ('update', 'Preserve invariants; replace obsolete structural rules'),
    'tetonic-eval': ('keep-expand', 'Retain evaluations; add lifecycle and tenant isolation scenarios'),
    'tetonic-bench': ('keep', 'Performance measurement; add runtime workload baselines'),
}

def main():
    tracked = git('ls-files', '-z').split('\0')
    tracked = [p for p in tracked if p and not p.startswith('docs/epics/v5-reconciliation/')]
    packages = []
    for manifest in sorted((ROOT / 'engine').glob('*/*/Cargo.toml')):
        data = tomllib.loads(manifest.read_text(encoding='utf-8'))
        if 'package' not in data:
            continue
        name = data['package']['name']
        if name not in ROLES:
            raise ValueError(f'Unclassified workspace package: {name}')
        directory = manifest.parent.relative_to(ROOT).as_posix()
        edges = []
        sections = [(k, data.get(k, {})) for k in ['dependencies', 'dev-dependencies', 'build-dependencies']]
        for target, values in data.get('target', {}).items():
            sections += [(f'{target}:{k}', values.get(k, {})) for k in ['dependencies', 'dev-dependencies', 'build-dependencies']]
        for kind, deps in sections:
            for alias, spec in deps.items():
                if isinstance(spec, dict) and 'path' in spec:
                    edges.append({'kind': kind, 'name': spec.get('package', alias), 'path': spec['path']})
        files = [p for p in tracked if p.startswith(directory + '/')]
        packages.append(dict(name=name, path=directory, disposition=ROLES[name][0], destination=ROLES[name][1],
                             tracked_files=len(files), rust_files=sum(p.endswith('.rs') for p in files),
                             local_dependencies=edges, explicit_bins=data.get('bin', []),
                             conventional_main=(manifest.parent/'src/main.rs').exists()))
    for p in packages:
        p['declared_dependents'] = sorted(q['name'] for q in packages if any(e['name'] == p['name'] for e in q['local_dependencies']))
    rows = []
    for path in tracked:
        owner = next((p for p in packages if path.startswith(p['path'] + '/')), None)
        raw = (ROOT/path).read_bytes()
        category = ('workspace-package' if owner else 'documentation' if path.startswith('docs/') else
                    'fixture' if '/fixtures/' in path or '/corpus/' in path else
                    'delivery-tooling' if path.startswith(('.github/', 'scripts/')) else 'repository-support')
        rows.append(dict(path=path, package=owner['name'] if owner else '', category=category,
                         proposed_disposition=owner['disposition'] if owner else 'retain-review-in-context',
                         review_depth='mechanical-census', bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest()))
    with (OUT/'file-inventory.csv').open('w', newline='', encoding='utf-8') as f:
        writer = csv.DictWriter(f, fieldnames=list(rows[0]))
        writer.writeheader(); writer.writerows(rows)
    baseline = dict(commit=git('rev-parse', 'HEAD').strip(), tracked_files=len(rows), packages=len(packages),
                    rust_files=sum(r['path'].endswith('.rs') for r in rows),
                    scope='All tracked files excluding this report; untracked user work excluded',
                    untracked_at_capture=[p for p in git('ls-files', '--others', '--exclude-standard').splitlines()
                                          if not p.startswith('docs/epics/v5-reconciliation/')])
    (OUT/'inventory.json').write_text(json.dumps(dict(baseline=baseline, packages=packages), indent=2)+'\n', encoding='utf-8')
    lines = ['# Workspace disposition inventory', '', 'Generated from manifests and tracked paths. Dispositions are planning recommendations, not proof of dead code.', '',
             '| Package | Files / Rust | Proposed disposition | Target responsibility | Declared dependents (including tests) |', '|---|---:|---|---|---|']
    for p in packages:
        lines.append(f"| `{p['name']}` | {p['tracked_files']} / {p['rust_files']} | {p['disposition']} | {p['destination']} | {', '.join(p['declared_dependents']) or 'none'} |")
    (OUT/'packages.md').write_text('\n'.join(lines)+'\n', encoding='utf-8')
    print(json.dumps(baseline, indent=2))

if __name__ == '__main__':
    main()
