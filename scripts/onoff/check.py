#!/usr/bin/env python3
"""Self-check for replica.py and trial.reached() on throwaway data: what
C^'s copy keeps and drops, that each leak check fires on a planted leak, and
when a row counts as delivered. Exits 0 or raises.

Usage: scripts/onoff/check.py
"""
import json
import os
import subprocess
import tempfile

import replica
import trial


def git(d, *a, date="2026-10-01T00:00:00+00:00"):
    env = os.environ | {"GIT_AUTHOR_DATE": date, "GIT_COMMITTER_DATE": date}
    return subprocess.run(["git", "-C", d, *a], check=True, capture_output=True,
                          text=True, env=env).stdout.strip()


def row(id, t, branch="old", **kw):
    return json.dumps({"id": id, "ts": t, "kind": "issue", "branch": branch, **kw})


def main():
    t = tempfile.mkdtemp(prefix="replica-check-")
    replica.ROOT = t
    src = os.path.join(t, "src")
    os.makedirs(os.path.join(src, ".fael", "log", "w"))
    git(t, "init", "-q", "-b", "main", src)
    git(src, "config", "user.name", "x")
    git(src, "config", "user.email", "x@x")
    open(os.path.join(src, "CLAUDE.md"), "w").write(
        "# Rules\n* keep `a.ts` small\n* run `fael kickoff` first\n"
        "## Memory\nfael is append-only\n* rows hold why\n## Tests\n* bun test\n")
    open(os.path.join(src, "a.ts"), "w").write("// a\n")
    open(os.path.join(src, ".fael", "log", "w", "2026-09.jsonl"), "w").write(
        row("TREE1", "2026-09-01T00:00:00Z") + "\n")
    git(src, "add", "-A")
    git(src, "commit", "-qm", "parent", date="2026-10-01T00:00:00+00:00")
    parent = git(src, "rev-parse", "HEAD")
    open(os.path.join(src, "a.ts"), "w").write("// a fixed\n")
    git(src, "commit", "-qam", "C", date="2026-10-02T00:00:00+00:00")
    c = git(src, "rev-parse", "HEAD")
    j = os.path.join(src, ".git", "fael", "log", "w")
    os.makedirs(j)
    open(os.path.join(j, "2026-10.jsonl"), "w").write("\n".join([
        row("OLD", "2026-09-15T00:00:00Z"),
        row("MINE", "2026-09-20T00:00:00Z", branch="feat/c"),     # C's own work
        row("LATE", "2026-10-01T12:00:00Z"),                      # after C^ landed
        json.dumps({"id": "CL", "ref": "OLD", "ts": "2026-10-03T00:00:00Z"}),
    ]) + "\n")
    cutoff = replica.ts("2026-10-01T00:00:00+00:00")

    off = os.path.join(t, "off")
    replica.clone(replica.base(src, parent, True), off)
    md = open(os.path.join(off, "CLAUDE.md")).read()
    assert md == "# Rules\n* keep `a.ts` small\n## Tests\n* bun test\n", md
    assert not os.path.exists(os.path.join(off, ".fael"))
    assert replica.git_leaks(off, c, cutoff) == {}, replica.git_leaks(off, c, cutoff)

    on = os.path.join(t, "on")
    replica.clone(replica.base(src, parent, False), on)
    assert "fael kickoff" in open(os.path.join(on, "CLAUDE.md")).read()
    _, rows, ids = replica.snapshot(src, parent, cutoff, "feat/c", on)
    assert ids == {"OLD", "TREE1"} and rows == 2, ids
    assert replica.snapshot_leaks(on, cutoff, "feat/c") == []

    # planted leaks: each check must fire
    open(os.path.join(on, ".git", "fael", "log", "w", "2026-10.jsonl"), "a").write(
        row("LATE", "2026-10-01T12:00:00Z") + "\n")
    assert replica.snapshot_leaks(on, cutoff, "feat/c") == ["LATE"]
    git(off, "fetch", "-q", src, f"{c}:refs/heads/leak")
    got = replica.git_leaks(off, c, cutoff)
    assert {"c_object", "second_history", "late_commits"} <= got.keys(), got
    reached(t)
    print("check ok")


def reached(t):
    """An id prints as its shortest unique prefix: 01M45SPWF is not 01M45SPWG.
    Delivered = in an edit hook's reply, not only the session brief."""
    p = os.path.join(t, "transcript.jsonl")
    hook = lambda event, text: json.dumps({"type": "system", "subtype": "hook_response",
                                           "hook_event": event, "stdout": text})
    open(p, "w").write("\n".join([
        hook("SessionStart", "- [01M45SPWG] issue PostToolUse in a brief"),
        hook("PostToolUse", "fael mem for a.ts:\n- [01M45SPWF] decision"),
        json.dumps({"type": "assistant", "text": "01M45SPWH"}),
    ]) + "\n")
    got = lambda row: (lambda r: (r["seen"], r["delivered"]))(trial.reached(p, row))
    assert got("01M45SPWG" + "0" * 17) == (True, False)
    assert got("01M45SPWF" + "0" * 17) == (True, True)
    assert got("01M45SPWH" + "0" * 17) == (False, False)  # not a hook reply


if __name__ == "__main__":
    main()
