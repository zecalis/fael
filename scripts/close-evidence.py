#!/usr/bin/env python3
"""Per closed issue of a repo's fael ledger: the fix evidence its close names
and whether carry's evidence gate lets it through today (PLAN-fael-fix-evidence
chunk 1 baseline).

Usage: scripts/close-evidence.py <repo> [--tsv <out>]

The close read is the one carry reads (`fix_close`, fael-core/src/query/stale.rs):
the issue's newest close row by ts, else the close `fael compact` folded into
the row. The gate is `names_fix` then `fix_reached` (fael/src/hook/reached.rs),
replayed with the same git commands from <repo>. It is the evidence gate only:
carry also needs the row on an edited file and not superseded.

Evidence columns, never a verdict on the fix itself:
  pr      a `#N` word (old policy, `bare_shas` reads none past it)
  sha     sha-like words when no `#N`; on_main = one is an ancestor of main
  cite    `(fael:<prefix>)`, prefix >= 8 chars, on main resolving to this one row
          (`ambig` when the prefix matches more than one row id)
  substr  the id's first 8 chars anywhere in main's messages (what carry's
          `--grep=<cite>` matches, and §7's first count)
"""
import glob, json, os, re, subprocess, sys
from collections import Counter

repo = sys.argv[1]
tsv = sys.argv[sys.argv.index("--tsv") + 1] if "--tsv" in sys.argv else None


def git(*a):
    return subprocess.run(["git", "-C", repo, *a], capture_output=True, text=True)


common = git("rev-parse", "--path-format=absolute", "--git-common-dir")
if common.returncode:
    sys.exit(common.stderr.strip())
common = common.stdout.strip()
rows, close_rows = {}, {}
for f in sorted(glob.glob(os.path.join(common, "fael", "log", "**", "*.jsonl"), recursive=True)):
    for line in open(f):
        try:
            v = json.loads(line)
        except ValueError:
            continue
        if f.endswith(".close.jsonl"):
            if v.get("ref"):
                close_rows.setdefault(v["ref"], []).append(v)
        else:
            rows[v.get("id")] = v


def close_text(r):
    cs = close_rows.get(r["id"])
    if cs:
        return max(cs, key=lambda c: c.get("ts", "")).get("text", "")
    c = r.get("closed")
    return c.get("text", "") if isinstance(c, dict) else None


# words(): ASCII alphanumeric runs, `#` kept (experience.rs)
words = lambda t: re.findall(r"[A-Za-z0-9#]+", t)
sha_like = lambda w: re.fullmatch(r"[0-9A-Fa-f]{7,40}", w) and re.search(r"[0-9]", w) and re.search(r"[A-Fa-f]", w)
pr_like = lambda w: re.fullmatch(r"#[0-9]+", w)

# the Rust word rules, pinned: a drift here silently skews the baseline
assert words("x (#12) a_b é9") == ["x", "#12", "a", "b", "9"]
assert sha_like("22703f2") and sha_like("DEADBEEF1") and sha_like("01DEFACED01")
assert not sha_like("1234567") and not sha_like("abcdefa") and not sha_like("22703f")
assert pr_like("#12") and not pr_like("#") and not pr_like("#12a")


def fix_reached(close, id_):
    """reached.rs, command for command: (verdict, reason)."""
    if any(pr_like(w) for w in words(close)):
        return True, "pr"
    shas = [w for w in words(close) if sha_like(w)]
    if not shas:
        return True, "no-sha"
    for sha in shas:
        s = git("log", "-1", "--format=%s", f"{sha}^{{commit}}")
        if s.returncode:
            return True, "sha-not-in-clone"
        grep = lambda *refs: git("log", "-1", "--format=%h", "-F", f"--grep={s.stdout.strip()}", f"--grep={id_[:8]}", *refs)
        o = grep("HEAD", "origin/HEAD")
        if o.returncode:
            o = grep("HEAD")
        if o.returncode == 0 and o.stdout.strip():
            return True, "grep-hit"
    return False, "unmerged"


has_origin = git("rev-parse", "--verify", "-q", "origin/HEAD").returncode == 0
main = "origin/HEAD" if has_origin else "HEAD"
log = git("log", main, "--format=%B").stdout
tokens = set(re.findall(r"\(fael:([0-9A-Z]{8,26})\)", log))
ids = [i for i in rows if i]


def cite(id_, tokens, ids):
    hit = [t for t in tokens if id_.startswith(t)]
    if not hit:
        return "no"
    return "ambig" if any(sum(i.startswith(t) for i in ids) > 1 for t in hit) else "yes"


# vela on 2026-10-10: (fael:01M4G1N1) on main, two rows share those 8 chars
two = ["01M4G1N16R2XCFR6K4HYGB1DP4", "01M4G1N199XPXZKVSXK9F84YN3"]
assert cite(two[0], {"01M4G1N1"}, two) == "ambig"
assert cite(two[0], {"01M4G1N16"}, two) == "yes"
assert cite(two[0], set(), two) == "no"


issues = [r for r in rows.values() if r.get("kind") == "issue" and close_text(r) is not None]
n, out = Counter(issues=len(issues)), []
for r in sorted(issues, key=lambda r: r["id"]):
    i, t = r["id"], close_text(r)
    ws = words(t)
    shas = [w for w in ws if sha_like(w)]
    ev = "pr" if any(pr_like(w) for w in ws) else "sha" if shas else "none"
    on_main = "-"
    if ev == "sha":
        on_main = "yes" if any(git("merge-base", "--is-ancestor", s, main).returncode == 0 for s in shas) else "no"
    c, sub = cite(i, tokens, ids), "yes" if i[:8] in log else "no"
    names = any(sha_like(w) or pr_like(w) for w in ws)
    carry, why = fix_reached(t, i) if names else (False, "no-fix-named")
    n[f"ev:{ev}"] += 1
    n[f"sha_on_main:{on_main}"] += ev == "sha"
    n[f"cite:{c}"] += 1
    n[f"cite_yes&ev:{ev}"] += c == "yes"
    n[f"substr:{sub}"] += 1
    n[f"carry:{'yes' if carry else 'no'}"] += 1
    n[f"why:{why}"] += 1
    n[f"carry_yes&ev:{ev}"] += carry
    out.append((i, ev, ",".join(shas) or "-", on_main, c, sub, "yes" if carry else "no", why))

print(json.dumps(dict(repo=repo, head=git("rev-parse", "HEAD").stdout.strip(),
                      main=main, main_sha=git("rev-parse", main).stdout.strip(),
                      counts={k: v for k, v in sorted(n.items()) if v}), indent=1))
if tsv:
    with open(tsv, "w") as f:
        f.write("id\tevidence\tshas\tsha_on_main\tcite\tsubstr\tcarry\twhy\n")
        f.writelines("\t".join(row) + "\n" for row in out)
