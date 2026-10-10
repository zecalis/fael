#!/usr/bin/env python3
"""PLAN-fael-push-noise chunk 1: replay fael's hook over cohort 1 and count
what it actually said against the screened labels.

Per commit C in screened.jsonl, per order (path order, reversed): C^'s copy
and its ledger snapshot (replica.py, the on arm's contract), a fresh HOME,
FAEL_STATE_DIR and session, then `fael hook session-start`, one prompt (it
marks the turn: without it per-turn kinds speak at every edit) and one
PostToolUse Edit per file C changed. What reached the agent is read off the
hook's own usage lines (`said`: row = search push, carry, check, brief, …);
`touch@1` is the shadow those lines already carry (`would_drop`).

A label sits on one (sha, row) pair; any other row said in that commit is
unlabeled, never right or wrong. Outputs <results>/<--out, push-noise>/:
cohort-1.json (aggregate + its sha256; time-dependent fields sit outside the
aggregate) and events-<order>.jsonl (every usage line, raw).

PLAN-fael-carry-reach chunk 1 adds `--turns`: `commit` (the default, the
run above) is one prompt per commit, so every edit shares one turn; `file`
sends a prompt before every edit, a turn per file. Only where the prompts
sit changes: each event is tagged with its turn and pair row, each pair keeps
its file order and carry's candidates, so turns.py can hold two runs to the
same inputs.

Each finished pair is kept in <out>/progress.jsonl, so a rerun of the same
command resumes where a cut-off run stopped; delete it to start over.

Usage: scripts/onoff/replay.py <src> <results> [--fael <bin>] [--only <n>]
       [--turns commit|file] [--out <dir under results>]
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import time
from collections import Counter

import replica
import trial

ORDERS = ("path", "reverse")
CHANNEL = {"row": "push", "carry": "carry", "check": "check", "brief": "brief"}
LABELS = {"a": "a", "b": "b", "c": "c", None: "gold"}
# say.rs `policy()` kinds with `per_turn`: they share one mark, so one line of
# any of them per user turn (`--turns commit`: a turn per commit, like an
# on/off `claude -p` trial; `file`: a turn per edited file)
TURNS = ("commit", "file")
PER_TURN = ("carry", "check", "merge", "promote")


def sha256(b):
    return hashlib.sha256(b).hexdigest()


# --- fael-core twins, read-only and only for the gold pre-check / reasons ---
def words(t):
    return [w for w in re.split(r"[^0-9A-Za-z#]", t) if w]


def names_fix(t):
    """stats::names_fix: a sha (7–40 hex, a digit and a letter) or `#N`."""
    hexy = lambda w: 7 <= len(w) <= 40 and re.fullmatch(r"[0-9a-fA-F]+", w) \
        and re.search(r"\d", w) and re.search(r"[a-fA-F]", w)
    return any(hexy(w) or re.fullmatch(r"#\d+", w) for w in words(t))


def code_spans(t):
    """query::code_spans, code spans only: a run of n backticks closes on n."""
    out, i = [], 0
    run = lambda k: len(t[k:]) - len(t[k:].lstrip("`"))
    while i < len(t):
        if t[i] != "`":
            i += 1
            continue
        n, j = run(i), i + run(i)
        while j < len(t) and not (t[j] == "`" and run(j) == n):
            j += run(j) if t[j] == "`" else 1
        if j < len(t):
            out.append(t[i + n:j])
            i = j + n
        else:
            i += n
    return out


def guard_paths(t):
    """The close's backticked spans holding a `/` (guard()'s reading), each
    as gone_refs normalizes it; a command or glob names no one file."""
    out = []
    for s in (s.strip() for s in code_spans(t)):
        if not s or "\n" in s or "://" in s or "/" not in s:
            continue
        p = s.removeprefix("/").removeprefix("./").split("#", 1)[0]
        p = re.sub(r"(:\d+){1,2}$", "", p)
        if p and not re.search(r"[\s?<>*]", p):
            out.append(p)
    return out


def latest_close(lines, row):
    """fix_close's input: the newest close record of row, else the folded one."""
    closes = [r for r in lines if r.get("ref") == row and "kind" not in r]
    if closes:
        return max(closes, key=lambda r: r["ts"])["text"]
    it = next((r for r in lines if r.get("id") == row), {})
    return (it.get("closed") or {}).get("text")


def ledger_rows(ws):
    root = os.path.join(ws, ".git", "fael", "log")
    return [json.loads(l) for d, _, fs in os.walk(root) for f in fs if f.endswith(".jsonl")
            for l in open(os.path.join(d, f)) if l.strip()]


def candidates(rows, files):
    """carry_line's pool per touched file: closed, not superseded issues whose
    newest close names a fix, newest first. ponytail: ULID order stands in for
    log order, and carry's SCAN/REACHED caps and fix_reached are left out
    (replica rewrites every sha, so all reach)."""
    gone = {r.get("supersedes") for r in rows}
    closes = {}
    for r in rows:
        if r.get("ref") and "kind" not in r and r["ts"] >= closes.get(r["ref"], {}).get("ts", ""):
            closes[r["ref"]] = r
    out = {}
    for r in rows:
        if r.get("kind") != "issue" or r["id"] in gone:
            continue
        close = (closes.get(r["id"]) or r.get("closed") or {}).get("text")
        if close and names_fix(close):
            for f in set(r.get("files", [])) & set(files):
                out.setdefault(f, []).append(r["id"])
    return {f: sorted(v, reverse=True) for f, v in sorted(out.items())}


def exists_at(src, rev, p):
    return subprocess.run(["git", "-C", src, "cat-file", "-e", f"{rev}:{p}"],
                          capture_output=True).returncode == 0


# --- one commit, one order ---
def hook(fael, ev, payload, ws, env):
    r = subprocess.run([fael, "hook", ev, "--client", "claude"], input=json.dumps(payload),
                       cwd=ws, env=env, capture_output=True, text=True, timeout=120)
    if r.returncode:
        return f"{ev} exit {r.returncode}: {r.stderr[:200]}"
    try:
        json.loads(r.stdout or "{}")
    except json.JSONDecodeError:
        return f"{ev} stdout not JSON: {r.stdout[:200]}"


def replay(a, pair, cand, order):
    """→ (said events, error or None, snapshot sha, gold pre-check facts)."""
    tdir = os.path.join(os.path.realpath(replica.ROOT), "pn-" + os.urandom(6).hex())
    ws = os.path.join(tdir, "repo")
    try:
        tip = replica.clone(replica.base(a.src, cand["parent"], False), ws)
        snap, _, ids = replica.snapshot(a.src, cand["parent"], replica.ts(cand["parent_time"]),
                                        cand["branch"], ws)
        env = trial.trial_env(tdir, "on")
        files = sorted(replica.paths(a.src, "diff", "--name-only", cand["parent"], cand["sha"]))
        if order == "reverse":
            files.reverse()
        base = {"session_id": "replay", "cwd": ws}
        err = None if pair["row"] in ids else "row not in snapshot"
        err = err or hook(a.fael, "session-start", base | {
            "hook_event_name": "SessionStart", "source": "startup"}, ws, env)
        for i, f in enumerate(files):
            if i == 0 or a.turns == "file":
                err = err or hook(a.fael, "prompt", base | {
                    "hook_event_name": "UserPromptSubmit", "prompt": "replay"}, ws, env)
            err = err or hook(a.fael, "edit", base | {
                "hook_event_name": "PostToolUse", "tool_name": "Edit",
                "tool_input": {"file_path": os.path.join(ws, f)}, "tool_response": {}}, ws, env)
        usage = os.path.join(env["FAEL_STATE_DIR"], "usage.jsonl")
        events = [json.loads(l) for l in open(usage)] if os.path.exists(usage) else []
        turn_of = turns_of(files, a.turns)
        for e in events:  # the prompt hook logs nothing: an event's turn is its file's
            e["turn"] = turn_of.get((e.get("files") or [None])[0], -1)
        err = err or turn_leak(events)
        rows = ledger_rows(ws)
        close = latest_close(rows, pair["row"])
        paths = guard_paths(close or "")
        pre = {"close": close, "names_fix": bool(close and names_fix(close)),
               "guard_paths": {p: exists_at(a.src, cand["parent"], p) for p in paths}}
        return events, err, {"snapshot_sha256": snap, "tip": tip, "files": files,
                             "candidates": candidates(rows, files)}, pre
    finally:
        shutil.rmtree(tdir, ignore_errors=True)


def turns_of(files, turns):
    """{file: the turn its edit ran in}."""
    return {f: i if turns == "file" else 0 for i, f in enumerate(files)}


def turn_leak(events):
    """Two per-turn lines in one turn mean the turn was not marked: the replay
    is not the session it claims to be. Counted per push and kind, not per
    `said` entry: one merge line names each of its ids."""
    n = Counter(e.get("turn", 0) for e in events
                for k in {s["kind"] for s in e.get("said", [])} if k in PER_TURN)
    over = {t: v for t, v in n.items() if v > 1}
    return f"per-turn lines said more than once in a turn: {over}" if over else None


def said(events):
    """{row id: first channel} in event order, plus touch@1's would_drop,
    each edit's cut records, {file: carry said there} and {turn: the
    per-turn line that spent it}."""
    first, drop, cut, carried, spent = {}, set(), {}, {}, {}
    for e in events:
        for s in e.get("said", []):
            if s.get("key") and s["kind"] in ("row", "carry", "check", "ask", "cited", "merge",
                                              "promote"):
                first.setdefault(s["key"], CHANNEL.get(s["kind"], "other"))
                if s["kind"] == "carry":
                    for f in e.get("files", []):
                        carried.setdefault(f, s["key"])
            if s["kind"] in PER_TURN:
                spent.setdefault(e.get("turn", 0), f"{s['kind']}:{s.get('key')}")
        if e.get("event") == "session-start":
            for i in e.get("ids", []):
                first.setdefault(i, "brief")
        drop |= set((e.get("would_drop") or {}).get("ids", []))
        for c in e.get("cut", []):
            cut.setdefault(c["id"], c["r"])
    return first, drop, cut, carried, spent


def reason(p, cand_row, first, cut, carried, pre, err, ctx=None):
    """Why a gold pair was not said (baseline): the channel's own rule, or not."""
    if err:
        return "replay_error"
    if not cand_row.get("overlap"):
        return "file_not_touched"
    if cand_row["why"] == "active_decision":
        return f"cut:{cut[p['row']]}" if p["row"] in cut else "unexplained"
    if not pre["names_fix"]:
        return "close_names_no_fix"
    other = [carried[f] for f in cand_row["overlap"] if carried.get(f, p["row"]) != p["row"]]
    if other:
        return f"not_newest_on_file:{other[0]}"
    # ctx (--turns): {file: turn}, carry's candidates, {turn: per-turn line}
    turn_of, cands, used = ctx or ({}, {}, {})
    turns = {turn_of.get(f, 0) for f in cand_row["overlap"]}
    # the turn's one carry went to another file's issue first
    spent = [i for f, i in carried.items() if i != p["row"] and turn_of.get(f, 0) in turns]
    if spent:
        return f"carry_spent_this_turn:{spent[0]}"
    # the file's newest was carried at an earlier file: Once::Key keeps it silent here
    hid = [i for f in cand_row["overlap"] for i in cands.get(f, [])
           if i > p["row"] and i in carried.values()]
    if hid:
        return f"not_newest_on_file:{hid[0]}"
    # check, merge or promote spent the turn first
    other = [used[t] for t in sorted(turns) if t in used]
    return f"turn_spent:{other[0]}" if other else "unexplained"


def tally(pairs):
    """§3.2's numbers for one order: counts with their denominators."""
    n = lambda **k: sum(all(p[x] == v for x, v in k.items()) for p in pairs)
    out = {}
    for lab in ("a", "b", "gold"):
        tot = {k: n(label=lab, kind=k) for k in ("active_decision", "closed_issue")}
        hit = {k: n(label=lab, kind=k, warned=True) for k in tot}
        out[lab] = {"warned": sum(hit.values()), "of": sum(tot.values()),
                    "by_kind": {k: [hit[k], tot[k]] for k in tot},
                    "by_channel": dict(Counter(p["channel"] for p in pairs
                                               if p["label"] == lab and p["warned"])),
                    "touch_would_drop": n(label=lab, would_drop=True)}
    ab = out["a"]["warned"] + out["b"]["warned"]
    carry = sum(out[x]["by_channel"].get("carry", 0) for x in ("a", "b"))
    ratio = lambda x, y: round(x / y, 4) if y else "undefined"
    out["noise_push_rate"] = ratio(ab, out["a"]["of"] + out["b"]["of"])
    out["guard_reach"] = ratio(carry, ab)
    out["c_warned"] = n(label="c", warned=True)
    return out


def absorb(agg, per, step):
    """Fold one finished pair (a progress.jsonl line) into the aggregate."""
    agg["pairs"].append(step["pair"])
    for o in ORDERS:
        per[o].append(step["per"][o])
        if step["errors"][o]:
            agg["errors"].setdefault(o, {})[step["pair"]["sha"]] = step["errors"][o]
        u = agg["unlabeled"].setdefault(o, {"warned": 0, "touch_would_drop": 0})
        u["warned"] += step["unlabeled"][o][0]
        u["touch_would_drop"] += step["unlabeled"][o][1]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("src")
    ap.add_argument("results")
    here = os.path.dirname(os.path.abspath(__file__))
    ap.add_argument("--fael", default=os.path.join(here, "..", "..", "target", "release", "fael"))
    ap.add_argument("--only", type=int, help="first n pairs (a smoke run)")
    ap.add_argument("--turns", choices=TURNS, default="commit")
    ap.add_argument("--out", default="push-noise", help="output dir under results")
    a = ap.parse_args()
    a.src, a.results, a.fael = map(os.path.abspath, (a.src, a.results, a.fael))
    screened_f = os.path.join(a.results, "screened.jsonl")
    cand_f = os.path.join(a.results, "cohort-1", "candidates.jsonl")
    screened = [json.loads(l) for l in open(screened_f)][:a.only]
    cands = {c["sha"]: c for c in map(json.loads, open(cand_f))}
    out_dir = os.path.join(a.results, a.out)
    os.makedirs(out_dir, exist_ok=True)
    os.makedirs(replica.ROOT, exist_ok=True)  # /tmp is cleared on reboot
    git_sha = subprocess.run(["git", "-C", here, "rev-parse", "HEAD"], capture_output=True,
                             text=True).stdout.strip()
    agg = {"meta": {
        "screened_sha256": sha256(open(screened_f, "rb").read()),
        "candidates_sha256": sha256(open(cand_f, "rb").read()),
        "fael_sha256": sha256(open(a.fael, "rb").read()),
        "fael_version": subprocess.run([a.fael, "--version"], capture_output=True,
                                       text=True).stdout.strip(),
        "worktree_head": git_sha,
        "replay_sha256": sha256(open(__file__, "rb").read()), "orders": ORDERS, "policies": ["baseline@1", "touch@1"],
        "turns": a.turns,
        "events": "SessionStart, then PostToolUse Edit per file C changed; a "
                  "UserPromptSubmit before the first edit (commit) or every edit (file)",
        "field_map": {"actual_warned": "usage said[kind∈row,carry,check,…].key ∪ "
                      "session-start ids", "channel": "said kind: row→push, carry, check, "
                      "brief, other", "touch@1": "usage would_drop.ids (shadow)",
                      "cut": "usage cut[].r (cap, hub_peek, budget, gate)"}},
        "orders": {}, "pairs": [], "unlabeled": {}, "errors": {}}
    started = time.strftime("%Y-%m-%dT%H:%M:%S%z")
    per = {o: [] for o in ORDERS}
    # each finished pair lands in progress.jsonl: a run cut short resumes after
    # the last one, if its inputs, binary, replay.py and --turns are the same
    run_key = {k: agg["meta"][k] for k in ("screened_sha256", "candidates_sha256", "fael_sha256",
                                           "replay_sha256", "turns")} | {"only": a.only}
    prog_f = os.path.join(out_dir, "progress.jsonl" if not a.only else f"progress-{a.only}.jsonl")
    done = [json.loads(l) for l in open(prog_f)] if os.path.exists(prog_f) else []
    done = done if all(d["key"] == run_key for d in done) else []
    keep = {(d["pair"]["sha"], d["pair"]["row"]) for d in done}
    for o in ORDERS:  # drop the events of a pair cut off before its progress line
        ef = os.path.join(out_dir, f"events-{o}.jsonl")
        old = open(ef).readlines() if os.path.exists(ef) else []
        open(ef, "w").writelines(l for l in old if tuple(json.loads(l)[k] for k in ("sha", "row"))
                                 in keep)
    open(prog_f, "w").writelines(json.dumps(d) + "\n" for d in done)
    for d in done:
        absorb(agg, per, d)
    for i, p in enumerate(screened):
        if i < len(done):
            continue
        cand = cands[p["sha"]]
        crow = next(r for r in cand["rows"] if r["id"] == p["row"])
        rec = {"sha": p["sha"], "row": p["row"], "label": LABELS[p["reason"]],
               "kind": crow["why"]}
        # replica.base()'s cache dir: one we build goes after, 255 would fill /tmp
        bdir = os.path.join(replica.ROOT, "base", f"{cand['parent'][:12]}-plain.git")
        kept = os.path.exists(bdir)
        step = {"key": run_key, "per": {}, "errors": {}, "unlabeled": {}, "events": {}}
        for o in ORDERS:
            events, err, ident, pre = replay(a, p, cand, o)
            step["events"][o] = events
            first, drop, cut, carried, spent = said(events)
            warned = p["row"] in first and not err
            r = rec | {"warned": warned, "channel": first.get(p["row"]) if warned else None,
                       "would_drop": p["row"] in drop, "snapshot_sha256": ident["snapshot_sha256"],
                       "tip": ident["tip"], "files": ident["files"],
                       "candidates": ident["candidates"]}
            if rec["label"] == "gold" and not warned:
                ctx = (turns_of(ident["files"], a.turns), ident["candidates"], spent)
                r["reason"] = reason(p, crow, first, cut, carried, pre, err, ctx)
            step["per"][o], step["errors"][o] = r, err
            others = set(first) - {p["row"]}
            step["unlabeled"][o] = [len(others), len(others & drop)]
        if not kept:
            shutil.rmtree(bdir, ignore_errors=True)
        rec["guard_precheck"] = pre if rec["kind"] == "closed_issue" else None
        step["pair"] = rec | {o: {k: v for k, v in step["per"][o].items() if k not in rec}
                              for o in ORDERS}
        for o, events in step.pop("events").items():
            with open(os.path.join(out_dir, f"events-{o}.jsonl"), "a") as f:
                f.writelines(json.dumps({"sha": p["sha"], "row": p["row"], **e}) + "\n"
                             for e in events)
        with open(prog_f, "a") as f:
            f.write(json.dumps(step) + "\n")
        absorb(agg, per, step)
        print(f"{i + 1}/{len(screened)} {p['sha'][:8]} {rec['label']}", flush=True)
    for o in ORDERS:
        agg["orders"][o] = tally(per[o]) | {"replay_error": len(agg["errors"].get(o, {}))}
    same = all(x[o]["snapshot_sha256"] == x[ORDERS[0]]["snapshot_sha256"]
               for x in agg["pairs"] for o in ORDERS)
    agg["meta"]["snapshots_equal_across_orders"] = same
    blob = json.dumps(agg, sort_keys=True, ensure_ascii=False).encode()
    doc = {"aggregate_sha256": sha256(blob), "started": started,
           "ended": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "aggregate": agg}
    name = "cohort-1.json" if not a.only else f"smoke-{a.only}.json"
    json.dump(doc, open(os.path.join(out_dir, name), "w"), indent=1, ensure_ascii=False)
    print(json.dumps({"aggregate_sha256": doc["aggregate_sha256"]} |
                     {o: {k: agg["orders"][o][k] for k in ("noise_push_rate", "guard_reach",
                                                          "replay_error")} for o in ORDERS}))


def selftest():
    """The fael-core twins against cases their Rust tests pin."""
    assert names_fix("fixed in e6deb61") and names_fix("see (#206)")
    assert not names_fix("Fixed on feat/vela-doc-page-round2") and not names_fix("deadbeef #x")
    assert code_spans("a `x` b ``y `z` y`` c `open") == ["x", "y `z` y"]
    assert guard_paths("guard `a/b.test.ts:12` and `./c/d.ts#x`, `e.rs`, "
                       "`fael find --files a/`, `e2e/<flow>.ts`") == ["a/b.test.ts", "c/d.ts"]
    # one turn: a second carry is a leak; a gold issue the turn's carry
    # skipped reads as spent, one beaten on its own file as not newest
    ev = [{"event": "edit", "files": ["x.ts"], "said": [{"kind": "carry", "key": "I1"}]},
          {"event": "edit", "files": ["y.ts"], "said": [{"kind": "row", "key": "D1"}]}]
    assert turn_leak(ev) is None and turn_leak(ev + ev[:1])
    assert turn_leak([{"said": [{"kind": "merge", "key": k} for k in "ABC"]}]) is None
    first, _, cut, carried, _ = said(ev)
    assert first == {"I1": "carry", "D1": "push"} and carried == {"x.ts": "I1"}
    row = lambda files: {"why": "closed_issue", "overlap": files}
    pre = {"names_fix": True}
    assert reason({"row": "G"}, row(["y.ts"]), first, cut, carried, pre, None) \
        == "carry_spent_this_turn:I1"
    assert reason({"row": "G"}, row(["x.ts"]), first, cut, carried, pre, None) \
        == "not_newest_on_file:I1"
    assert reason({"row": "G"}, row(["x.ts"]), first, cut, carried,
                  {"names_fix": False}, None) == "close_names_no_fix"
    # a turn per file: x's carry spends turn 0 only; gold 01B on y loses to
    # its newer 01C said at x (Once::Key), or to a check that spent turn 1
    ev = [{"event": "edit", "files": ["x.ts"], "turn": 0, "said": [{"kind": "carry", "key": "01C"}]},
          {"event": "edit", "files": ["y.ts"], "turn": 1, "said": [{"kind": "check", "key": "01D"}]}]
    assert turn_leak(ev) is None and turn_leak(ev + ev[1:])
    first, _, cut, carried, used = said(ev)
    assert used == {0: "carry:01C", 1: "check:01D"}
    turn_of = turns_of(["x.ts", "y.ts"], "file")
    g = lambda cands: reason({"row": "01B"}, row(["y.ts"]), first, cut, carried, pre, None,
                             (turn_of, cands, used))
    assert g({"y.ts": ["01C", "01B"]}) == "not_newest_on_file:01C"
    assert g({"y.ts": ["01B"]}) == "turn_spent:check:01D"
    assert candidates([{"id": "01B", "kind": "issue", "files": ["y.ts"]},
                       {"ref": "01B", "ts": "1", "text": "fixed in e6deb61"},
                       {"id": "01A", "kind": "issue", "files": ["y.ts"]},
                       {"ref": "01A", "ts": "1", "text": "by design"}], ["y.ts"]) \
        == {"y.ts": ["01B"]}


if __name__ == "__main__":
    selftest()
    main()
