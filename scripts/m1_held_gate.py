#!/usr/bin/env python3
"""PLAN-fael-row-retire m1 gate: said.held / retired yield with bulk sweeps cut.

Reads what `fael stats` reads, nothing new: the usage log
($FAEL_STATE_DIR or ~/.local/state/fael/usage.jsonl) and each repo's ledger
(<git-common-dir>/fael/log/**/*.jsonl, closes in *.close.jsonl). Mirrors
fael-core/src/stats/{said,retire}.rs, so with --bulk 0 (no cut) and every repo
on the machine the numbers equal `fael stats --json --since`.

Definitions, mirrored from the Rust:
  event   = a row's close (ref), the row superseding it (supersedes), or a bump
            (bumps)                                  retire.rs:retire_events
  gone[id]= the EARLIEST event of id, whenever it happened   retire.rs:retire_times
  retired(id, t) = gone[id] in [t, t + 24h]                  said.rs:retired
  said.held earned = a usage line carries said:[{kind:"held",key:id}] at t and
            retired(id, t); one tally per said entry          said.rs:214
  retired.pushed   = distinct ids in read/edit usage lines; at_touch = of those,
            retired(id, t) for some push at t               retire.rs:retired
Bulk cut: a sha that closed >= --bulk distinct rows is a sweep; every close,
supersede and bump stamped with it is dropped BEFORE gone[] is taken (so a row
swept at one sha but later handled for real is judged on its real event).
Own/other: row.session stem == usage line session stem -> own (capture.rs:mine);
a row with no session is "unknown" (the ledger cannot say who wrote it).
Supersede edges a `restores` row reverted are skipped, as stats does.
Repos: a usage line belongs to the repo whose ledger holds the id it names, so
deleted worktrees still resolve.
"""
import argparse, json, os, random, subprocess, sys
from collections import Counter, defaultdict
from datetime import datetime, timedelta, timezone
from pathlib import Path

DAY = 24 * 3600 * 1000


def ms(ts):
    try:
        return int(datetime.fromisoformat(ts.replace("Z", "+00:00")).timestamp() * 1000)
    except (AttributeError, ValueError):
        return None


def stem(s):
    return Path(s).stem if s else None


def events(rows, closes):
    """[(id, ms, kind, sha, event_row_id)] from close rows, supersedes, bumps."""
    out = []
    rev = {r["restores"] for r in rows if r.get("restores")}  # reverted supersede edges
    for c in closes:
        if c.get("ref") and ms(c.get("ts")):
            out.append((c["ref"], ms(c["ts"]), "close", c.get("sha"), c["id"]))
    for r in rows:
        t = ms(r.get("ts"))
        if not t:
            continue
        if r.get("supersedes") and r["id"] not in rev:
            out.append((r["supersedes"], t, "supersede", r.get("sha"), r["id"]))
        if r.get("bumps"):
            out.append((r["bumps"], t, "bump", r.get("sha"), r["id"]))
        f = r.get("closed")  # a close `fael compact` folded into its row
        if isinstance(f, dict) and ms(f.get("ts")):
            out.append((r["id"], ms(f["ts"]), "close", f.get("sha"), f.get("id", "")))
    return out


def bulk_shas(evs, n):
    """sha -> distinct rows it closed, for shas that closed >= n rows."""
    per = defaultdict(set)
    for id_, _, kind, sha, _ in evs:
        if kind == "close" and sha:
            per[sha[:7]].add(id_)
    return {s: len(v) for s, v in per.items() if n and len(v) >= n}


def cut(evs, bulk):
    return [e for e in evs if not (e[3] and e[3][:7] in bulk)]


def gone(evs):
    """id -> earliest (ms, kind, sha, event_row_id)."""
    g = {}
    for id_, t, k, sha, eid in evs:
        if id_ not in g or t < g[id_][0]:
            g[id_] = (t, k, sha, eid)
    return g


def later(evs):
    """id -> sorted [ms] of every event (the --any variant: not only the earliest)."""
    out = defaultdict(list)
    for id_, t, *_ in evs:
        out[id_].append(t)
    return out


def retired_any(L, id_, t):
    return any(0 <= x - t <= DAY for x in L.get(id_, ()))


def retired(g, id_, t):
    e = g.get(id_)
    return e is not None and 0 <= e[0] - t <= DAY


def held_tally(held, g, rowsby):
    """held: [(ms, session_stem, id)] -> {bucket: [said, earned]}; bucket all/own/other/unknown."""
    out = defaultdict(lambda: [0, 0])
    for t, sess, id_ in held:
        w = (rowsby.get(id_) or {}).get("session")
        b = "unknown" if not w else ("own" if w == sess else "other")
        ok = retired(g, id_, t)
        for k in ("all", b):
            out[k][0] += 1
            out[k][1] += ok
    return out


def retire_tally(pushes, g):
    """pushes: [(ms, id)] -> (pushed, at_touch)."""
    pushed, hit = set(), set()
    for t, id_ in pushes:
        pushed.add(id_)
        if retired(g, id_, t):
            hit.add(id_)
    return len(pushed), len(hit)


def load_ledger(repo):
    common = subprocess.run(
        ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
        cwd=repo, capture_output=True, text=True, check=True).stdout.strip()
    rows, closes = [], []
    for p in sorted(Path(common, "fael", "log").rglob("*.jsonl")):
        dst = closes if p.name.endswith(".close.jsonl") else rows
        for line in p.read_text(errors="replace").splitlines():
            try:
                dst.append(json.loads(line))
            except ValueError:
                pass
    return rows, closes


def load_usage():
    d = os.environ.get("FAEL_STATE_DIR") or str(Path.home() / ".local/state/fael")
    for line in Path(d, "usage.jsonl").read_text(errors="replace").splitlines():
        try:
            v = json.loads(line)
        except ValueError:
            continue
        if ms(v.get("ts")):
            yield v


def window(usage, ids, lo, hi):
    """(held, pushes) in [lo, hi) for ledger ids `ids`."""
    held, pushes = [], []
    for v in usage:
        t = ms(v["ts"])
        if not lo <= t < hi:
            continue
        for e in v.get("said") or []:
            if e.get("kind") == "held" and e.get("key") in ids:
                held.append((t, stem(v.get("session")), e["key"]))
        if v.get("event") in ("read", "edit"):
            pushes += [(t, i) for i in v.get("ids") or [] if i in ids]
    return held, pushes


def pct(a, b):
    return f"{a}/{b} = {100 * a / b:.0f}%" if b else f"{a}/{b}"


def selftest():
    cl = lambda i, r, sha, t="2026-10-15T00:00:00Z": {"id": i, "ref": r, "sha": sha, "ts": t}
    closes = [cl(f"c{i}", f"r{i}", "aaaaaaa1") for i in range(20)] + [cl("x", "r0", "bbbbbbb", "2026-10-16T00:00:00Z"), cl("y", "q", "ccccccc")]
    rows = [{"id": "b1", "ts": "2026-10-14T00:00:00Z", "bumps": "r1", "sha": "aaaaaaa2"}]
    evs = events(rows, closes)
    bulk = bulk_shas(evs, 20)
    assert bulk == {"aaaaaaa": 20}, bulk
    assert bulk_shas(evs, 0) == {} and bulk_shas(evs, 21) == {}
    kept = cut(evs, bulk)
    assert {e[0] for e in kept} == {"r0", "q"}, kept  # r1's bump shares the 7-char prefix: cut
    g = gone(kept)
    assert g["r0"][2] == "bbbbbbb"  # swept at aaaaaaa, judged on its later real close
    t = ms("2026-10-15T12:00:00Z")
    assert retired(g, "r0", t) and not retired(g, "r0", ms("2026-10-17T00:00:00Z"))
    assert not retired(gone(evs), "r5", ms("2026-10-16T00:00:00Z")) and retired(gone(evs), "r5", t - DAY // 2)
    h = held_tally([(t, "s1", "r0"), (t, "s2", "r0"), (t, "s1", "q")], g, {"r0": {"session": "s1"}})
    assert h["own"] == [1, 1] and h["other"] == [1, 1] and h["unknown"] == [1, 0] and h["all"] == [3, 2], dict(h)
    assert retire_tally([(t, "r0"), (t, "r0"), (t, "zz")], g) == (2, 1)
    print("selftest ok")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--since")
    ap.add_argument("--until")
    ap.add_argument("--repo", action="append", help="repeatable; default: cwd")
    ap.add_argument("--bulk", type=int, default=20, help="cut shas that closed >= N rows (0 = no cut)")
    ap.add_argument("--sample", type=int, default=20)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()
    if a.selftest:
        return selftest()
    if not a.since:
        ap.error("--since is required")
    lo = ms(a.since + "T00:00:00Z" if len(a.since) == 10 else a.since)
    hi = ms(a.until + "T00:00:00Z" if a.until and len(a.until) == 10 else a.until) if a.until else int(datetime.now(timezone.utc).timestamp() * 1000)
    prev = (lo - (hi - lo), lo)
    usage = list(load_usage())
    for repo in a.repo or ["."]:
        rows, closes = load_ledger(repo)
        rowsby = {r["id"]: r for r in rows}
        evs = events(rows, closes)
        bulk = bulk_shas(evs, a.bulk)
        print(f"\n===== {repo}  window {a.since}..{a.until or 'now'}  vs equal window before =====")
        print(f"bulk shas cut (>= {a.bulk} rows): " + (", ".join(f"{s} ({n} rows)" for s, n in sorted(bulk.items(), key=lambda x: -x[1])) or "none"))
        print(f"events: {len(evs)} total, {len(evs) - len(cut(evs, bulk))} cut")
        ids = set(rowsby)
        res = {}
        for label, evset in (("raw", evs), ("cut", cut(evs, bulk))):
            g = gone(evset)
            for wn, (l, h) in (("window", (lo, hi)), ("before", prev)):
                held, pushes = window(usage, ids, l, h)
                res[label, wn] = (held_tally(held, g, rowsby), retire_tally(pushes, g))
        for label in ("raw", "cut"):
            print(f"\n-- {label} --")
            L = later(evs if label == "raw" else cut(evs, bulk))
            for wn, (l, h) in (("window", (lo, hi)), ("before", prev)):
                hh, _ = window(usage, ids, l, h)
                n = sum(retired_any(L, i, t) for t, _, i in hh)
                print(f"[{wn}] (variant, NOT stats' rule) held earned if ANY event lands in 24h, not only the earliest: {pct(n, len(hh))}")
            for wn in ("window", "before"):
                ht, (p, at) = res[label, wn]
                print(f"[{wn}] said.held earned/said: {pct(ht['all'][1], ht['all'][0])}   own {pct(ht['own'][1], ht['own'][0])}  other {pct(ht['other'][1], ht['other'][0])}  unknown(no row session) {pct(ht['unknown'][1], ht['unknown'][0])}")
                print(f"[{wn}] retired.at_touch/pushed: {pct(at, p)}")
        # sample: closes (not bulk) that landed within 24h after a Held line named the row
        g = gone(cut(evs, bulk))
        held, _ = window(usage, ids, lo, hi)
        kinds = Counter()
        cand = {}
        for t, _, id_ in held:
            if retired(g, id_, t):
                kinds[g[id_][1]] += 1
                if g[id_][1] == "close":
                    cand.setdefault(id_, (t, g[id_]))
        print(f"\nearned held lines by first event (bulk cut): {dict(kinds)}")
        pick = random.Random(a.seed).sample(sorted(cand), min(a.sample, len(cand)))
        cl = {c["id"]: c for c in closes}
        print(f"\nsample {len(pick)} of {len(cand)} distinct rows closed after a Held line (seed {a.seed}):")
        for id_ in pick:
            r, (t, e) = rowsby[id_], cand[id_]
            txt = (cl.get(e[3]) or {}).get("text") or (r.get("closed") or {}).get("text") or ""
            print(f"- id {id_}  kind {r.get('kind')}  closing sha {e[2]}  held->close {(e[0] - t) / 3600000:.1f}h\n  title: {r.get('title') or r.get('text', '')[:100]}\n  files: {', '.join(r.get('files') or [])}\n  close: {txt}")


if __name__ == "__main__":
    sys.exit(main())
