#!/usr/bin/env python3
"""PLAN-fael-onoff chunk 3: run one trial (task, arm, run) and keep its files.

  on  — C^'s copy + the ledger snapshot (replica.py) + `fael install` into the
        trial's own HOME (hooks, MCP, skill: what a user gets)
  off — C^'s copy with fael's commands cut from every markdown file, no
        ledger, a HOME with nothing in it
Both arms: the same tree otherwise, prompt, runner, model, flags, timeout and
env; a fresh HOME and FAEL_STATE_DIR per trial; the agent works in
/tmp/trials/<hex>/repo, a path that names neither fael nor the arm.

After the agent, the evaluator applies its diff to a fresh copy outside the
workspace, puts back the test suite as it stood at C for every package C's
tests or the diff touch, and runs it. Results go to
<results>/trials/<pr>-<arm>-<run>/: manifest.json, diff.patch,
transcript.jsonl, runner.log, tests.json, tests.log, leaks.json. Scores are
chunk 5's. A trial that exists is never overwritten. /tmp/trials/<hex> is
removed after (--keep holds it): a later agent must not find an earlier
trial's workspace, ledger or C's tests. Run trials one at a time.

Runner `claude` needs CLAUDE_CODE_OAUTH_TOKEN (`claude setup-token`) or
ANTHROPIC_API_KEY: the trial's HOME holds no login. Runner `stub` changes
nothing and feeds the installed hooks a session start and an edit of each
file C changed — a plumbing check that spends no agent run.

Usage: scripts/onoff/trial.py <src> <results> <pr> <on|off> <run>
         [--runner claude|stub] [--model <id>] [--timeout <s>] [--no-install] [--keep]
"""
import argparse
import hashlib
import json
import os
import re
import secrets
import shutil
import subprocess
import sys
import time

import replica

TEST = re.compile(r"(^|/)(e2e|__tests__)/|\.(test|spec)\.[cm]?[jt]sx?$")
RUNNABLE = re.compile(r"\.(test|spec)\.[cm]?[jt]sx?$")
# `git log -G` (ERE) twin of replica.CMD, for the off arm's history check
CMD_ERE = (r"`fael|fael (add|find|close|kickoff|sync|stats|hook|mcp|install|next|claim)"
           r"|Fael MCP|\.fael/|\.git/fael|chore\(fael\)|\(see Memory\)")
# no \b: in an escaped reply an id may follow the "n" of a "\\n"
ULID = re.compile(r"(?<![0-9A-Z])[0-9A-HJKMNP-TV-Z]{8,26}(?![0-9A-Z])")
# inherited from whoever launched the harness (this very session, often)
DROP = re.compile(r"^(CLAUDE|ANTHROPIC_|FAEL_|GIT_|XDG_|MCP_)")
KEEP = {"CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_API_KEY"}


def sh(cmd, **kw):
    return subprocess.run(cmd, check=True, capture_output=True, text=True, **kw).stdout


def sha256(b):
    return hashlib.sha256(b if isinstance(b, bytes) else b.encode()).hexdigest()


def load_task(results, pr):
    task = next((t for t in json.load(open(os.path.join(results, "passed.json")))
                 if t["pr"] == pr), None)
    if not task:
        sys.exit(f"pr {pr} is not in {results}/passed.json")
    cand = next(c for line in open(os.path.join(results, "candidates.jsonl"))
                for c in [json.loads(line)] if c["sha"] == task["sha"])
    return task, cand


def trial_env(tdir, arm):
    env = {k: v for k, v in os.environ.items() if not DROP.match(k) or k in KEEP}
    env |= {"HOME": os.path.join(tdir, "home"),
            # the real HOME's bun cache: a fresh one per trial re-downloads
            "BUN_INSTALL_CACHE_DIR": os.path.expanduser("~/.bun/install/cache")}
    if arm == "on":  # off: no FAEL_* — `env` would show it
        env |= {"FAEL_STATE_DIR": os.path.join(tdir, "state"), "FAEL_NO_AUTO_UPDATE": "1"}
    for k in ("HOME", "FAEL_STATE_DIR"):
        if k in env:
            os.makedirs(env[k], exist_ok=True)
    return env


def setup(a, task, cand, tdir, ws, env):
    """The workspace and the arm; returns the manifest's setup part."""
    tip = replica.clone(replica.base(a.src, cand["parent"], a.arm == "off"), ws)
    sh(["git", "-C", ws, "config", "user.name", "agent"])
    sh(["git", "-C", ws, "config", "user.email", "agent@example.invalid"])
    m = {"tip": tip}
    if a.arm == "on":
        cutoff = replica.ts(cand["parent_time"])
        snap, rows, ids = replica.snapshot(a.src, cand["parent"], cutoff, cand["branch"], ws)
        m |= {"snapshot_sha256": snap, "snapshot_rows": rows,
              "row_in_snapshot": task["row"] in ids,
              "fael": sh(["fael", "--version"]).strip(),
              "install": sh(["fael", "install", "--client", "claude"], cwd=ws, env=env)}
    if not a.no_install:
        t = time.time()
        sh(["bun", "install", "--frozen-lockfile"], cwd=ws, env=env)
        m["bun_install_s"] = round(time.time() - t)
    return m


def run_claude(a, task, ws, env, out):
    argv = ["claude", "-p", task["prompt"], "--model", a.model,
            "--output-format", "stream-json", "--verbose", "--include-hook-events",
            "--permission-mode", "bypassPermissions", "--no-session-persistence"]
    with open(os.path.join(out, "transcript.jsonl"), "w") as o, \
            open(os.path.join(out, "runner.log"), "w") as e:
        try:
            code = subprocess.run(argv, cwd=ws, env=env, stdout=o, stderr=e,
                                  timeout=a.timeout).returncode
        except subprocess.TimeoutExpired:
            code = "timeout"
    return {"runner": "claude", "runner_version": sh(["claude", "--version"]).strip(),
            "argv": argv[:2] + ["<prompt>"] + argv[3:], "exit": code}


def run_stub(a, cand, ws, env, out):
    """No agent: the installed hooks see a session start, then an edit of each
    file C changed, in path order (what a hook replay feeds fael)."""
    hooks = {}
    path = os.path.join(env["HOME"], ".claude", "settings.json")
    if os.path.exists(path):
        for event, groups in json.load(open(path))["hooks"].items():
            for g in groups:
                for h in g["hooks"]:
                    hooks.setdefault(event, []).append((g.get("matcher"), h["command"]))
    files = replica.paths(a.src, "diff", "--name-only", cand["parent"], cand["sha"])
    session = secrets.token_hex(8)
    events = [("SessionStart", None, {"source": "startup"})] + [
        ("PostToolUse", "Edit", {"tool_name": "Edit", "tool_input":
                                 {"file_path": os.path.join(ws, f)}, "tool_response": {}})
        for f in sorted(files)]
    with open(os.path.join(out, "transcript.jsonl"), "w") as o:
        for event, tool, extra in events:
            for matcher, cmd in hooks.get(event, []):
                if matcher and not (tool and re.fullmatch(matcher, tool)):
                    continue
                stdin = json.dumps({"session_id": session, "cwd": ws,
                                    "hook_event_name": event, **extra})
                r = subprocess.run(cmd, shell=True, cwd=ws, env=env, input=stdin,
                                   capture_output=True, text=True)
                o.write(json.dumps({"type": "system", "subtype": "hook_response",
                                    "hook_event": event, **extra,
                                    "stdout": r.stdout, "exit": r.returncode}) + "\n")
    return {"runner": "stub", "exit": 0, "events": len(events)}


def collect(ws, tip, row, out):
    """diff.patch (commits and loose edits alike) · tokens · delivered."""
    idx = os.path.join(os.path.dirname(ws), "collect.index")
    e = os.environ | {"GIT_INDEX_FILE": idx}
    subprocess.run(["git", "-C", ws, "read-tree", tip], env=e, check=True)
    subprocess.run(["git", "-C", ws, "add", "-A"], env=e, check=True)
    diff = subprocess.run(["git", "-C", ws, "diff", "--cached", "--binary", tip],
                          env=e, check=True, capture_output=True).stdout
    open(os.path.join(out, "diff.patch"), "wb").write(diff)
    os.remove(idx)
    return {"diff_sha256": sha256(diff), "diff_bytes": len(diff)} | reached(
        os.path.join(out, "transcript.jsonl"), row)


def reached(transcript, row):
    """tokens · seen (the row in any hook reply) · delivered (in an edit's)."""
    usage, delivered, seen = None, False, False
    for line in open(transcript):
        try:
            v = json.loads(line)
        except json.JSONDecodeError:
            continue
        if v.get("type") == "result":
            usage = {k: v.get(k) for k in ("usage", "total_cost_usd", "num_turns")}
        # fael prints the shortest unique prefix of an id, 8 chars or more
        if "hook" in str(v.get("subtype", "")) and any(
                row.startswith(t) for t in ULID.findall(line)):
            seen = True
            # the edit hook is PostToolUse(Edit|Write|…): the row reached the
            # agent at an edit, not only in the session brief — read off the
            # event field (the brief's own text may name PostToolUse)
            # (claude 2.1.295's stream-json and the stub both write hook_event)
            delivered |= v.get("hook_event") == "PostToolUse"
    return {"tokens": usage, "delivered": delivered, "seen": seen}


def package_of(root, path):
    d = os.path.dirname(path)
    while d and not os.path.exists(os.path.join(root, d, "package.json")):
        d = os.path.dirname(d)
    return d


def put_suite(ev, p, mine, at_c):
    """Package p's tests become exactly C's (mine, read by at_c): any other
    test file in p goes, the agent's own included. A nested package's tests
    are its own package's business, not p's."""
    for root, _, names in os.walk(os.path.join(ev, p)):
        if "node_modules" in root:
            continue
        for n in names:
            f = os.path.relpath(os.path.join(root, n), ev)
            if TEST.search(f) and f not in mine and package_of(ev, f) == p:
                os.remove(os.path.join(ev, f))
    for f in mine:
        os.makedirs(os.path.dirname(os.path.join(ev, f)), exist_ok=True)
        open(os.path.join(ev, f), "wb").write(at_c(f))


def evaluate(a, cand, tdir, tip, out, env):
    """C's test suite against the agent's diff, outside the workspace."""
    ev = os.path.join(tdir, "eval")
    sh(["git", "clone", "-q", "--no-local", replica.base(a.src, cand["parent"], a.arm == "off"), ev])
    sh(["git", "-C", ev, "checkout", "-q", tip])
    patch = os.path.join(out, "diff.patch")
    applied = os.path.getsize(patch) == 0 or subprocess.run(
        ["git", "-C", ev, "apply", "--binary", patch], capture_output=True).returncode == 0
    c_tests = [f for f in replica.paths(a.src, "ls-tree", "-r", "--name-only", cand["sha"])
               if TEST.search(f)]
    changed = replica.paths(a.src, "diff", "--name-only", cand["parent"], cand["sha"])
    # numstat -z: "<added>\t<deleted>\t<path>" per NUL, the path unquoted
    touched = [x.split("\t", 2)[2] for x in replica.paths(ev, "apply", "--numstat", patch)
               if x.count("\t") >= 2] if applied and os.path.getsize(patch) else []
    pkgs = sorted({package_of(ev, f) for f in [x for x in changed if TEST.search(x)] + touched})
    res = {"applied": applied, "hidden": [f for f in changed if TEST.search(f)], "packages": {}}
    with open(os.path.join(out, "tests.log"), "w") as log:
        for p in pkgs if applied else []:
            mine = {f for f in c_tests if package_of(ev, f) == p}
            put_suite(ev, p, mine, lambda f: subprocess.run(
                ["git", "-C", a.src, "show", f"{cand['sha']}:{f}"],
                check=True, capture_output=True).stdout)
            run = sorted("./" + os.path.relpath(f, p) for f in mine if RUNNABLE.search(f))
            entry = {"run": run, "not_run": sorted(f for f in mine if not RUNNABLE.search(f))}
            if run:
                if not os.path.exists(os.path.join(ev, "node_modules")):
                    sh(["bun", "install", "--frozen-lockfile"], cwd=ev, env=env)
                r = subprocess.run(["bun", "test", *run], cwd=os.path.join(ev, p), env=env,
                                   capture_output=True, text=True, timeout=1800)
                log.write(f"=== {p}\n{r.stdout}{r.stderr}\n")
                n = {k: int(m.group(1)) if (m := re.search(rf"(\d+) {k}\b", r.stdout + r.stderr))
                     else 0 for k in ("pass", "fail", "skip")}
                entry |= {"exit": r.returncode, **n}
            res["packages"][p or "."] = entry
    json.dump(res, open(os.path.join(out, "tests.json"), "w"), indent=1)
    return res


def leaks(a, task, cand, ws, env):
    """§3's pilot checks, on the workspace as the agent got it."""
    cutoff = replica.ts(cand["parent_time"])
    f = {"git": replica.git_leaks(ws, cand["sha"], cutoff)}
    if a.arm == "off":  # before fael runs below and leaves state anywhere
        f["fael_in_home"] = subprocess.run(["grep", "-rli", "fael", env["HOME"]],
                                           capture_output=True, text=True).stdout.splitlines()
    hidden = []
    for p in replica.paths(a.src, "diff", "--name-only", cand["parent"], cand["sha"]):
        if TEST.search(p) and os.path.exists(os.path.join(ws, p)):
            at_c = subprocess.run(["git", "-C", a.src, "show", f"{cand['sha']}:{p}"],
                                  capture_output=True).stdout
            if open(os.path.join(ws, p), "rb").read() == at_c:
                hidden.append(p)
    f["hidden_tests_in_workspace"] = hidden
    check = env | {"FAEL_STATE_DIR": os.path.join(os.path.dirname(ws), "check-state")}
    rows = subprocess.run(["fael", "find", "--all", "--json", "--limit", "100000"], cwd=ws,
                          env=check, capture_output=True, text=True).stdout.splitlines()
    if a.arm == "on":
        f["snapshot"] = replica.snapshot_leaks(ws, cutoff, cand["branch"])
        f["task_row_unreadable"] = not any(task["row"] in r for r in rows)
    else:
        f["fael_rows_readable"] = len(rows)
        f["fael_dirs"] = [d for d in (".fael", ".git/fael") if os.path.exists(os.path.join(ws, d))]
        f["fael_commands_in_md_history"] = sh(
            ["git", "-C", ws, "log", "--all", "--format=%h", "-E", "-G", CMD_ERE, "--", "*.md"]
        ).split()
        f["stripped_md"] = _tree_diff(replica.base(a.src, cand["parent"], False), ws)
        f["arms_differ_beyond_md"] = [p for p in f["stripped_md"] if not p.endswith(".md")]
    info = {"stripped_md"}  # what was cut, for the owner to read — not a failure
    return {"checks": f, "ok": not any(v for k, v in f.items() if k not in info)}


def config(home):
    """What the agent's client loads from HOME: hooks, MCP servers, skills."""
    c = os.path.join(home, ".claude")
    read = lambda p: json.load(open(p)) if os.path.exists(p) else {}
    return {"settings": read(os.path.join(c, "settings.json")),
            "mcp": read(os.path.join(home, ".claude.json")).get("mcpServers", {}),
            "skills": sorted(os.listdir(os.path.join(c, "skills")))
            if os.path.isdir(os.path.join(c, "skills")) else []}


def _tree_diff(plain, ws):
    """Paths whose blob differs between the plain base's main and ws's HEAD."""
    a = dict(l.split("\t", 1)[::-1] for l in replica.paths(plain, "ls-tree", "-r", "main"))
    b = dict(l.split("\t", 1)[::-1] for l in replica.paths(ws, "ls-tree", "-r", "HEAD"))
    return sorted(p for p in a.keys() | b.keys() if a.get(p) != b.get(p))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("src")
    ap.add_argument("results")
    ap.add_argument("pr", type=int)
    ap.add_argument("arm", choices=["on", "off"])
    ap.add_argument("run", type=int)
    ap.add_argument("--runner", choices=["claude", "stub"], default="claude")
    ap.add_argument("--model")
    ap.add_argument("--timeout", type=int, default=3600)
    ap.add_argument("--no-install", action="store_true")
    ap.add_argument("--keep", action="store_true",
                    help="keep /tmp/trials/<id> (default: removed, so no later agent reads it)")
    a = ap.parse_args()
    a.src, a.results = os.path.abspath(a.src), os.path.abspath(a.results)
    if a.runner == "claude":
        if not any(os.environ.get(k) for k in KEEP):
            sys.exit("runner claude: export CLAUDE_CODE_OAUTH_TOKEN (claude setup-token) "
                     "or ANTHROPIC_API_KEY — the trial HOME holds no login")
        if not a.model:
            sys.exit("runner claude: --model is required (the same id for both arms)")
    task, cand = load_task(a.results, a.pr)
    name = f"{a.pr}-{a.arm}-{a.run}" + ("-stub" if a.runner == "stub" else "")
    out = os.path.join(a.results, "trials", name)
    if os.path.exists(out):
        sys.exit(f"{out} exists — a trial is never overwritten")
    run_id = secrets.token_hex(6)
    tdir = os.path.join(replica.ROOT, run_id)
    ws = os.path.join(tdir, "repo")
    os.makedirs(out)
    env = trial_env(tdir, a.arm)
    m = {"trial": name, "run_id": run_id, "workspace": ws, "arm": a.arm, "run": a.run,
         "task_set_sha256": sha256(open(os.path.join(a.results, "passed.json"), "rb").read()),
         "task": {k: task[k] for k in ("pr", "sha", "row", "label")} | {"parent": cand["parent"]},
         "rubric_version": sha256(task["rubric"])[:12], "prompt_sha256": sha256(task["prompt"]),
         "model": a.model, "timeout_s": a.timeout, "env_keys": sorted(env),
         "started": time.strftime("%Y-%m-%dT%H:%M:%S%z")}
    m |= setup(a, task, cand, tdir, ws, env)
    m["config"] = config(env["HOME"])
    m["config_sha256"] = sha256(json.dumps(m["config"], sort_keys=True))
    m["leaks"] = leaks(a, task, cand, ws, env)
    json.dump(m["leaks"], open(os.path.join(out, "leaks.json"), "w"), indent=1)
    if not m["leaks"]["ok"]:
        json.dump(m, open(os.path.join(out, "manifest.json"), "w"), indent=1)
        sys.exit(f"leak check failed, no agent run: {out}/leaks.json")
    m |= (run_claude(a, task, ws, env, out) if a.runner == "claude"
          else run_stub(a, cand, ws, env, out))
    m["ended"] = time.strftime("%Y-%m-%dT%H:%M:%S%z")
    m |= collect(ws, m["tip"], task["row"], out)
    m["tests"] = evaluate(a, cand, tdir, m["tip"], out, env)
    json.dump(m, open(os.path.join(out, "manifest.json"), "w"), indent=1)
    if not a.keep:
        shutil.rmtree(tdir)
    print(json.dumps({k: m[k] for k in ("trial", "run_id", "exit", "delivered", "seen")}
                     | {"leaks_ok": m["leaks"]["ok"]}))


if __name__ == "__main__":
    main()
