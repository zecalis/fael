#!/usr/bin/env python3
"""PLAN-fael-row-retire m2: stamped retires (`shown`) per source, split by
target from:user vs agent, plus overturned bumps — a stamped bump whose target
is closed or superseded later. Read each overturned row: only one that was
already wrong at bump time counts against (b). Totals match `fael stats --json`
said.held.acted when run over every repo with usage.
Retires stamped with a bulk sha (one that closed >= 20 rows) are cut, as in m1.
Usage: m2_overturn.py <git-common-dir>/fael/log
"""
import json,sys,glob
from collections import Counter,defaultdict
L=sys.argv[1]
rows,closes={},[]
for f in glob.glob(L+'/**/*.jsonl',recursive=True):
    for line in open(f):
        try: r=json.loads(line)
        except: continue
        (closes.append(r) if f.endswith('.close.jsonl') else rows.__setitem__(r['id'],r))
rev={r['restores'] for r in rows.values() if r.get('restores')}
shacnt=Counter(c.get('sha') for c in closes)
bulk={s for s,n in shacnt.items() if s and n>=20}
retire=defaultdict(list)  # target -> [(ts,kind,text,shown,sha)]
for c in closes:
    if c.get('ref'): retire[c['ref']].append((c['ts'],'close',c.get('text',''),c.get('shown'),c.get('sha')))
for r in rows.values():
    if r.get('supersedes') and r['id'] not in rev: retire[r['supersedes']].append((r['ts'],'supersede',r.get('text',''),r.get('shown'),r.get('sha')))
stamped=[r for r in rows.values() if r.get('shown')]+[c for c in closes if c.get('shown')]
if not stamped: sys.exit('no stamped retires')
since=min(r['ts'] for r in stamped); print('window since',since,'bulk shas',sorted(bulk))
user=lambda i: rows.get(i,{}).get('from')=='user'
kind=lambda i: rows.get(i,{}).get('kind','?')
tab=defaultdict(Counter); over=[]
for r in rows.values():
    t=r.get('bumps')
    if not t or not r.get('shown') or r['ts']<since or r.get('sha') in bulk: continue
    for s in r['shown']:
        tab[s]['bump']+=1; tab[s]['bump_user' if user(t) else 'bump_agent']+=1
    later=[e for e in retire.get(t,[]) if e[0]>r['ts']]
    if later:
        e=min(later); over.append((t,kind(t),user(t),r['shown'],r['ts'][:16],e[1],e[0][:16],e[4] in bulk,e[2][:110]))
for c in [c for c in closes if c.get('shown') and c.get('sha') not in bulk]:
    for s in c['shown']: tab[s]['close']+=1; tab[s]['close_user' if user(c.get('ref')) else 'close_agent']+=1
for r in rows.values():
    if r.get('supersedes') and r.get('shown') and r['id'] not in rev and r.get('sha') not in bulk:
        for s in r['shown']: tab[s]['supersede']+=1
for s,c in sorted(tab.items()): print(s,dict(c))
nb=sum(1 for r in rows.values() if r.get('bumps') and r.get('shown') and r['ts']>=since and r.get('sha') not in bulk)
tg={r['bumps'] for r in rows.values() if r.get('bumps') and r.get('shown') and r['ts']>=since and r.get('sha') not in bulk}
print(f'stamped bumps {nb}, distinct targets {len(tg)}, user targets {sum(map(user,tg))}, kinds {Counter(map(kind,tg))}')
ot={o[0] for o in over}
print(f'overturned bumps {len(over)} (distinct targets {len(ot)}; bulk-sha {sum(o[7] for o in over)})')
for o in sorted(over,key=lambda o:o[4]): print(' ',*o)
print('stamped closes:')
for c in closes:
    if c.get('shown'): print(' ',c['ts'][:16],c['ref'],kind(c['ref']),'user' if user(c['ref']) else '',c['shown'],c.get('text','')[:120])
