"""Reproducible inventory and lexical source atlas, not a semantic Rust call graph.
Reads all baseline tracked bytes; never uses documentation prose as behavior evidence.
"""
from pathlib import Path
import subprocess, json, hashlib, re, csv, collections

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
BASE = '0f722054c691ee90f57391fe8e9fafbe9a99a108'

def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT)

def link(path, line=1):
    return f'../../../{path}:{line}'

def category(path, text):
    p = Path(path)
    if p.suffix.lower() in {'.md', '.txt'} or p.name == 'LICENSE':
        return 'documentation/instructions: content excluded from behavioral evidence'
    if path.startswith('docs/architecture/audits/') and p.suffix == '.rs':
        return 'isolated historical audit probe, not engine production code'
    if 'fixtures/' in path or 'security-fixtures/' in path:
        return 'fixture: representative input/adversarial data, not production wiring'
    if '/tests/' in path or p.stem.endswith('_tests') or p.name.startswith('test_'):
        return 'test: verifies selected behavior, not production reachability'
    if p.suffix == '.rs': return 'Rust source: whole-file lexical extraction; semantic review varies'
    if p.suffix in {'.py','.ps1','.sh'}: return 'script: whole-file lexical extraction'
    if p.name in {'Cargo.lock'}: return 'generated dependency lock: resolved inputs, not execution'
    if p.name == 'Cargo.toml': return 'Cargo manifest: package/build/dependency configuration'
    if p.suffix in {'.yml','.yaml','.toml','.json'}: return 'configuration/schema/workflow/data: structured text scan'
    if path == 'engine/clients/ts/protocol.ts': return 'generated TypeScript protocol declarations: lexical extraction; generator/schema define provenance'
    if p.suffix == '.ts': return 'TypeScript source: lexical extraction'
    return 'other text/resource: explicit inventory; semantics not inferred'

def main():
    files = git('ls-tree','-r','--name-only',BASE).decode().splitlines()
    rows=[]; atlas=[]; symbols=[]; tables=[]; script_entries=[]
    for path in files:
        data=(ROOT/path).read_bytes()
        try: text=data.decode('utf-8-sig'); binary='\x00' in text
        except UnicodeError: text=''; binary=True
        lines=text.splitlines(); cat='binary: hash/size only; no behavior inferred' if binary else category(path,text)
        subsystem='/'.join(path.split('/')[:3]) if path.startswith('engine/') else path.split('/')[0]
        defs=[]; markers=[]; imports=[]
        is_source=Path(path).suffix in {'.rs','.py','.sh','.ps1','.ts','.sql','.yml','.yaml','.toml','.json'}
        # Names/calls extracted lexically. Macros, aliases, traits, cfg and dispatch require manual tracing.
        if is_source and not binary:
            for n,line in enumerate(lines,1):
                if re.search(r'^\s*(?:(?:pub(?:\([^)]*\))?|async|unsafe|extern\s+"[^"]+")\s+)*(?:fn|struct|enum|trait|type|mod)\s+\w+|^\s*(?:async\s+)?def\s+\w+|^\s*class\s+\w+',line):
                    defs.append({'line':n,'declaration':line.strip()[:240]})
                    symbols.append({'file':path,'line':n,'declaration':line.strip()})
                if re.search(r'^\s*(?:use|import|from|mod)\s',line): imports.append({'line':n,'text':line.strip()[:220]})
                if re.search(r'\.await|tokio::spawn|spawn_blocking|spawn_local|Mutex|RwLock|mpsc::|broadcast::|watch::|CREATE TABLE|INSERT INTO|UPDATE |DELETE FROM|timeout\(|try_send|std::env|env::var|#\[cfg',line):
                    markers.append({'line':n,'text':line.strip()[:250]})
                for table in re.findall(r'CREATE TABLE(?: IF NOT EXISTS)?\s+(\w+)',line,re.I): tables.append({'table':table,'file':path,'line':n})
                if re.search(r'if __name__|^\s*(async )?fn main|^\s*function\s+|^param\(',line): script_entries.append({'file':path,'line':n,'entry':line.strip()})
        row={'path':path,'category':cat,'subsystem':subsystem,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),'lines':len(lines),'method':'hash/classification only' if not is_source or binary else 'all lines scanned for declarations/imports/control/storage markers','review_depth':'mechanical; see source-cited narrative for semantic tracing','declarations':len(defs),'markers':len(markers)}
        rows.append(row)
        if is_source and not binary: atlas.append({'file':path,'category':cat,'definitions':defs,'imports':imports,'control_storage_markers':markers})
    HERE.mkdir(parents=True,exist_ok=True)
    with (HERE/'coverage.csv').open('w',newline='',encoding='utf-8') as f:
        w=csv.DictWriter(f,fieldnames=list(rows[0]));w.writeheader();w.writerows(rows)
    for name,obj in [('source-atlas.json',atlas),('symbols.json',symbols),('storage-tables.json',tables),('script-entrypoints.json',script_entries)]:
        rendered = ('[\n'+',\n'.join(json.dumps(item,separators=(',',':')) for item in obj)+'\n]') if name in {'source-atlas.json','symbols.json'} else json.dumps(obj,indent=2)
        (HERE/name).write_text(rendered+'\n',encoding='utf-8')
    metadata=json.loads(subprocess.check_output(['cargo','metadata','--no-deps','--format-version','1','--manifest-path','engine/Cargo.toml'],cwd=ROOT))
    packages=[]
    for p in metadata['packages']:
        packages.append({'name':p['name'],'manifest':Path(p['manifest_path']).relative_to(ROOT).as_posix(),'targets':[{'name':t['name'],'kind':t['kind'],'source':Path(t['src_path']).relative_to(ROOT).as_posix()} for t in p['targets']],'internal_dependencies':[{'name':d['name'],'kind':d['kind'],'optional':d['optional']} for d in p['dependencies'] if d.get('path')]})
    (HERE/'packages.json').write_text(json.dumps(packages,indent=2)+'\n',encoding='utf-8')
    summary={'baseline':BASE,'branch':git('branch','--show-current').decode().strip(),'tracked_files':len(rows),'categories':dict(collections.Counter(r['category'] for r in rows)),'text_artifacts_scanned':len(atlas),'declarations':len(symbols),'table_declarations':len(tables),'binary_targets':[t for p in packages for t in p['targets'] if 'bin' in t['kind']],'limitations':['lexical index is not a resolved call graph','documentation prose excluded from behavioral evidence','all bytes inventoried; not every statement manually verified','untracked pre-existing files outside baseline'],'untracked_at_capture':git('ls-files','--others','--exclude-standard').decode().splitlines()}
    (HERE/'baseline.json').write_text(json.dumps(summary,indent=2)+'\n',encoding='utf-8')
    print(json.dumps({k:v for k,v in summary.items() if k!='untracked_at_capture'},indent=2))

if __name__=='__main__': main()
