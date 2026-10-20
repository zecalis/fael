#!/usr/bin/env python3
"""PLAN-fael-row-retire m2: open decisions/notes whose stamped files changed in
part (held) vs all (all-changed), split by from:user. An approximation of
`fael stats` lifecycle (no rename aliases, no retired-note proof): fael read 31
vs lifecycle 33 decisions on 2026-10-21.
Usage (cwd = repo root): m2_held_split.py <repo-root> <git-common-dir>/fael/log
"""
import json,glob,subprocess,sys,os
from collections import Counter
root,L=sys.argv[1],sys.argv[2]
rows,gone={},set()
for f in glob.glob(L+'/**/*.jsonl',recursive=True):
    for l in open(f):
        try: r=json.loads(l)
        except: continue
        if f.endswith('.close.jsonl'):
            if r.get('ref'): gone.add(r['ref'])
        else: rows[r['id']]=r
rev={r['restores'] for r in rows.values() if r.get('restores')}
for r in rows.values():
    if r.get('supersedes') and r['id'] not in rev: gone.add(r['supersedes'])
cache={}
def changed(f,want):
    p=os.path.join(root,f)
    if not os.path.isfile(p): return None
    if p not in cache: cache[p]=subprocess.run(['git','hash-object',p],capture_output=True,text=True).stdout.strip()
    return not cache[p].startswith(want)
fh={i:r.get('fh') for i,r in rows.items()}
for r in sorted(rows.values(),key=lambda r:r['ts']):
    if r.get('bumps') and isinstance(r.get('fh'),dict) and isinstance(fh.get(r['bumps']),dict): fh[r['bumps']]={**fh[r['bumps']],**r['fh']}
c=Counter()
for r in rows.values():
    if r['id'] in gone or r.get('kind') not in ('decision','note') or not isinstance(fh.get(r['id']),dict) or r.get('held'): continue
    v=[changed(f,w) for f,w in fh[r['id']].items() if isinstance(w,str)]
    v=[bool(x) for x in v]
    if any(v) and not all(v): c[(r['kind'],'user' if r.get('from')=='user' else 'agent')]+=1
    elif v and all(v): c[(r['kind'],'all-changed')]+=1
print(dict(c))
