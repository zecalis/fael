#!/usr/bin/env python3
"""PLAN-fael-onoff chunk 2: mine candidate tasks from a repo's main history.

A candidate is a first-parent commit C on origin/main whose changed files
overlap the files of a ledger row that was experience before C^ landed:
an issue closed before C^, or a decision still active at C^. Rows written on
C's own branch are dropped (they are C's work, not prior experience).

Read-only on the target repo. Usage:
  scripts/onoff/mine.py <repo> <out-dir>
Writes <out-dir>/candidates.jsonl (the whole pool) and a new
<out-dir>/cohort-<n>/ holding only candidates no earlier cohort screened.
Screen it with scripts/onoff/screen-brief.md, then run tally.py.
"""
import collections
import glob
import json
import os
import random
import re
import subprocess
import sys
from datetime import datetime

SEED = 20261009


def ts(s):
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


def git(repo, *args):
    return subprocess.run(["git", "-C", repo, *args], check=True,
                          capture_output=True, text=True).stdout


def load_ledger(repo):
    common = git(repo, "rev-parse", "--git-common-dir").strip()
    log_dir = os.path.join(repo, common, "fael", "log")
    rows, closes, retired = {}, {}, collections.defaultdict(list)
    restored = set()
    for f in glob.glob(os.path.join(log_dir, "*", "*.jsonl")):
        for line in open(f):
            r = json.loads(line)
            if r.get("ref"):  # a close
                closes.setdefault(r["ref"], ts(r["ts"]))
            elif r.get("kind") in ("issue", "decision"):
                rows[r["id"]] = r
            if r.get("supersedes"):
                retired[r["supersedes"]].append((ts(r["ts"]), r["id"]))
            if r.get("restores"):
                restored.add(r["restores"])
    # a reverted supersede never retired anything
    superseded = {old: min(t for t, by in v if by not in restored)
                  for old, v in retired.items()
                  if any(by not in restored for _, by in v)}
    return rows, closes, superseded


def experience_at(row, t, closes, superseded):
    """Why this row was experience at time t, or None."""
    if ts(row["ts"]) >= t:
        return None
    if row["id"] in superseded and superseded[row["id"]] < t:
        return None
    closed = closes.get(row["id"])
    if row["kind"] == "issue":
        return "closed_issue" if closed and closed < t else None
    return "active_decision" if not (closed and closed < t) else None


def main_commits(repo):
    out = git(repo, "log", "--first-parent", "origin/main",
              "--format=%x00%H %P%x09%cI%x09%s", "--name-only")
    commits, times = [], {}
    for block in out.split("\x00")[1:]:
        head, *files = block.strip().split("\n")
        shas, when, subject = head.split("\t", 2)
        sha, *parents = shas.split()
        times[sha] = ts(when)
        commits.append((sha, parents[0] if parents else None, subject,
                        [f for f in files if f]))
    return commits, times


def main(repo, out_dir):
    rows, closes, superseded = load_ledger(repo)
    prs = json.loads(subprocess.run(
        ["gh", "pr", "list", "--state", "merged", "--limit", "1000",
         "--json", "number,headRefName"], cwd=repo, check=True,
        capture_output=True, text=True).stdout)
    branch_of = {p["number"]: p["headRefName"] for p in prs}
    commits, times = main_commits(repo)

    funnel = collections.Counter(commits=len(commits))
    out = []
    for sha, parent, subject, files in commits:
        m = re.search(r"\(#(\d+)\)$", subject)
        if not parent or not m or int(m.group(1)) not in branch_of:
            funnel["no_pr"] += 1
            continue
        pr = int(m.group(1))
        branch, t = branch_of[pr], times[parent]
        changed = set(files)
        hits = []
        for r in rows.values():
            overlap = changed & set(r.get("files") or [])
            if not overlap or r.get("branch") == branch:
                continue
            why = experience_at(r, t, closes, superseded)
            if why:
                hits.append({"id": r["id"], "why": why, "key": r.get("key"),
                             "title": r.get("title") or r["text"][:120],
                             "text": r["text"], "overlap": sorted(overlap)})
        if not hits:
            funnel["no_experience"] += 1
            continue
        code = [h for h in hits
                if any(not f.endswith(".md") for f in h["overlap"])]
        if not code:
            funnel["docs_only_overlap"] += 1
            continue
        funnel["candidate"] += 1
        out.append({"sha": sha, "parent": parent, "pr": pr, "branch": branch,
                    "subject": subject, "parent_time": t.isoformat(),
                    "rows": code})

    os.makedirs(out_dir, exist_ok=True)
    with open(os.path.join(out_dir, "candidates.jsonl"), "w") as f:
        for c in out:
            f.write(json.dumps(c, ensure_ascii=False) + "\n")
    print(json.dumps(funnel))
    kinds = collections.Counter(h["why"] for c in out for h in c["rows"])
    print("rows on candidates:", dict(kinds))
    write_cohort(out_dir, out)


def screened(out_dir):
    """Commit shas already screened in an earlier cohort (tally.py writes it).
    A candidate never changes once mined: rows before C^ are append-only."""
    path = os.path.join(out_dir, "screened.jsonl")
    if not os.path.exists(path):
        return set()
    return {json.loads(line)["sha"] for line in open(path) if line.strip()}


def shuffled(cands):
    # screened in this fixed random order, so a cohort's passes are a random
    # sample of its pool, not the ones that looked good first
    order = sorted(cands, key=lambda c: c["sha"])
    random.Random(SEED).shuffle(order)
    return order


def write_cohort(out_dir, out):
    done = screened(out_dir)
    new = [c for c in out if c["sha"] not in done]
    print(f"already screened {len(out) - len(new)} · new {len(new)}")
    if not new:
        return
    n = 1 + len(glob.glob(os.path.join(out_dir, "cohort-*")))
    d = os.path.join(out_dir, f"cohort-{n}")
    os.makedirs(os.path.join(d, "prescreen"))
    order = shuffled(new)
    with open(os.path.join(d, "candidates.jsonl"), "w") as f:
        for c in new:
            f.write(json.dumps(c, ensure_ascii=False) + "\n")
    json.dump([c["sha"] for c in order], open(os.path.join(d, "order.json"), "w"))
    with open(os.path.join(d, "screen.md"), "w") as f:
        f.write(f"# cohort {n} screen — seed {SEED}, {len(order)} candidates\n\n"
                "Criteria: scripts/onoff/screen-brief.md\n")
        for i, c in enumerate(order, 1):
            f.write(f"\n## {i}. #{c['pr']} {c['subject']}\n"
                    f"`{c['parent'][:10]}` → `{c['sha'][:10]}`\n")
            for h in c["rows"]:
                f.write(f"- {h['id']} {h['why']} — {h['title']}"
                        f" ({', '.join(h['overlap'])})\n")
    print(f"→ {d}/screen.md")


if __name__ == "__main__":
    main(*sys.argv[1:3])
