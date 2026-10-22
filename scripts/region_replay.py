#!/usr/bin/env python3
"""Replay past held asks against the region-scope design (decision 01M5H3Z66, held:region-scope).
Read-only: reads the fael logs, ~/.local/state/fael/usage.jsonl and git objects of fael/vela/board/crew.
Usage: python3 -I scripts/region_replay.py [--since RFC3339] [--until RFC3339] [--examples N] [-v]
 --since default = first stamped retire (m2 window start); --until default = none.
Asks: (A) `held` lines in usage.jsonl (edit/shell-edit/search events; file = row.files ∩ event files),
      (B) acted asks stamped `shown` done/start/add on bump/close/supersede events (file unknown: every
          stamped file whose whole-file blob moved between stamp and the act). Reported separately.
Verdict per (row,file): SILENT / CHANGED / TOKENLESS / UNRECOVERABLE (see classify()).
"""
import json, sys, os, re, glob, subprocess, collections
HOME = os.path.expanduser('~')
P = HOME + '/Project'
REPOS = {  # name -> (git root, fael log dir)
    'fael': (P + '/fael/fael', P + '/fael/fael/.git/fael/log'),
    'vela': (P + '/zecalis/vela/vela', P + '/zecalis/vela/vela/.git/fael/log'),
    'board': (P + '/zecalis/fael/board', P + '/zecalis/fael/board/.git/fael/log'),
    'crew': (P + '/zecalis/fael/crew/crew', P + '/zecalis/fael/crew/crew/.git/fael/log'),
}
USAGE = HOME + '/.local/state/fael/usage.jsonl'
MAXLINES, CAP, DRIFT = 12, 200, 10


def arg(n, d=None):
    return sys.argv[sys.argv.index(n) + 1] if n in sys.argv else d


UNTIL = arg('--until', '9')
NEX = int(arg('--examples', '5'))
VERB = '-v' in sys.argv


def repo_of(path):
    if '/zecalis/fael/board' in path: return 'board'
    if '/zecalis/fael/crew' in path: return 'crew'
    if '/zecalis/vela' in path or '/zecalis/wt-vela' in path: return 'vela'
    if '/Project/fael/' in path: return 'fael'
    return None


def git(repo, *a):
    r = subprocess.run(['git', '-C', REPOS[repo][0], *a], capture_output=True)
    return r.stdout if r.returncode == 0 else None


# ---- loading (as m2_overturn.py) ----
def load(L):
    rows, closes = {}, []
    for f in glob.glob(L + '/**/*.jsonl', recursive=True):
        for line in open(f):
            try: r = json.loads(line)
            except Exception: continue
            if r['ts'] > UNTIL: continue
            (closes.append(r) if f.endswith('.close.jsonl') else rows.__setitem__(r['id'], r))
    return rows, closes


D = {}
for name, (_, L) in REPOS.items():
    rows, closes = load(L)
    rev = {r['restores'] for r in rows.values() if r.get('restores')}
    shacnt = collections.Counter(c.get('sha') for c in closes)
    bulk = {s for s, n in shacnt.items() if s and n >= 20}
    ev = collections.defaultdict(list)  # target -> [(ts, type, sha, shown)]
    for c in closes:
        if c.get('ref'): ev[c['ref']].append((c['ts'], 'close', c.get('sha'), c.get('shown'), c))
    for r in rows.values():
        if r.get('bumps'): ev[r['bumps']].append((r['ts'], 'bump', r.get('sha'), r.get('shown'), r))
        if r.get('supersedes') and r['id'] not in rev:
            ev[r['supersedes']].append((r['ts'], 'supersede', r.get('sha'), r.get('shown'), r))
    for v in ev.values(): v.sort(key=lambda e: e[0])
    D[name] = dict(rows=rows, closes=closes, bulk=bulk, ev=ev)

stamped = [r['ts'] for d in D.values() for r in list(d['rows'].values()) + d['closes'] if r.get('shown')]
SINCE = arg('--since', min(stamped) if stamped else '0')


def stamp_at(repo, rid, T, strict=True):
    """Effective fh of row at time T: row fh merged with bumps before T."""
    d = D[repo]; fh = dict(d['rows'][rid].get('fh') or {}) if isinstance(d['rows'][rid].get('fh'), dict) else {}
    for e in d['ev'].get(rid, []):
        if e[1] == 'bump' and (e[0] < T) and isinstance(e[4].get('fh'), dict): fh.update(e[4]['fh'])
    return fh


# ---- file versions ----
_blob = {}
def blob(repo, h):
    k = (repo, h)
    if k not in _blob:
        o = git(repo, 'cat-file', 'blob', h)
        _blob[k] = o.decode('utf-8', 'replace') if o is not None else None
    return _blob[k]

def blob12(repo, spec):
    o = git(repo, 'rev-parse', spec)
    return o.decode().strip() if o else None

_hist = {}
def hist(repo, f):
    """[(commit time s, sha)] of every commit on any ref touching f, newest first (cached)."""
    if (repo, f) not in _hist:
        o = git(repo, 'log', '--all', '--format=%ct %H', '--', f)
        _hist[(repo, f)] = [(int(x.split()[0]), x.split()[1]) for x in (o.decode().splitlines() if o else [])]
    return _hist[(repo, f)]

def tsec(T):
    import datetime
    return datetime.datetime.fromisoformat(T.replace('Z', '+00:00')).timestamp()

def at_or_before(repo, f, T):
    """latest commit (any ref) at or before T touching f -> (blob text, blob hash)."""
    t = tsec(T)
    for ct, c in hist(repo, f):
        if ct <= t:
            h = blob12(repo, c + ':' + f)
            return (blob(repo, h), h) if h else None
    return None

def after(repo, f, T, hours=12):
    """earliest commit touching f in (T, T+hours]."""
    t = tsec(T)
    for ct, c in reversed(hist(repo, f)):
        if t < ct <= t + hours * 3600:
            h = blob12(repo, c + ':' + f)
            if h: return (blob(repo, h), h, c)
    return None


# ---- design ----
HUMP = re.compile(r'[a-z0-9][A-Z]')
WORD = re.compile(r'[A-Za-z_][A-Za-z0-9_]*(?:(?:::|\.)[A-Za-z_][A-Za-z0-9_]*)*')

def tokens(r):
    out = []
    for w in WORD.findall((r.get('text') or '') + ' ' + (r.get('title') or '')):
        if '_' in w or '::' in w or HUMP.search(w):
            t = re.split(r'::|\.', w)[-1]
            if len(t) >= 4 and t not in out: out.append(t)
    return out

def lines_of(text, tok):
    pat = re.compile(r'(?<![A-Za-z0-9_])' + re.escape(tok) + r'(?![A-Za-z0-9_])')
    L = text.split('\n')
    return L, [i for i, l in enumerate(L) if pat.search(l)]

def region(L, hits):
    keep = set()
    ind = lambda s: len(s) - len(s.lstrip())
    for i in hits:
        keep.add(i); base = ind(L[i]); n = 0; j = i + 1
        while j < len(L) and n < CAP:
            if L[j].strip():
                if ind(L[j]) <= base: break
                keep.add(j)
            j += 1; n += 1
    return sorted(keep)

def ranges(ix):
    out, s = [], None
    for i in ix:
        if s is None: s = p = i
        elif i == p + 1: p = i
        else: out.append((s + 1, p + 1)); s = p = i
    if s is not None: out.append((s + 1, p + 1))
    return ','.join(f'{a}-{b}' if a != b else str(a) for a, b in out)

def classify(row, old, new):
    """-> (verdict, info). old = file at stamp, new = file at ask."""
    toks = []
    for t in tokens(row):
        L, h = lines_of(old, t)
        if h and len(h) <= MAXLINES: toks.append(t)
    if not toks: return 'TOKENLESS', {'tokens': []}
    gone = [t for t in toks if not lines_of(new, t)[1]]
    Lo, Ln = old.split('\n'), new.split('\n')
    ro = region(Lo, sorted({i for t in toks for i in lines_of(old, t)[1]}))
    rn = region(Ln, sorted({i for t in toks for i in lines_of(new, t)[1]}))
    info = {'tokens': toks, 'gone': gone, 'old_rng': ranges(ro), 'new_rng': ranges(rn)}
    if gone: return 'CHANGED', info
    same = [Lo[i] for i in ro] == [Ln[i] for i in rn]
    return ('SILENT' if same else 'CHANGED'), info

AGG = ['CHANGED', 'SILENT', 'TOKENLESS', 'UNRECOVERABLE']


def judge(repo, rid, files, T, new_of):
    """Aggregate per-file verdicts. new_of(f)->(text,hash,tag)|None. Returns (verdict, details, tag)."""
    row = D[repo]['rows'][rid]; fh = stamp_at(repo, rid, T)
    res = []
    for f in files:
        want = fh.get(f)
        if not isinstance(want, str): res.append(('UNRECOVERABLE', {'why': 'no stamp'}, f, '')); continue
        old = blob(repo, want)
        if old is None: res.append(('UNRECOVERABLE', {'why': 'stamp blob gone'}, f, '')); continue
        n = new_of(f, want)
        if n is None: res.append(('UNRECOVERABLE', {'why': 'ask version missing'}, f, '')); continue
        text, h, tag = n
        if text is None or h.startswith(want) or want.startswith(h[:12]):
            res.append(('UNRECOVERABLE', {'why': 'file unchanged at ask version (uncommitted edit?)'}, f, tag)); continue
        v, info = classify(row, old, text); info['why'] = ''
        res.append((v, info, f, tag))
    for a in AGG:
        for v, info, f, tag in res:
            if v == a: return a, dict(info, file=f), tag
    return 'UNRECOVERABLE', {'why': 'no candidate file'}, ''


# ---- build asks ----
asks = []  # dict(repo,src,kind,row,ts,file...)

def A_new_of(repo, T):
    def g(f, want):
        r = at_or_before(repo, f, T)
        if r and not r[1].startswith(want):
            return (r[0], r[1], 'A:before')
        n = after(repo, f, T)
        if n and not n[1].startswith(want): return (n[0], n[1], 'A:next-commit')
        return (None, want, 'A:before') if r else None
    return g

seen = set()
for l in open(USAGE):
    try: e = json.loads(l)
    except Exception: continue
    if not (SINCE <= e['ts'] <= UNTIL): continue
    ids = [s['key'] for s in e.get('said', []) or [] if s.get('kind') == 'held']
    if not ids: continue
    repo = repo_of(e['repo'])
    if not repo: asks.append(dict(repo='?', src='A', row=ids[0], ts=e['ts'], v='UNRECOVERABLE', why='repo unmapped')); continue
    for rid in ids:
        if (rid, e['ts']) in seen or rid not in D[repo]['rows']:
            if rid not in D[repo]['rows'] and (rid, e['ts']) not in seen:
                asks.append(dict(repo=repo, src='A', row=rid, ts=e['ts'], v='UNRECOVERABLE', why='row not in repo log'))
            seen.add((rid, e['ts'])); continue
        seen.add((rid, e['ts']))
        r = D[repo]['rows'][rid]
        fh = stamp_at(repo, rid, e['ts'])
        # Stop->next push: the edited files of the push are not always the turn's; prefer row.files ∩ edited,
        # else every stamped file. Keep only files whose whole-file blob moved (the ask would name them).
        files = [f for f in r.get('files', []) if f in (e.get('files') or []) and f in fh] or list(fh)
        g = A_new_of(repo, e['ts'])
        files = [f for f in files if isinstance(fh.get(f), str) and (g(f, fh[f]) or (None,))[0] is not None]
        if not files: asks.append(dict(repo=repo, src='A', row=rid, ts=e['ts'], v='UNRECOVERABLE', why='no stamped file moved/recoverable')); continue
        v, info, tag = judge(repo, rid, files, e['ts'], A_new_of(repo, e['ts']))
        asks.append(dict(repo=repo, src='A', row=rid, ts=e['ts'], v=v, info=info, tag=tag))

for repo, d in D.items():  # B: acted asks done/start/add
    for rid, evs in d['ev'].items():
        for (ts, typ, sha, shown, obj) in evs:
            srcs = [s for s in (shown or []) if s in ('done', 'start', 'add')]
            if not srcs or not (SINCE <= ts <= UNTIL) or sha in d['bulk'] or rid not in d['rows']: continue
            fh0 = stamp_at(repo, rid, ts)
            files = [f for f in fh0]
            actfh = obj.get('fh') if typ == 'bump' and isinstance(obj.get('fh'), dict) else {}
            def new_of(f, want, sha=sha, actfh=actfh, ts=ts, repo=repo):
                h = actfh.get(f)
                if isinstance(h, str) and blob(repo, h) is not None: return (blob(repo, h), h, 'B:bump-fh')
                if sha:
                    hh = blob12(repo, f'{sha}:{f}')
                    if hh: return (blob(repo, hh), hh, 'B:act-sha')
                r = at_or_before(repo, f, ts)
                return (r[0], r[1], 'B:all-refs') if r else None
            # keep only files whose whole-file blob moved (the ask would name them)
            cand = []
            for f in files:
                n = new_of(f, fh0[f])
                if n and isinstance(fh0.get(f), str) and not n[1].startswith(fh0[f]): cand.append(f)
            if not cand:
                asks.append(dict(repo=repo, src='B', row=rid, ts=ts, v='UNRECOVERABLE', why='no stamped file moved/recoverable', srcs=srcs, act=typ)); continue
            v, info, tag = judge(repo, rid, cand, ts, new_of)
            asks.append(dict(repo=repo, src='B', row=rid, ts=ts, v=v, info=info, tag=tag, srcs=srcs, act=typ))


# ---- outcomes ----
def later(a):
    d = D.get(a['repo']);
    if not d: return []
    out = []
    for (ts, typ, sha, shown, obj) in d['ev'].get(a['row'], []):
        ok = ts >= a['ts'] if a['src'] == 'B' else ts > a['ts']
        if ok and typ in ('close', 'supersede'):
            out.append((ts, typ, sha in d['bulk']))
    return out

def drifted_before(a, close_ts):
    repo = a['repo']; d = D[repo]; row = d['rows'][a['row']]
    start = row['ts']
    for e in d['ev'].get(a['row'], []):
        if e[1] == 'bump' and e[0] < close_ts: start = max(start, e[0])
    files = [f.rstrip('/') for f in row.get('files', []) if not re.match(r'^[a-z][a-z0-9+.-]+:(?!//)', f)]
    if any(not f.endswith('.md') for f in files): files = [f for f in files if not f.endswith('.md')]
    if not files: return 0
    ref = 'origin/main' if git(repo, 'rev-parse', 'origin/main') else 'main'
    o = git(repo, 'log', ref, '--since=' + start, '--until=' + close_ts, '--format=%x01%H', '--name-only')
    n = 0
    for blk in (o.decode().split('\x01') if o else []):
        ps = [x for x in blk.split('\n')[1:] if x.strip()]
        if any(p in files or any(p.startswith(f + '/') for f in files) for p in ps): n += 1
    return n

# ---- report ----
def table(sel, title):
    print(f'\n== {title}')
    print(f"{'repo':6} {'asks':>5} {'SILENT':>7} {'CHANGED':>8} {'TOKENLESS':>10} {'UNRECOV':>8}  rows")
    tot = collections.Counter();
    for rp in list(REPOS) + ['?']:
        c = collections.Counter(a['v'] for a in sel if a['repo'] == rp)
        n = sum(c.values())
        if not n: continue
        rws = len({a['row'] for a in sel if a['repo'] == rp})
        print(f"{rp:6} {n:5} {c['SILENT']:7} {c['CHANGED']:8} {c['TOKENLESS']:10} {c['UNRECOVERABLE']:8}  {rws}")
        tot.update(c)
    n = sum(tot.values())
    print(f"{'ALL':6} {n:5} {tot['SILENT']:7} {tot['CHANGED']:8} {tot['TOKENLESS']:10} {tot['UNRECOVERABLE']:8}  {len({(a['repo'],a['row']) for a in sel})}")
    rec = n - tot['UNRECOVERABLE']
    print(f"   recoverable {rec}; SILENT share of recoverable {tot['SILENT']}/{rec} = {100*tot['SILENT']/max(rec,1):.1f}%; of all asks {100*tot['SILENT']/max(n,1):.1f}%")
    miss = [a for a in sel if a['v'] == 'SILENT' and [x for x in later(a) if not x[2]]]
    missb = [a for a in sel if a['v'] == 'SILENT' and not [x for x in later(a) if not x[2]] and later(a)]
    print(f"   MISS (SILENT, row later closed/superseded, bulk-sha cut) {len(miss)}/{tot['SILENT']} = {100*len(miss)/max(tot['SILENT'],1):.1f}% of SILENT  (excluded bulk-sha only: {len(missb)})")
    for a in miss: print('     MISS', a['repo'], a['row'][:10], a['ts'][:16], a['src'], later(a)[0][:2], D[a['repo']]['rows'][a['row']].get('title', '')[:60])
    tl = [a for a in sel if a['v'] == 'TOKENLESS' and [x for x in later(a) if not x[2]]]
    yes = 0
    for a in tl:
        x = [x for x in later(a) if not x[2]][0]
        dn = drifted_before(a, x[0]); yes += dn >= DRIFT
        if VERB: print('     TOKENLESS-closed', a['repo'], a['row'][:10], x[:2], 'commits', dn)
    print(f"   TOKENLESS later closed/superseded {len(tl)} (distinct rows {len({(a['repo'],a['row']) for a in tl})}): Drifted would have caught first: yes {yes}, no {len(tl)-yes}")
    tags = collections.Counter((a.get('tag') or '-') for a in sel if a['v'] != 'UNRECOVERABLE')
    why = collections.Counter(((a.get('info') or {}).get('why') or a.get('why') or '?') for a in sel if a['v'] == 'UNRECOVERABLE')
    print('   version source (recoverable):', dict(tags)); print('   unrecoverable why:', dict(why))
    return tot, len(miss)

print('window', SINCE, '..', UNTIL if UNTIL != '9' else 'open')
tA, _ = table([a for a in asks if a['src'] == 'A'], 'A: held lines (usage.jsonl), file = row.files ∩ edited')
tB, _ = table([a for a in asks if a['src'] == 'B'], 'B: acted done/start/add asks (file inferred)')
tot, miss = table(asks, 'ALL')
sil = tot['SILENT']; rec = sum(tot.values()) - tot['UNRECOVERABLE']
print(f"\nBAR  SILENT>=30% of asks: {100*sil/max(sum(tot.values()),1):.1f}% (of recoverable {100*sil/max(rec,1):.1f}%) -> {'PASS' if sil>=0.3*max(sum(tot.values()),1) else 'FAIL'}")
print(f"BAR  MISS<=10% of SILENT: {100*miss/max(sil,1):.1f}% -> {'PASS' if miss<=0.1*sil else 'FAIL'}")

def show(v):
    sel = [a for a in asks if a['v'] == v]
    step = max(len(sel) // NEX, 1)
    print(f'\n-- {NEX} examples of {v} ({len(sel)})')
    for a in sel[::step][:NEX]:
        i = a['info']; r = D[a['repo']]['rows'][a['row']]
        print(f"{a['repo']} {a['row'][:10]} {a['src']} {a['ts'][:16]} [{r['kind']}] \"{(r.get('title') or r['text'])[:70]}\"\n   file {i.get('file')} tokens {i['tokens'][:6]} gone {i.get('gone')}\n   region lines stamp[{i.get('old_rng','')[:70]}] ask[{i.get('new_rng','')[:70]}] ({a['tag']})")
show('SILENT'); show('CHANGED')


# ---- follow-up: were the later closes the act on a held ask? (--followup) ----
if '--followup' in sys.argv:
    import datetime
    mins = lambda a, b: (tsec(b) - tsec(a)) / 60
    print('\n== TOKENLESS asks by source')
    c = collections.Counter()
    for a in asks:
        if a['v'] != 'TOKENLESS': continue
        if a['src'] == 'A': c['Stop push (held usage line)'] += 1
        else:
            for s in a['srcs']: c['chunk ' + s if s != 'add' else 'add'] += 1
    print(dict(c), '(a B ask with several shown sources counts in each)')
    # session of each held usage line
    sess = collections.defaultdict(list)  # (repo,row) -> [(ts, session stem)]
    for l in open(USAGE):
        try: e = json.loads(l)
        except Exception: continue
        rp = repo_of(e['repo'])
        for s in e.get('said', []) or []:
            if s.get('kind') == 'held' and rp:
                sess[(rp, s['key'])].append((e['ts'], os.path.basename(e.get('session', '')).replace('.jsonl', '')))
    targets = {}
    for a in asks:
        if a['v'] == 'SILENT' or a['v'] == 'TOKENLESS':
            x = [x for x in later(a) if not x[2]]
            if x and (a['v'] == 'TOKENLESS' or True):
                if a['v'] == 'SILENT' or a['v'] == 'TOKENLESS': targets.setdefault((a['repo'], a['row']), []).append((a['v'], a['ts'], x[0]))
    print('\n== later close/supersede per row (SILENT misses + TOKENLESS-closed)')
    print('repo row verdict(asks) kind shown close_ts mins_since_last_held_ask same_session by text')
    for (rp, rid), lst in sorted(targets.items()):
        if not any(v == 'TOKENLESS' for v, _, _ in lst) and not any(v == 'SILENT' for v, _, _ in lst): continue
        ts, typ, _ = lst[0][2]
        obj = [e for e in D[rp]['ev'][rid] if e[0] == ts and e[1] == typ][0]
        shown, o = obj[3], obj[4]
        helds = [t for t, _ in sess.get((rp, rid), []) if t <= ts]
        asks_b = [a['ts'] for a in asks if a['repo'] == rp and a['row'] == rid and a['ts'] <= ts]
        last = max(helds + asks_b) if (helds or asks_b) else None
        sid = os.path.basename(o.get('session', '') or '').replace('.jsonl', '')
        same = any(t <= ts and s and (s == sid or s.startswith(sid[:8])) for t, s in sess.get((rp, rid), [])) if sid else False
        txt = o.get('text', '') if typ == 'close' else o.get('text', '')
        print(rp, rid[:10], '/'.join(sorted({v for v, _, _ in lst})), f'x{len(lst)}', typ, shown, ts[:16],
              'min=%.0f' % mins(last, ts) if last else 'min=none', 'same_session=%s' % same, o.get('by'), '|', txt[:160].replace('\n', ' '))
