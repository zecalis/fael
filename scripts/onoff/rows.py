#!/usr/bin/env python3
"""PLAN-fael-capture-yield: build a blind row-level screen sheet.

Rows in a window: issues closed and decisions written while the W PR commits
ending at <end-sha> landed on origin/main (PR commit = mine.main_commits()
subject ending in (#N)). A row superseded (edge not restored), or closed as
"superseded", by the window's end counts through its successor only. Each row is judged at its `at` commit: the squash
of the PR whose branch closed (issue) or wrote (decision) it, else main as it
stood at that moment.

Read-only on the target repo: the sheet's refs live in a bare copy under
<out-dir>/repo.git as tags at-<n>, so a screener never sees a sha, ts or PR.
Usage:
  scripts/onoff/rows.py <repo> <out-dir> <end-sha> <W>
  scripts/onoff/rows.py <repo> <out-dir> --ids <file>   (one row id per line)
Writes sheet.md (screen with row-brief.md) and key.json (n → row, kept from
the screener).
"""
import glob
import json
import os
import random
import re
import subprocess
import sys

sys.dont_write_bytecode = True  # no __pycache__ beside mine.py
import mine  # noqa: E402

SEED = 20261010


def ledger(repo):
    common = mine.git(repo, "rev-parse", "--git-common-dir").strip()
    every = []
    for f in glob.glob(os.path.join(repo, common, "fael", "log", "*", "*.jsonl")):
        every += [json.loads(line) for line in open(f)]
    every.sort(key=lambda r: mine.ts(r["ts"]))
    rows, closes = {}, {}
    for r in every:
        if r.get("ref"):
            closes.setdefault(r["ref"], r)  # first close by ts
        elif r.get("kind") in ("issue", "decision"):
            rows[r["id"]] = r
    return rows, closes, every


def gone_by(every, hi):
    """Rows superseded (edge not restored) or closed as "superseded" by hi."""
    early = [r for r in every if mine.ts(r["ts"]) <= hi]
    reverted = {r["restores"] for r in early if r.get("restores")}
    return ({r["supersedes"] for r in early
             if r.get("supersedes") and r["id"] not in reverted}
            | {r["ref"] for r in early
               if r.get("ref") and r["text"] == "superseded"})


def at_commit(event, pr_main, main):
    """The main commit a row's event is judged at."""
    t = mine.ts(event["ts"])
    later = [m for m in pr_main.get(event.get("branch"), []) if main[m] >= t]
    if later:
        return min(later, key=main.get)
    return max((s for s in main if main[s] <= t), key=main.get)


def main(repo, out_dir, *sel):
    rows, closes, every = ledger(repo)
    commits, times = mine.main_commits(repo)
    main_t = {sha: times[sha] for sha, *_ in commits}
    prs = json.loads(subprocess.run(
        ["gh", "pr", "list", "--state", "merged", "--limit", "1000",
         "--json", "number,headRefName"], cwd=repo, check=True,
        capture_output=True, text=True).stdout)
    branch_of = {p["number"]: p["headRefName"] for p in prs}
    pr_main = {}
    for sha, _, subject, _ in commits:
        m = re.search(r"\(#(\d+)\)$", subject)
        if m and int(m.group(1)) in branch_of:
            pr_main.setdefault(branch_of[int(m.group(1))], []).append(sha)

    if sel[0] == "--ids":
        ids = [l.strip() for l in open(sel[1]) if l.strip()]
    else:
        end, w = sel[0], int(sel[1])
        pr = [c[0] for c in commits if re.search(r"\(#\d+\)$", c[2])]
        i = next(i for i, s in enumerate(pr) if s.startswith(end))
        win = pr[i:i + w]
        hi, lo = times[win[0]], times[pr[i + w]]
        def event(r):
            e = closes.get(r["id"]) if r["kind"] == "issue" else r
            return e and lo < mine.ts(e["ts"]) <= hi
        gone = gone_by(every, hi)
        ids = sorted(k for k, r in rows.items() if k not in gone and event(r))
        print(json.dumps({"pr_commits": len(win), "from": lo.isoformat(),
                          "to": hi.isoformat(), "issues": sum(
                              rows[k]["kind"] == "issue" for k in ids),
                          "decisions": sum(rows[k]["kind"] == "decision"
                                           for k in ids)}))
    random.Random(SEED).shuffle(ids)

    os.makedirs(out_dir)
    bare = os.path.join(out_dir, "repo.git")
    subprocess.run(["git", "init", "-q", "--bare", bare], check=True)
    mine.git(bare, "fetch", "-q", os.path.abspath(repo),
             "refs/remotes/origin/main:refs/heads/main")
    key = []
    with open(os.path.join(out_dir, "sheet.md"), "w") as f:
        f.write(f"# row screen — seed {SEED}, {len(ids)} rows\n\n"
                "Criteria: scripts/onoff/row-brief.md\n")
        for n, k in enumerate(ids, 1):
            r, c = rows[k], closes.get(k)
            at = at_commit(c if r["kind"] == "issue" else r, pr_main, main_t)
            mine.git(bare, "tag", f"at-{n}", at)
            key.append({"n": n, "id": k, "kind": r["kind"], "ts": r["ts"],
                        "close_ts": c and c["ts"], "at": at})
            f.write(f"\n## {n}. {r['kind']} — at `at-{n}`\n"
                    f"files: {', '.join(r.get('files') or [])}\n\n"
                    f"{r.get('title') or ''}\n\n{r['text']}\n")
            if c:
                f.write(f"\nclose: {c['text']}\n")
    json.dump(key, open(os.path.join(out_dir, "key.json"), "w"), indent=1)
    print(f"→ {out_dir}/sheet.md ({len(ids)} rows)")


if __name__ == "__main__":
    main(*sys.argv[1:])
