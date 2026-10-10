#!/usr/bin/env python3
"""PLAN-fael-onoff: what fael pushed in a trial and what the agent did with it.

Reads <trial>/transcript.jsonl (stream-json with hook events). A row is pushed
when its id is in a fael hook reply; it is used when, after that, the agent
names it: a fael verb on its id or key (find/claim/close/bump/add, CLI or MCP)
or the id/key in its own text or another tool call. Writes <trial>/acted.json
and prints one line per trial. Counts only, never a verdict on value.

Usage: scripts/onoff/acted.py <trial-dir>...
"""
import json
import os
import re
import sys

# fael ids are ULIDs printed as their shortest unique prefix, 8 chars or more
ID = re.compile(r"(?<![0-9A-Z])01[0-9A-HJKMNP-TV-Z]{6,24}(?![0-9A-Z])")
ROW = re.compile(r"\[(01\w+)\]\s+(?:\([^)]*\)\s+)?(\w+)")
KEY = re.compile(r"#([\w:.-]+)")
PROMPT_KEY = re.compile(r"([\w:.-]+) \(\d+, via")
VERB = re.compile(r"\bfael\s+(\w+)([^;&|\n]*)")
# a line naming a row without its "[id] kind" form says why it came
WHY = [("broke before", "broke"), ("changed since", "stale"), ("done with one?", "close-nudge")]


def same(a, b):
    if "#" in (a[0], b[0]):  # a prompt key: vela:credit-note-ref is not -refund-print
        return a == b
    # ponytail: prefix match, two rows sharing an 8-char prefix would merge
    return a.startswith(b) or b.startswith(a)


def hook_text(v):
    out = v.get("output") or v.get("stdout") or ""
    try:
        return json.loads(out)["hookSpecificOutput"]["additionalContext"]
    except (ValueError, KeyError, TypeError):
        return out


def pushes(i, event, text, rows):
    for line in text.splitlines():
        m = ROW.search(line)
        found = [(m.group(1), m.group(2), KEY.findall(line.split("→")[0]))] if m else [
            (t, next((w for p, w in WHY if p in line), "named"), []) for t in ID.findall(line)]
        if "names open key" in line:  # the prompt hook names keys, not ids
            found = [("#" + k, "prompt-key", [k]) for k in PROMPT_KEY.findall(line)]
        for rid, kind, keys in found:
            r = next((r for k, r in rows.items() if same(k, rid)), None)
            if r is None:
                r = rows[rid] = {"id": rid, "kind": kind, "keys": [], "first": i,
                                 "events": {}, "actions": []}
            r["keys"] += [k for k in keys if k not in r["keys"]]
            r["events"][event] = r["events"].get(event, 0) + 1


def actions(i, block):
    """(verb, ids, keys) the agent's text or tool call names."""
    if block.get("type") == "text":
        s = block["text"]
        yield "cited", ID.findall(s), KEY.findall(s)
        return
    if block.get("type") != "tool_use":
        return
    name, s = block["name"], json.dumps(block["input"], ensure_ascii=False)
    if "fael" in name:  # MCP: mcp__fael__find {"id": ..., "key": ...}
        inp = block["input"]
        yield name.rsplit("__", 1)[-1], ID.findall(s), [inp["key"]] if inp.get("key") else []
        return
    verbs = list(VERB.finditer(s))
    for m in verbs:
        yield m.group(1), ID.findall(m.group(2)), re.findall(r"--key[= ]+([\w:.-]+)", m.group(2))
    rest = VERB.sub("", s)
    yield "cited", ID.findall(rest), KEY.findall(rest)


def tally(tdir):
    rows, task = {}, None
    man = os.path.join(tdir, "manifest.json")
    if os.path.exists(man):
        m = re.search(r"'row': '(\w+)'", str(json.load(open(man)).get("task")))
        task = m and m.group(1)
    for i, line in enumerate(open(os.path.join(tdir, "transcript.jsonl"))):
        try:
            v = json.loads(line)
        except json.JSONDecodeError:
            continue
        if "hook" in str(v.get("subtype", "")):
            pushes(i, v.get("hook_event", "?"), hook_text(v), rows)
        elif v.get("type") == "assistant":
            for b in v.get("message", {}).get("content", []):
                for verb, ids, keys in actions(i, b):
                    for r in rows.values():
                        if i > r["first"] and (any(same(t, r["id"]) for t in ids)
                                               or set(keys) & set(r["keys"])):
                            r["actions"].append(verb)
    for r in rows.values():
        r["task_row"] = bool(task and same(task, r["id"]))
    used = [r for r in rows.values() if r["actions"]]
    out = {"trial": os.path.basename(os.path.normpath(tdir)), "pushed": len(rows),
           "used": len(used), "task_row": next(
               ({"pushed": True, "actions": r["actions"]} for r in rows.values() if r["task_row"]),
               {"pushed": False}), "rows": sorted(rows.values(), key=lambda r: r["first"])}
    json.dump(out, open(os.path.join(tdir, "acted.json"), "w"), indent=1, ensure_ascii=False)
    return out


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    for t in sys.argv[1:]:
        o = tally(t)
        print(f"{o['trial']}: pushed {o['pushed']} · used {o['used']} · task row {o['task_row']}")
        for r in o["rows"]:
            if r["actions"]:
                print(f"  {r['id']} {r['kind']} {' '.join('#' + k for k in r['keys'])}"
                      f" ← {', '.join(sorted(set(r['actions'])))}")


if __name__ == "__main__":
    main()
