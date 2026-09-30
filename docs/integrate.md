# Integrate fael into an agent

fael needs four calls from an agent's lifecycle. Each is one process spawn:
Event JSON on stdin, Reply JSON on stdout, always exit 0 (about 2–3 ms each; `session-start` about 10 ms
because it runs one `git check-ignore`). Claude Code and Codex use a built-in adapter
(`--client claude|codex`), and OpenCode gets a generated plugin
([`fael/skill/opencode.js`](../fael/skill/opencode.js), the reference implementation of
this page). Any other agent calls the neutral format:

```
fael hook <stop|session-start|read|edit>   < Event   > Reply

Event  {"cwd": str, "session": str, "client": str, "files": [str], "stop_active": bool, "text": str, "reply": str}
Reply  {"block": bool, "reason"?: str, "context"?: str}
```

| When | Call | Send | Do with the Reply |
|---|---|---|---|
| session starts | `session-start` | `cwd`, `session` | put `context` in the system prompt / first turn |
| a file was read | `read` | `cwd`, `session`, `files` | append `context` to the tool result |
| a file was written | `edit` | `cwd`, `session`, `files` | append `context` to the tool result |
| the agent is about to end its turn | `stop` | `cwd`, `session`, `reply`, `text`, `stop_active` | files the reply's `fael <kind>:` lines and never blocks; only under `[capture] block = true` does `block` mean: do not end, send `reason` back as the next message |

- `session` — an RFC 3339 time the session started (`2026-09-25T10:00:00.000Z`), or a transcript
  file whose birthtime is the start. **Send the exact same string to `read`, `edit` and `stop`**: it keys the
  session's edit list, stop blocks when files were edited after the newest row, and the read/edit push
  says each row once per session — without it every read re-pushes the same rows at full budget.
- `files` — absolute, or relative to `cwd`. Paths outside the repo are dropped.
- `reply` — the assistant's **last message only**. fael files its `fael decision|issue|note: <text> [files: a,b]`
  lines (column 0, outside code fences); leave it out and fael reads the last message of `session` as a Claude
  transcript. Send it once per turn end — the same lines twice in a session file once.
- `text` — the assistant's text in this session (or at least the last message). The issue rule
  looks for "found a bug", "inconsistent", "might break", and similar; leave it out and fael reads `session` as a Claude transcript.
- `stop_active` — `true` when this stop comes right after one you blocked, so it never loops.
  fael also blocks each problem only once per session.
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

With `fael.remote` set, the Stop hook also runs `fael sync` once per session, in the background: a dead
network or a failed login skips it (the last run's output is `auto-sync.log` in fael's per-machine state
dir) and never holds the turn. Rows filed after that first stop ship with the next session's, or run
`fael sync` yourself. Turn it off with `[sync] auto = false` in `.fael/config.toml`; there is no daemon.
