# Integrate fael into an agent

fael needs four calls from an agent's lifecycle. Each is one process spawn:
Event JSON on stdin, Reply JSON on stdout, always exit 0 (under 10 ms each on a repo with ~1,500 rows; `session-start` about 45 ms, once per session,
most of it process spawns at ~8 ms per git call). Claude Code and Codex use a built-in adapter
(`--client claude|codex`), and OpenCode gets a generated plugin
([`fael/skill/opencode.js`](../fael/skill/opencode.js), the reference implementation of
this page). Any other agent calls the neutral format:

```
fael hook <stop|session-start|read|edit|search|prompt>   < Event   > Reply

Event  {"cwd": str, "session": str, "client": str, "files": [str], "text": str, "reply": str,
        "agent"?: str, "source"?: str, "tool"?: str, "tool_input"?: obj, "tool_response"?: obj}
Reply  {"block": false, "context"?: str, "notice"?: str}
```

| When | Call | Send | Do with the Reply |
|---|---|---|---|
| session starts | `session-start` | `cwd`, `session` | put `context` in the system prompt / first turn |
| a file was read | `read` | `cwd`, `session`, `files` | append `context` to the tool result |
| a file was written | `edit` | `cwd`, `session`, `files` | append `context` to the tool result |
| a file was found via search or the shell | `search` | `cwd`, `session`, plus `files` — or the raw call as `tool`, `tool_input`, `tool_response` | append `context` to the tool result |
| the user sent a prompt | `prompt` | `cwd`, `session`, `text` (the prompt) | add `context` to that turn — one pointer line when a prompt word equals the head of an open key (`credit` in `vela:credit-ledger`), each key once per session |
| the agent is about to end its turn | `stop` | `cwd`, `session`, `reply`, `text` | nothing to do: fael files the reply's `fael <kind>:` lines and never blocks (`block` is always `false`, kept for old integrations) |

`fael install` wires every event above on **Claude Code**. **Codex has no prompt hook** (its
hooks know no UserPromptSubmit), so the pointer-line hint is Claude-only there — deliberate
(its Stop/session-start/search/edit hooks are wired). A custom client can still call
`prompt` from its own lifecycle; the adapter accepts it for both (`--client claude|codex`).

- `session` — an RFC 3339 time the session started (`2026-09-25T10:00:00.000Z`), or a transcript
  file whose birthtime is the start. **Send the exact same string to `read`, `edit` and `stop`**: it keys the
  session's edit list (`fael add` without `--files` files them), stop's bug line for the next push, and the read/edit push
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
the same one. Every worktree of a clone shares one journal, so a row filed in one is read
in all of them, and `fael doctor` flags local branches whose PR already merged (`[Merged]`) for
deletion.

## Rows from another clone

Rows live in the clone's journal (`.git/fael/log/<writer>/`), never in the tree, so a public repo
publishes no memory and a PR carries no log lines. Every worktree and agent on the machine reads
the same journal. To bring in rows from another clone or machine, copy its log over and import it:

```bash
fael import ../other-clone/.git/fael/log   # that clone's journal; importing twice is safe
fael import old.jsonl --map src/=lib/      # one log file, a path prefix rewritten on the way in
```

Sharing between people, continuously, is the separate fael-sync service, not part of fael.
