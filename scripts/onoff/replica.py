#!/usr/bin/env python3
"""PLAN-fael-onoff chunk 3: a repo as it stood at a commit's parent, and its
fael ledger as it stood then. trial.py builds each trial from this; a hook
replay (PLAN-fael-push-noise chunk 1) reuses it per commit.

base(): a bare repo holding only history reachable from C^ (fetched by sha
into an empty repo, so C and later never arrive), with .fael/ dropped from
every commit — the tracked frozen log is ledger content, and it must reach an
agent only through the snapshot. strip=True also cuts fael's commands from
every markdown file in every commit (the off arm's AGENTS.md/CLAUDE.md), so
neither `git status` nor `git log -p` shows what was cut.

snapshot(): every ledger line (the clone's journal, plus the tree log at C^)
written before C^ landed and not on C's own branch, folded into
<repo>/.git/fael/log/ — fael has no as-of read, so the filter lives here.

Read-only on <src>. Usage (prints one JSON line):
  scripts/onoff/replica.py <src> <C sha> <dest> [--strip] [--ledger [--branch=<C's>]]
"""
import glob
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from datetime import datetime

# the agent sees its cwd: nothing in the path may name fael or the arm
ROOT = "/tmp/trials"
# a fael command or path in prose; `fael #key` pointers inside a rule are kept
CMD = re.compile(rb"`fael\b|\bfael (add|find|close|kickoff|sync|stats|hook|mcp"
                 rb"|install|next|claim)\b|Fael MCP|\.fael/|\.git/fael"
                 rb"|chore\(fael\)|\(see Memory\)")
HEAD = re.compile(rb"(#+)\s+(.*?)\s*$")


def ts(s):
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


def git(repo, *args, **kw):
    return subprocess.run(["git", "-C", repo, *args], check=True,
                          capture_output=True, **kw).stdout


def strip(text):
    """Markdown without fael's commands: a `# Memory` section that names fael
    goes whole, any other line naming a fael command or path goes alone."""
    lines = text.splitlines(keepends=True)
    out, i = [], 0
    while i < len(lines):
        h = HEAD.match(lines[i])
        if h and h.group(2).lower() == b"memory":
            j = i + 1
            while j < len(lines) and not (
                    (m := HEAD.match(lines[j])) and len(m.group(1)) <= len(h.group(1))):
                j += 1
            if b"fael" in b"".join(lines[i:j]).lower():
                i = j
                continue
        if not CMD.search(lines[i]):
            out.append(lines[i])
        i += 1
    return b"".join(out)


def _path(raw):
    # fast-export C-quotes odd paths; only the prefix/suffix tests read it
    return raw[1:-1] if raw.startswith(b'"') else raw


def rewrite(raw, dest, strip_md):
    """fast-export raw's main → fast-import dest, dropping .fael/ and (strip_md)
    rewriting markdown that names fael. Dropped blobs still land in dest
    unreferenced; clone() copies only what main reaches."""
    exp = subprocess.Popen(["git", "-C", raw, "fast-export", "--reencode=yes",
                            "--signed-commits=strip", "--signed-tags=strip",
                            "refs/heads/main"], stdout=subprocess.PIPE)
    imp = subprocess.Popen(["git", "-C", dest, "fast-import", "--quiet"],
                           stdin=subprocess.PIPE)
    r, w = exp.stdout, imp.stdin
    fael_blobs, in_blob, mark = {}, False, None
    while line := r.readline():
        if line.startswith(b"data "):
            data = r.read(int(line[5:]))
            if in_blob and b"fael" in data.lower():
                fael_blobs[mark] = data
            in_blob = False
            w.write(line + data)
            continue
        if line == b"blob\n":
            in_blob = True
        elif line.startswith(b"mark "):
            mark = line[5:].strip()
        elif line.startswith((b"M ", b"D ")):
            parts = line.rstrip(b"\n").split(b" ", 3 if line[0] == ord("M") else 1)
            p = _path(parts[-1])
            if p == b".fael" or p.startswith(b".fael/"):
                continue
            if line.startswith(b"M ") and strip_md and p.endswith(b".md") \
                    and parts[2] in fael_blobs:
                new = strip(fael_blobs[parts[2]])
                if new != fael_blobs[parts[2]]:
                    w.write(b"M %s inline %s\ndata %d\n" % (parts[1], parts[3], len(new))
                            + new + b"\n")
                    continue
        w.write(line)
    w.close()
    if exp.wait() or imp.wait():
        sys.exit(f"rewrite {raw} → {dest} failed")


def base(src, parent, strip_md):
    """The cached bare repo for (C^, strip): built once, read by every run."""
    d = os.path.join(ROOT, "base", f"{parent[:12]}-{'strip' if strip_md else 'plain'}.git")
    if os.path.exists(os.path.join(d, "done")):
        return d
    raw = d + ".raw"
    for p in (d, raw):
        shutil.rmtree(p, ignore_errors=True)
    git(ROOT, "init", "-q", "--bare", raw)
    git(raw, "fetch", "-q", "--no-tags", src, f"{parent}:refs/heads/main")
    git(ROOT, "init", "-q", "--bare", "-b", "main", d)
    rewrite(raw, d, strip_md)
    shutil.rmtree(raw)
    open(os.path.join(d, "done"), "w").close()
    return d


def clone(b, dest):
    """A trial's own origin (so a push never reaches another run) and a
    checkout of it; --no-local copies only objects main reaches."""
    origin = dest + ".origin.git"
    git(ROOT, "clone", "-q", "--bare", "--no-local", b, origin)
    git(ROOT, "clone", "-q", "--no-local", origin, dest)
    return git(dest, "rev-parse", "HEAD", text=True).strip()


def ledger(src, parent, cutoff, branch):
    """{relative log path: [lines]} of every ledger line written before cutoff
    and not on branch: the journal, then the tree log at C^ (frozen history
    fael still reads), duplicates dropped."""
    common = os.path.join(src, git(src, "rev-parse", "--git-common-dir", text=True).strip())
    files = {}
    for f in sorted(glob.glob(os.path.join(common, "fael", "log", "**", "*.jsonl"),
                              recursive=True)):
        rel = os.path.relpath(f, os.path.join(common, "fael", "log"))
        files[rel] = open(f, "rb").read().splitlines()
    tree = git(src, "ls-tree", "-r", "--name-only", parent, "--", ".fael/log", text=True)
    for p in tree.split():
        if p.endswith(".jsonl"):
            rel = os.path.relpath(p, ".fael/log")
            files.setdefault(rel, []).extend(git(src, "show", f"{parent}:{p}").splitlines())
    out = {}
    for rel, lines in files.items():
        seen, keep = set(), []
        for line in lines:
            if not line.strip() or line in seen:
                continue
            seen.add(line)
            r = json.loads(line)
            if "ts" not in r:
                sys.exit(f"{rel}: a ledger line with no ts — cannot place it in time")
            if ts(r["ts"]) < cutoff and r.get("branch") != branch:
                keep.append(line)
        if keep:
            out[rel] = keep
    return out


def snapshot(src, parent, cutoff, branch, repo):
    """Write ledger() into repo's journal; returns (sha256, rows, ids)."""
    root = os.path.join(repo, ".git", "fael", "log")
    h, n, ids = hashlib.sha256(), 0, set()
    for rel, lines in sorted(ledger(src, parent, cutoff, branch).items()):
        os.makedirs(os.path.dirname(os.path.join(root, rel)), exist_ok=True)
        with open(os.path.join(root, rel), "wb") as f:
            f.write(b"\n".join(lines) + b"\n")
        h.update(rel.encode() + b"\0" + b"\n".join(lines) + b"\n")
        n += len(lines)
        ids |= {json.loads(x).get("id") for x in lines}
    return h.hexdigest(), n, ids - {None}


def snapshot_leaks(repo, cutoff, branch):
    """Every line in repo's journal is older than cutoff and off C's branch."""
    bad = []
    for f in glob.glob(os.path.join(repo, ".git", "fael", "log", "**", "*.jsonl"),
                       recursive=True):
        for line in open(f, "rb"):
            r = json.loads(line)
            if ts(r["ts"]) >= cutoff or r.get("branch") == branch:
                bad.append(r.get("id") or r.get("ref"))
    return bad


def git_leaks(repo, c, cutoff):
    """What C^'s copy must not hold: C's object, a second history, a commit
    after C^ landed, a .fael path, an object nothing reaches."""
    fails = {}
    if subprocess.run(["git", "-C", repo, "cat-file", "-e", c],
                      capture_output=True).returncode == 0:
        fails["c_object"] = c
    every = git(repo, "rev-list", "--all", text=True).split()
    if len(every) != len(git(repo, "rev-list", "HEAD", text=True).split()):
        fails["second_history"] = len(every)
    late = [t for t in git(repo, "log", "--all", "--format=%cI", text=True).split()
            if ts(t) > cutoff]
    if late:
        fails["late_commits"] = len(late)
    if git(repo, "log", "--all", "--format=%H", "--", ".fael", text=True).strip():
        fails["fael_path"] = True
    reach = len(git(repo, "rev-list", "--objects", "--all", text=True).splitlines())
    held = len(git(repo, "cat-file", "--batch-all-objects", "--batch-check",
                   text=True).splitlines())
    if held != reach:
        fails["unreachable_objects"] = held - reach
    return fails


def main(src, c, dest, *flags):
    parent = git(src, "rev-parse", f"{c}^", text=True).strip()
    c = git(src, "rev-parse", c, text=True).strip()
    cutoff = ts(git(src, "show", "-s", "--format=%cI", parent, text=True).strip())
    tip = clone(base(src, parent, "--strip" in flags), dest)
    out = {"c": c, "parent": parent, "parent_time": cutoff.isoformat(), "tip": tip,
           "git_leaks": git_leaks(dest, c, cutoff)}
    if "--ledger" in flags:
        # C's own branch (mine.py's candidates.jsonl has it); unknown → no drop
        branch = next((f[9:] for f in flags if f.startswith("--branch=")), None)
        sha, rows, _ = snapshot(src, parent, cutoff, branch, dest)
        out |= {"branch": branch, "snapshot_sha256": sha, "snapshot_rows": rows,
                "snapshot_leaks": snapshot_leaks(dest, cutoff, branch)}
    print(json.dumps(out))


if __name__ == "__main__":
    main(*sys.argv[1:])
