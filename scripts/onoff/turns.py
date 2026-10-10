#!/usr/bin/env python3
"""PLAN-fael-carry-reach chunk 1: hold two replay.py runs (`--turns commit`,
`--turns file`) to the same inputs and say per gold pair why carry missed it.

- reproduce: the commit run's pairs match push-noise's baseline artifact on
  every field the baseline has (and its orders, when the run is full)
- invariant: per pair and order, both runs share snapshot_sha256, tip, the
  edited file order and carry's candidates; one fael binary
- deterministic: a second file run (--again) has the same aggregate sha
- trace: each lost gold's reason is found again in the raw events
- table: reach (gold closed issues said) and cost (carry a/b/unlabeled,
  check/merge/promote lines) per turn mode × order

Exits 1 when any check fails. Usage: scripts/onoff/turns.py <results>
<baseline.json> <commit dir> <file dir> [--again <file dir>] [--json <out>]

`turns.py budget <base commit> <base file> <new commit> <new file>` holds a
carry rule change (chunk 2) to the user's budget (decision
carry-reach:chunk2-budget) against the same runs on the binary before it.
"""
import argparse
import glob
import json
import os
import sys
from collections import Counter

ORDERS = ("path", "reverse")
SAME = ("snapshot_sha256", "tip", "files", "candidates")
# the gold pairs newest-per-file kept from carry in chunk 1 (sha prefixes)
HIDDEN = ("dc060de8", "39fd39f6", "cd9e7b03")


def run(d):
    """(aggregate doc, {order: {(sha, row): [events]}}) of one replay.py out dir."""
    doc = json.load(open(*glob.glob(os.path.join(d, "*.json"))))
    ev = {}
    for o in ORDERS:
        ev[o] = {}
        for l in open(os.path.join(d, f"events-{o}.jsonl")):
            e = json.loads(l)
            ev[o].setdefault((e["sha"], e["row"]), []).append(e)
    return doc, ev


def reproduce(base, got):
    """Fields the baseline has that the commit run says otherwise."""
    bad = []
    for b, g in zip(base["pairs"], got["pairs"]):
        for k in b:
            if k not in ORDERS and b[k] != g.get(k):
                bad.append(f"{b['sha'][:8]} {k}")
        for o in ORDERS:
            bad += [f"{b['sha'][:8]} {o}.{k}" for k in b[o] if b[o][k] != g[o].get(k)]
    if len(got["pairs"]) == len(base["pairs"]):
        bad += [f"orders.{o}" for o in ORDERS if got["orders"][o] != base["orders"][o]]
    return bad


def invariant(c, f):
    bad = [] if c["meta"]["fael_sha256"] == f["meta"]["fael_sha256"] else ["fael_sha256"]
    for x, y in zip(c["pairs"], f["pairs"]):
        bad += [f"{x['sha'][:8]} {o}.{k}" for o in ORDERS for k in SAME if x[o][k] != y[o][k]]
    return bad + (["pair count"] if len(c["pairs"]) != len(f["pairs"]) else [])


def pos(files, e):
    """Where an event sits in the edit order; -1 before the first edit."""
    return files.index(e["files"][0]) if e.get("files") else -1


def trace(p, o, events, overlap, mode):
    """(ok, evidence): the lost gold's reason found in the raw events."""
    x, r = p[o], p[o]["reason"]
    files, cands = x["files"], x["candidates"]
    turn = lambda f: files.index(f) if mode == "file" else 0
    at = [f for f in overlap if f in files]
    said = [(e, s) for e in events for s in e.get("said", [])]
    kind, _, key = r.partition(":")
    if kind == "close_names_no_fix":
        return not p["guard_precheck"]["names_fix"], p["guard_precheck"]["close"]
    if kind == "carry_spent_this_turn":
        hit = [(e["files"][0]) for e, s in said if s["kind"] == "carry" and s.get("key") == key
               and any(e["turn"] == turn(f) and pos(files, e) < files.index(f) for f in at)]
        return bool(hit), f"{key} carried at {hit[:1]} earlier in the gold's turn"
    if kind == "not_newest_on_file":
        newer = [f for f in at if key in cands.get(f, []) and p["row"] in cands[f]
                 and key > p["row"]]
        hit = [(e["files"][0]) for e, s in said if s["kind"] == "carry" and s.get("key") == key
               and any(pos(files, e) <= files.index(f) for f in newer)]
        return bool(newer and hit), f"{key} newer on {newer[:1]}, carried at {hit[:1]}"
    if kind == "turn_spent":
        k, _, i = key.partition(":")
        hit = [(e["files"][0]) for e, s in said if s["kind"] == k and str(s.get("key")) == i
               and any(e["turn"] == turn(f) and pos(files, e) <= files.index(f) for f in at)]
        return bool(hit), f"{key} said at {hit[:1]} in the gold's turn"
    return False, r


def cost(events):
    """Per-turn lines said, summed over every pair's session (as unlabeled)."""
    n = Counter()
    for (_, row), es in events.items():
        for e in es:
            for k in {s["kind"] for s in e.get("said", [])} & {"check", "merge", "promote"}:
                n[k] += 1
            for s in e.get("said", []):
                if s["kind"] == "carry":
                    n["carry"] += 1
                    n["carry_unlabeled"] += s.get("key") != row
    return {k: n[k] for k in ("carry", "carry_unlabeled", "check", "merge", "promote")}


def peaks(events):
    """Most carry lines in one edit, one turn and one session."""
    edit = turn = sess = 0
    for es in events.values():
        per = Counter()
        for e in es:
            n = sum(s["kind"] == "carry" for s in e.get("said", []))
            edit, per[e["turn"]] = max(edit, n), per[e["turn"]] + n
        turn, sess = max([turn, *per.values()]), max(sess, sum(per.values()))
    return {"edit": edit, "turn": turn, "session": sess}


def budget(base, new):
    """(row, ok) per budget item and turn mode × order; base/new: {mode: run()}."""
    out = []
    for m in base:
        (bd, bev), (nd, nev) = base[m], new[m]
        bp = {(p["sha"], p["row"]): p for p in bd["aggregate"]["pairs"]}
        for o in ORDERS:
            gold = [p for p in nd["aggregate"]["pairs"] if p["label"] == "gold"]
            lost = [p["sha"][:8] for p in gold if bp[p["sha"], p["row"]][o]["warned"]
                    and not p[o]["warned"]]
            out.append((f"{m}/{o} gold lost {lost}", not lost))
            if m == "file":
                got = [p["sha"][:8] for p in gold if p["sha"][:8] in HIDDEN and p[o]["warned"]]
                out.append((f"{m}/{o} hidden gold reached {got}", bool(got)))
            bc, nc = cost(bev[o]), cost(nev[o])
            out.append((f"{m}/{o} carry {bc['carry']} -> {nc['carry']}",
                        nc["carry"] <= bc["carry"] * 1.2))
            for k in ("check", "merge", "promote"):
                out.append((f"{m}/{o} {k} {bc[k]} -> {nc[k]}",
                            abs(nc[k] - bc[k]) <= max(bc[k] * 0.1, 3)))
            bk, nk = peaks(bev[o]), peaks(nev[o])
            out.append((f"{m}/{o} carry max edit/turn/session {bk} -> {nk}",
                        nk["edit"] <= 1 and nk["turn"] <= 1 and nk["session"] <= bk["session"]))
    return out


def main():
    if sys.argv[1:2] == ["budget"]:
        dirs = sys.argv[2:6]
        rows = budget({"commit": run(dirs[0]), "file": run(dirs[1])},
                      {"commit": run(dirs[2]), "file": run(dirs[3])})
        for r, ok in rows:
            print(("ok   " if ok else "OVER ") + r)
        sys.exit(0 if all(ok for _, ok in rows) else 1)
    ap = argparse.ArgumentParser()
    ap.add_argument("results")
    ap.add_argument("baseline")
    ap.add_argument("commit")
    ap.add_argument("file")
    ap.add_argument("--again")
    ap.add_argument("--json")
    a = ap.parse_args()
    cands = {c["sha"]: c for c in map(json.loads, open(
        os.path.join(a.results, "cohort-1", "candidates.jsonl")))}
    base = json.load(open(a.baseline))["aggregate"]
    runs = {"commit": run(a.commit), "file": run(a.file)}
    agg = {m: d["aggregate"] for m, (d, _) in runs.items()}
    out = {"reproduce": reproduce(base, agg["commit"]),
           "invariant": invariant(agg["commit"], agg["file"]),
           "errors": {m: agg[m]["errors"] for m in agg if agg[m]["errors"]},
           "turns_meta": {m: agg[m]["meta"].get("turns") for m in agg}}
    if a.again:
        out["deterministic"] = run(a.again)[0]["aggregate_sha256"] == runs["file"][0]["aggregate_sha256"]
    out["table"], out["gold"], out["trace_fail"] = {}, [], []
    for m, (_, ev) in runs.items():
        for o in ORDERS:
            t = agg[m]["orders"][o]
            gold = t["gold"]["by_kind"]["closed_issue"]
            out["table"][f"{m}/{o}"] = {
                "gold_closed_reached": f"{gold[0]}/{gold[1]}",
                "carry_a": f"{t['a']['by_channel'].get('carry', 0)}/{t['a']['of']}",
                "carry_b": f"{t['b']['by_channel'].get('carry', 0)}/{t['b']['of']}",
                **cost(ev[o])}
            for p in agg[m]["pairs"]:
                if p["label"] != "gold" or p["kind"] != "closed_issue":
                    continue
                row = next(r for r in cands[p["sha"]]["rows"] if r["id"] == p["row"])
                g = {"mode": m, "order": o, "sha": p["sha"][:8], "row": p["row"][:10],
                     "said": p[o]["channel"] or "-", "reason": p[o].get("reason", "")}
                if not p[o]["warned"]:
                    g["trace_ok"], g["evidence"] = trace(p, o, ev[o].get((p["sha"], p["row"]), []),
                                                         row["overlap"], m)
                    if not g["trace_ok"] or g["reason"] == "unexplained":
                        out["trace_fail"].append(g)
                out["gold"].append(g)
    print(f"reproduce mismatches: {len(out['reproduce'])} {out['reproduce'][:5]}")
    print(f"invariant mismatches: {len(out['invariant'])} {out['invariant'][:5]}")
    print(f"errors: {out['errors']}  deterministic: {out.get('deterministic', 'not run')}")
    print("\n| turns/order | " + " | ".join(next(iter(out["table"].values()))) + " |")
    for k, v in out["table"].items():
        print(f"| {k} | " + " | ".join(str(x) for x in v.values()) + " |")
    print("\n| turns | order | sha | gold row | said | reason | trace |")
    for g in out["gold"]:
        print(f"| {g['mode']} | {g['order']} | {g['sha']} | {g['row']} | {g['said']} | "
              f"{g['reason']} | {g.get('trace_ok', '')} |")
    if a.json:
        json.dump(out, open(a.json, "w"), indent=1, ensure_ascii=False)
    bad = out["reproduce"] or out["invariant"] or out["errors"] or out["trace_fail"] \
        or out.get("deterministic") is False
    sys.exit(1 if bad else 0)


def selftest():
    """trace finds each reason in the events, and fails one it cannot find."""
    files = ["x.ts", "y.ts"]
    ev = [{"files": ["x.ts"], "turn": 0, "said": [{"kind": "carry", "key": "01C"}]},
          {"files": ["y.ts"], "turn": 0, "said": [{"kind": "merge", "key": "01D"}]}]
    p = lambda r, cands=None: {"row": "01B", "guard_precheck": {"names_fix": True, "close": ""},
                               "path": {"reason": r, "files": files, "candidates": cands or {}}}
    t = lambda r, cands=None, mode="commit": trace(p(r, cands), "path", ev, ["y.ts"], mode)[0]
    assert t("carry_spent_this_turn:01C") and not t("carry_spent_this_turn:01X")
    # a turn per file: x's carry spent turn 0, not y's
    assert not t("carry_spent_this_turn:01C", mode="file")
    assert t("not_newest_on_file:01C", {"y.ts": ["01C", "01B"]})
    assert not t("not_newest_on_file:01C", {"y.ts": ["01B"]})
    assert t("turn_spent:merge:01D") and not t("unexplained")
    assert not t("close_names_no_fix")
    tab = {"path": {"n": 1}, "reverse": {"n": 1}}
    base = {"orders": tab, "pairs": [{"sha": "s", "k": 1, "path": {"w": 1}, "reverse": {"w": 2}}]}
    assert reproduce(base, base) == []
    got = {"orders": tab | {"path": {"n": 2}},
           "pairs": [{"sha": "s", "k": 1, "path": {"w": 1, "new": 0}, "reverse": {"w": 3}}]}
    assert reproduce(base, got) == ["s reverse.w", "orders.path"]
    assert cost({("s", "01C"): ev}) == {"carry": 1, "carry_unlabeled": 0, "check": 0,
                                        "merge": 1, "promote": 0}
    two = ev + [{"files": ["y.ts"], "turn": 1, "said": [{"kind": "carry", "key": "01B"}]}]
    assert peaks({("s", "01C"): two}) == {"edit": 1, "turn": 1, "session": 2}
    pair = lambda w: {"sha": "dc060de8aa", "row": "01B", "label": "gold",
                      **{o: {"warned": w} for o in ORDERS}}
    r = lambda w, evs: {"commit": ({"aggregate": {"pairs": [pair(w)]}}, {o: evs for o in ORDERS}),
                        "file": ({"aggregate": {"pairs": [pair(w)]}}, {o: evs for o in ORDERS})}
    one = {("s", "01C"): ev}
    assert all(ok for _, ok in budget(r(False, one), r(True, one)))
    # gold lost, and a second carry in one turn: both over
    assert not budget(r(True, one), r(False, one))[0][1]
    over = {("s", "01C"): two[:2] + [dict(two[2], turn=0)]}
    assert not all(ok for _, ok in budget(r(True, one), r(True, over)))


if __name__ == "__main__":
    selftest()
    main()
