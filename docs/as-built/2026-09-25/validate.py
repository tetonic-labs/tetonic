"""Validate scope, working-byte hashes, evidence anchors and local Markdown links."""
from pathlib import Path
import json, csv, hashlib, re, subprocess
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[2]
BASE='0f722054c691ee90f57391fe8e9fafbe9a99a108'
errors=[]
rows=list(csv.DictReader((HERE/'coverage.csv').open(encoding='utf-8')))
expected=set(subprocess.check_output(['git','ls-tree','-r','--name-only',BASE],cwd=ROOT).decode().splitlines())
actual=[r['path'] for r in rows]
if len(actual)!=len(set(actual)) or set(actual)!=expected:errors.append('ledger set differs from baseline')
for r in rows:
    p=ROOT/r['path']
    if not p.is_file(): errors.append('missing file '+r['path']);continue
    if hashlib.sha256(p.read_bytes()).hexdigest()!=r['sha256']:errors.append('hash mismatch '+r['path'])
    if not all(r[k] for k in ('category','method','review_depth','subsystem')):errors.append('missing disposition '+r['path'])
evidence=json.loads((HERE/'evidence.json').read_text(encoding='utf-8'))
for e in evidence:
    lines=(ROOT/e['file']).read_text(encoding='utf-8-sig').splitlines()
    if not 1<=e['line']<=len(lines) or e['anchor'] not in lines[e['line']-1]:errors.append('invalid anchor '+str(e))
links=0; diagrams=[]
for p in HERE.glob('*.md'):
    body=p.read_text(encoding='utf-8')
    if body.count('```')%2:errors.append('unbalanced fence '+p.name)
    for target in re.findall(r'\]\(([^)]+)\)',body):
        target=target.strip('<>')
        if '://' in target or target.startswith('#'):continue
        file,_,fragment=target.partition('#')
        dest=(p.parent/file).resolve()
        links+=1
        if not dest.is_file():errors.append('missing link '+p.name+': '+target)
        elif fragment.startswith('L'):
            try:
                n=int(fragment[1:]); total=len(dest.read_text(encoding='utf-8-sig').splitlines())
                if n<1 or n>total:errors.append('line out of bounds '+target)
            except (ValueError,UnicodeError):errors.append('invalid line fragment '+target)
    for i,block in enumerate(re.findall(r'```mermaid\n(.*?)\n```',body,re.S),1):
        diagrams.append({'document':p.name,'number':i,'source':block})
(HERE/'diagrams.json').write_text(json.dumps(diagrams,indent=2)+'\n',encoding='utf-8')
result={'baseline':BASE,'tracked_files_checked':len(rows),'source_evidence_anchors_checked':len(evidence),'local_links_checked':links,'mermaid_diagrams':len(diagrams),'errors':errors,'passed':not errors}
(HERE/'validation-results.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
print(json.dumps(result,indent=2))
raise SystemExit(bool(errors))
