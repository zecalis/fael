#!/usr/bin/env python3
"""PLAN-fael-onoff chunk 2: fold every cohort's screen into one task set.

Each <out-dir>/cohort-<n>/prescreen/*.jsonl holds one screen verdict per
sheet position n (schema: scripts/onoff/screen-brief.md). This maps n back to
the commit sha through cohort-<n>/order.json, checks every verdict is
complete and names a row the candidate really has, then rewrites
<out-dir>/screened.jsonl (what mine.py skips next time) and
<out-dir>/passed.json: the passes in cohort then sheet order, at most CAP
tasks per row so one hub row cannot dominate the set.

Usage: scripts/onoff/tally.py <out-dir>
"""
import collections
import glob
import json
import os
import sys

CAP = 2


def cohort(d):
    order = json.load(open(os.path.join(d, "order.json")))
    cands = {c["sha"]: c for line in open(os.path.join(d, "candidates.jsonl"))
             for c in [json.loads(line)]}
    got = {}
    for f in glob.glob(os.path.join(d, "prescreen", "*.jsonl")):
        for line in open(f):
            if line.strip():
                r = json.loads(line)
                got[r["n"]] = r
    missing = [n for n in range(1, len(order) + 1) if n not in got]
    if missing:
        sys.exit(f"{d}: no verdict for {missing}")
    out = []
    for n in range(1, len(order) + 1):
        r, c = got[n], cands[order[n - 1]]
        ids = {h["id"] for h in c["rows"]}
        if r["verdict"] == "pass" and r["row"] not in ids:
            sys.exit(f"{d} #{n}: row {r['row']} is not on commit {c['sha'][:10]}")
        out.append({**r, "sha": c["sha"], "pr": c["pr"]})
    return out


def main(out_dir):
    dirs = sorted(glob.glob(os.path.join(out_dir, "cohort-*")),
                  key=lambda d: int(d.rsplit("-", 1)[1]))
    rows, used, passed = [], collections.Counter(), []
    for d in dirs:
        name = os.path.basename(d)
        res = [{**r, "cohort": name} for r in cohort(d)]
        rows += res
        print(name, len(res),
              dict(collections.Counter(r["reason"] or "pass" for r in res)))
        for r in res:
            if r["verdict"] != "pass":
                continue
            if used[r["row"]] >= CAP:
                print(f"  cap: #{r['pr']} ({r['row']} already in {CAP} tasks)")
                continue
            used[r["row"]] += 1
            passed.append(r)
    with open(os.path.join(out_dir, "screened.jsonl"), "w") as f:
        for r in rows:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")
    json.dump(passed, open(os.path.join(out_dir, "passed.json"), "w"),
              ensure_ascii=False, indent=1)
    print(f"screened {len(rows)} · tasks after cap {len(passed)}")


if __name__ == "__main__":
    main(sys.argv[1])
