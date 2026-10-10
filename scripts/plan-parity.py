#!/usr/bin/env python3
"""PLAN-fael-board b1 parity: every plan's next chunk from `fael plan next` (plans.db)
must equal what `fapony plan <PLAN>` picks from the markdown.

    scripts/plan-parity.py <repo-root> [<fael binary>]

Imports into a scratch FAEL_DIR, so the repo's own plans.db is never touched. Run it from
the main checkout of each repo: both tools read the branch of the dir they run in.
Exit 1 on any mismatch.
"""

import os
import subprocess
import sys
import tempfile
from pathlib import Path


def fapony_token(out: str) -> str:
    lines = out.splitlines()
    if any(l.startswith("⚠ parked") for l in lines):
        return "(parked)"
    if any(l.startswith("⚠ blocked") for l in lines):
        return "(blocked)"
    if any(l.startswith("(all chunks closed") for l in lines):
        return "(all closed)"
    for i, l in enumerate(lines):
        if l == "## next" and i + 1 < len(lines):
            nxt = lines[i + 1]
            return "(none ready)" if nxt.startswith("(none ready") else nxt
    return "(no next section)"


def main() -> int:
    root = Path(sys.argv[1]).resolve()
    fael = sys.argv[2] if len(sys.argv) > 2 else "fael"
    env = dict(os.environ, FAEL_DIR=tempfile.mkdtemp(prefix="fael-parity-"))
    subprocess.run([fael, "plan", "import"], cwd=root, env=env, check=True)
    got = {}
    out = subprocess.run([fael, "plan", "next"], cwd=root, env=env, check=True,
                         capture_output=True, text=True).stdout
    for line in out.splitlines():
        key, _, what = line.partition("\t")
        got[key] = what
    bad = n = 0
    for fapony_dir in sorted(root.glob("**/.fapony")):
        if "node_modules" in fapony_dir.parts:
            continue
        app = fapony_dir.parent.relative_to(root).as_posix()
        app = "" if app == "." else app
        for sub in ("plan", "parked"):
            for f in sorted((fapony_dir / sub).glob("PLAN-*.md")):
                name = f.name[len("PLAN-"):-len(".md")].lower()
                key = f"{app}/{name}" if app else name
                want = fapony_token(subprocess.run(
                    ["fapony", "plan", str(f)], cwd=fapony_dir.parent,
                    capture_output=True, text=True).stdout)
                n += 1
                if got.get(key) != want:
                    bad += 1
                    print(f"✗ {key}\n    fapony: {want}\n    fael:   {got.get(key)}")
    print(f"{n - bad}/{n} plans agree")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
