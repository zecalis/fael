#!/usr/bin/env python3
"""Every open plan checkpoint that waits on events, read once: what each gate
has now against what its locked decision needs. Read-only; it counts, it never
judges a gate's outcome (each plan's own chunk does that).

Usage: scripts/checkpoints.py [<vela repo>]   (default ~/Project/zecalis/zecalis)

Windows and bars are the locked values of the decision named on each line;
a change there is a new decision first, then this file.
"""
import glob
import json
import os
import re
import subprocess
import sys
import tempfile
from datetime import datetime, timezone

HERE = os.path.dirname(os.path.abspath(__file__))
FAEL = os.path.dirname(HERE)
VELA = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser("~/Project/zecalis/zecalis")
STATE = os.environ.get("FAEL_STATE_DIR") or os.path.expanduser("~/.local/state/fael")
sys.path.insert(0, os.path.join(HERE, "onoff"))
import mine  # noqa: E402  main_commits(): the "PR commit" the onoff plans count


def ts(s):
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


def git(repo, *a):
    return subprocess.run(["git", "-C", repo, *a], check=True, capture_output=True,
                          text=True).stdout.strip()


def stats(since, state=None):
    env = dict(os.environ, **({"FAEL_STATE_DIR": state} if state else {}))
    out = subprocess.run(["fael", "stats", "--json", "--since", since], env=env,
                         check=True, capture_output=True, text=True).stdout
    return json.loads(out)


def vela_state():
    """vela-only usage, the one-liner of PLAN-fael-vela-dogfood §7 (01M4FN54)."""
    d = tempfile.mkdtemp(prefix="vela-stats-")
    keep = re.compile(r'"repo":"[^"]*/zecalis/(wt-vela|zecalis)')
    with open(os.path.join(d, "usage.jsonl"), "w") as out:
        for f in sorted(glob.glob(os.path.join(STATE, "usage", "*"))) + [os.path.join(STATE, "usage.jsonl")]:
            if os.path.isfile(f):
                out.writelines(l for l in open(f) if keep.search(l))
    return d


def ledger(repo):
    """rows by id, and each row's newest close ts (a close row, else the one compact folded in)."""
    common = git(repo, "rev-parse", "--path-format=absolute", "--git-common-dir")
    rows, closed = {}, {}
    for f in glob.glob(os.path.join(common, "fael", "log", "**", "*.jsonl"), recursive=True):
        for line in open(f):
            try:
                v = json.loads(line)
            except ValueError:
                continue
            if f.endswith(".close.jsonl"):
                if v.get("ref") and v.get("ts"):
                    closed[v["ref"]] = max(closed.get(v["ref"], ""), v["ts"])
            elif v.get("id"):
                rows[v["id"]] = v
    for r in rows.values():
        c = r.get("closed")
        if r["id"] not in closed and isinstance(c, dict) and c.get("ts"):
            closed[r["id"]] = c["ts"]
    return rows, closed


def release_of(sha):
    """(tag, commit time) of the first fael tag holding sha, the rule of 01M4HX74; None if unreleased."""
    tags = git(FAEL, "tag", "--contains", sha, "--sort=creatordate").split()
    return (tags[0], ts(git(FAEL, "log", "-1", "--format=%cI", tags[0]))) if tags else None


def bar(have, need):
    return f"{have}/{need}"


def gates():
    allt = stats("all")
    vs = stats("2026-10-09T06:08:13Z", vela_state())
    vrows, vclosed = ledger(VELA)
    frows, fclosed = ledger(FAEL)
    commits, times = mine.main_commits(VELA)

    den = allt["experience"]["label"]["close_core"]["den"]
    yield den >= 100, "label 4", f"issue closes since contract {bar(den, 100)}", "§6.4"

    c, k = vs["value"]["issues_closed"], vs["friction"]["calls"]
    yield c >= 30 and k >= 300, "vela-dogfood 4", f"vela closes {bar(c, 30)} · agent calls {bar(k, 300)}", "01M4FKDD"

    ms = int(re.search(r"FIX_CUTOFF_MS: i64 = ([\d_]+)", open(os.path.join(
        FAEL, "fael-core/src/stats/experience.rs")).read()).group(1).replace("_", ""))
    cut = datetime.fromtimestamp(ms / 1000, timezone.utc)
    new = {n: sum(1 for i, t in cl.items() if rs.get(i, {}).get("kind") == "issue" and ts(t) >= cut)
           for n, (rs, cl) in {"fael": (frows, fclosed), "vela": (vrows, vclosed)}.items()}
    n = sum(new.values())
    yield n >= 30, "fix-evidence 4", f"closes after cutoff {bar(n, 30)} {new} · then owner checks ≥20% by hand", "01M4HX74"

    rel = release_of("74b2b4b")
    if not rel:
        yield False, "decision-held 5", "M1 (74b2b4b) in no release yet", "01M4J0SSD"
    else:
        d = sum(1 for r in vrows.values() if r.get("kind") == "decision" and ts(r["ts"]) >= rel[1])
        yield d >= 100, "decision-held 5", f"vela decisions since {rel[0]} {bar(d, 100)} (installed on vela at or after the tag)", "01M4J0SSD"

    no = stats("2026-10-04T05:31:09Z")["said"]["notice"]["said"]  # say-gate chunk 3 merge (#208)
    s9 = stats("2026-10-09")["said"]
    br, bo = s9["brief"]["said"], s9["bodies"]["said"]
    yield no >= 100 and br >= 100, "say-gate 4b", f"notice said {bar(no, 100)} · brief said {bar(br, 100)}", "plan §6"
    tag, at = release_of(git(FAEL, "log", "--format=%H", "-S", "silent-start", "--reverse").split()[0])
    st = stats(at.isoformat())["by_event"].get("session-start", {}).get("events", 0)
    yield bo >= 100 and st >= 200, "say-gate 4c", f"bodies said {bar(bo, 100)} · session-starts since {tag} {bar(st, 200)}", "plan:fael-say-gate:revert-measure"

    for name, repo in (("fael", FAEL), ("vela", VELA)):
        common = git(repo, "rev-parse", "--path-format=absolute", "--git-common-dir")
        try:
            g = json.load(open(os.path.join(common, "fael", "cache", "push-gate.json")))
        except (OSError, ValueError):
            g = {"stage": "shadow", "pending": 0}
        info = f"{name} {g.get('stage')}" + (f" · next look {bar(g.get('pending', 0), 100)} search pushes" if g.get("stage") in ("shadow", "canary", "ramp") else "")
        yield None, "learn-loop 5c", info, "stage.rs"

    shipped = ts("2026-10-10T03:01:40Z")
    p = sum(1 for sha, *_ in commits if times[sha] > shipped)
    yield p >= 200, "capture-yield 3", f"vela PR commits after ship {bar(p, 200)}", "01M4HW6A"

    end = next(i for i, (sha, *_) in enumerate(commits) if sha.startswith("195ab488"))
    pool = len(commits) - end  # commits[] is newest first
    yield None, "onoff 2", f"tasks 15/30 · vela main commits since cohort 1 {end} (cohort 1: {pool} commits → 15 tasks)", "01M4GSD3"

    yield None, "auto-update 4", "by hand at the next release (release.sh --no-local)", "01M4GDC6"


def main():
    mark = {True: "ready", False: "wait ", None: "info "}
    for ok, gate, what, src in gates():
        print(f"{mark[ok]}  {gate:<16} {what}  ({src})")


if __name__ == "__main__":
    main()
