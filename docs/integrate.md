# Integrate fael into an agent

fael needs four calls from an agent's lifecycle. Each is one process spawn:
Event JSON on stdin, Reply JSON on stdout, always exit 0 (about 10 ms each on a repo with a thousand rows; `session-start` about 35 ms on a small log, 100 ms measured on ~1,500 rows before it
dropped two spawns; once per session, a git spawn costs ~8 ms). Claude Code and Codex use a built-in adapter
(`--client claude|codex`), and OpenCode gets a generated plugin
([`fael/skill/opencode.js`](../fael/skill/opencode.js), the reference implementation of
this page). Any other agent calls the neutral format:

```
fael hook <stop|session-start|read|edit|search|prompt>   < Event   > Reply

Event  {"cwd": str, "session": str, "client": str, "files": [str], "stop_active": bool, "text": str, "reply": str,
        "agent"?: str, "source"?: str, "tool"?: str, "tool_input"?: obj, "tool_response"?: obj}
Reply  {"block": bool, "reason"?: str, "context"?: str, "notice"?: str}
```

| When | Call | Send | Do with the Reply |
|---|---|---|---|
| session starts | `session-start` | `cwd`, `session` | put `context` in the system prompt / first turn |
| a file was read | `read` | `cwd`, `session`, `files` | append `context` to the tool result |
| a file was written | `edit` | `cwd`, `session`, `files` | append `context` to the tool result |
| a file was found via search or the shell | `search` | `cwd`, `session`, plus `files` — or the raw call as `tool`, `tool_input`, `tool_response` | append `context` to the tool result |
| the user sent a prompt | `prompt` | `cwd`, `session`, `text` (the prompt) | add `context` to that turn — one pointer line when a prompt word equals the head of an open key (`credit` in `vela:credit-ledger`), each key once per session |
| the agent is about to end its turn | `stop` | `cwd`, `session`, `reply`, `text`, `stop_active` | files the reply's `fael <kind>:` lines and never blocks; only under `[capture] block = true` does `block` mean: do not end, send `reason` back as the next message |

`fael install` wires every event above on **Claude Code**. **Codex has no prompt hook** (its
hooks know no UserPromptSubmit), so the pointer-line hint is Claude-only there — deliberate
(its Stop/session-start/search/edit hooks are wired). A custom client can still call
`prompt` from its own lifecycle; the adapter accepts it for both (`--client claude|codex`).

- `session` — an RFC 3339 time the session started (`2026-09-25T10:00:00.000Z`), or a transcript
  file whose birthtime is the start. **Send the exact same string to `read`, `edit` and `stop`**: it keys the
  session's edit list, stop blocks when files were edited after the newest row, and the read/edit push
  says each row once per session — without it every read re-pushes the same rows at full budget.
- `files` — absolute, or relative to `cwd`. Paths outside the repo are dropped.
- `notice` — one line for the **user**, never the agent: which issues the brief handed over, which
  decision or issue a push reminded the agent of (once per file per session), and the turn's receipt
  on `stop`. Show it where only the user sees it (a toast, a status line); a client with no such
  channel drops it. Never put it in `context`. `[notify] user = false` turns it off.
- `reply` — the assistant's **last message only**. fael files its `fael decision|issue|note: <text> [files: a,b]`
  lines (column 0, outside code fences); leave it out and fael reads the last message of `session` as a Claude
  transcript. Send it once per turn end — the same lines twice in a session file once.
- `text` — the assistant's text in this session (or at least the last message). The issue rule
  looks for "found a bug", "inconsistent", "might break", and similar; leave it out and fael reads `session` as a Claude transcript.
- `stop_active` — `true` when this stop comes right after one you blocked, so it never loops.
  fael also blocks each problem only once per session.
- `agent` — only when the event fires inside a sub-agent: any id that is stable for that sub-agent. A
  sub-agent is its own context window, so `read`/`edit` keep a separate said-once list per `agent`
  (it gets the rows its parent was already told), and a `stop` with `agent` only files that `reply`'s
  lines — it never blocks. Leave it out on the session's own thread. A client whose sub-agents already
  have their own `session` needs nothing here.
- `source` — on `session-start`, send `"compact"` right after the client compacted its context: the
  pushed rows are gone from it, so fael starts the said-once list over.
- `client` — a name for `fael stats`.

Any error, missing field or repo without `.fael/` is `{"block": false}`. Integration cannot break the agent.

Also register `fael mcp` (stdio, tools `find` / `add` / `close`). If the agent can load skills,
use the one `fael install` writes (`~/.claude/skills/fael/SKILL.md`).

## One worktree per agent

Two agents (or two sessions) sharing one worktree folder share its HEAD and index with no
coordination: one checks out its branch and the other is suddenly on the wrong branch, so its
next push or PR targets work it never meant to touch. fael does not guard this — it is a log,
not a daemon, and each row already records the branch it was filed on. The fix is structural:
give each agent its own worktree (`git worktree add ../wt-<agent>`) and never run two agents in
the same one. `fael find --branches` reads the other worktree's unmerged rows without checking
anything out, and `fael doctor` flags local branches whose PR already merged (`[Merged]`) for
deletion.

## Moving a tracked repo to local

A repo with no `store` line stops writing `.fael/log` on upgrade: the tree log is read as frozen
history and new rows stay in the journal. A repo that set `store = "tracked"` keeps committing
`.fael/log`, and every PR that appends to the same month file conflicts on GitHub (it ignores
`merge=union`). To stop, or to fold the tree log in before removing it:

```bash
fael migrate local        # fold .fael/log into this clone's journal, set store = "local"
git add .fael/config.toml && git commit -m "fael: keep rows out of the tree"
```

The simplest end state keeps `.fael/log` as frozen history: nothing appends to it any more, so it
never conflicts again, and every clone keeps reading it. If you remove it instead
(`git rm -r --cached .fael/log`), run `fael migrate local` in **every** clone before it pulls the
removal — the fold reads the working tree, and it is what makes a row edited in the tree (say, a PR
that renamed its files) win over the stale copy in that clone's journal.

## Private memory repo (public source, private memory)

A public repo should not publish its team's memory. Keep the source on GitHub and the memory in a
separate private Git repo: rows go to a ref under `refs/fael/` on **that** remote, never to `origin`.
Format and push rules: [`sync-format.md`](sync-format.md).

Once per source repo — commit this, it holds no memory, only the switch that keeps rows out of the tree:

```bash
mkdir -p .fael && echo 'store = "local"' > .fael/config.toml
git add .fael/config.toml && git commit -m "fael: keep rows out of the tree"
```

Once per machine (`fael.remote` lives in `.git/config` and is never committed; auth is your own Git
credential; the URL can be any private Git repo, Gitea/Forgejo, or a bare repo on a NAS):

```bash
git config fael.remote git@github.com:acme/my-project-memory.git   # an empty private repo
fael sync                       # push your rows, ingest everyone else's
```

A fresh clone gets the team's memory back with the same two commands — the source carries no
`.fael/log`, the rows arrive from the private remote into the local journal:

```bash
git clone git@github.com:acme/my-project.git && cd my-project
git config fael.remote git@github.com:acme/my-project-memory.git
fael sync                       # synced: pushed 0, ingested N
fael find --all
```

Point `fael.remote` at `origin` on a public repo and `fael sync` warns (the ref is fetchable by
anyone).

With `fael.remote` set, the Stop hook also runs `fael sync` in the background — once per session for each newest row you have filed, so a Stop with nothing new starts nothing: a dead
network or a failed login skips it (the last run's output is `auto-sync-<hash>.log`, one per worktree, in fael's per-machine
state dir) and never holds the turn. Rows filed after a session's last stop ship with the next session's first, or run
`fael sync` yourself. Turn it off with `[sync] auto = false` in `.fael/config.toml`; there is no daemon.
