# Integrate fael into an agent

fael needs six calls from an agent's lifecycle. Each is one process spawn:
Event JSON on stdin, Reply JSON on stdout, always exit 0 (under 10 ms each on a repo with ~2,000 rows; `session-start` about 30 ms, once per session,
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
| the user sent a prompt | `prompt` | `cwd`, `session`, `text` (the prompt) | nothing: silent no-op; no turn state or context is written |
| the agent is about to end its turn | `stop` | `cwd`, `session`, `reply`, `text` | nothing to do: fael files the reply's `fael <kind>:` lines and never blocks (`block` is always `false`, kept for old integrations) |

OpenCode 2 replaced its plugin API, so the one file carries two implementations:
OpenCode 1 calls the named `Fael` export, OpenCode 2 loads the default
`{ id, setup }` definition and registers each event on the domain that owns the
operation. Two OpenCode 2 limits: a server plugin has no toast (measured on
2.0.29 — the plugin ctx carries no `tui`/`notify`, and `event` is
subscribe-only; toasts live in CLI plugins behind a separate `./tui`
entrypoint), so the user line (`notice`) goes to an out-of-band per-session
file, `$TMPDIR/fael-notice-<sessionID>.log`, one line per beat, never into
model content — tail it to watch receipts live; and its shell hook carries no
session id, so `FAEL_SESSION` comes from matching the shell's command against
recent shell tool calls — exact command (plus cwd when known), consumed once.
Zero or several matches leave it unset: identical concurrent commands warn each
candidate's notice file, and `fael add` without `--files` there needs a single
active session or explicit `--files`.

`fael install` wires **Codex** Stop, SessionStart, SubagentStart, SubagentStop,
UserPromptSubmit and shell/search/edit hooks. UserPromptSubmit calls the same silent no-op adapter as Claude Code; it
adds no context and keeps no per-turn edit-ask counter. New or changed Codex hooks
require user review and trust through `/hooks`; installation does not trust them.
See the [Codex hook contract](https://learn.chatgpt.com/docs/hooks).

Codex hook state uses `session_id`, matching the shell's `CODEX_THREAD_ID`.
Rollout filenames include a timestamp before that id, so their stems are not
session identities. The transcript remains separate evidence for Stop's start
time. Subagent lifecycle hooks carry the parent's `session_id`: fael uses the
child's `agent_id`, with `agent_transcript_path` kept separate, and never falls
back to the parent when the child id is missing. Observed child tool hooks also
carry the parent `session_id` and the child `agent_id`; their `transcript_path`
names the child. They use that same child id for state and shell attribution. A resume keeps said-once state; `SessionStart` with `source: compact`
resets only that thread's context.

Stdio MCP has no per-request caller binding. Its inherited environment and even
a recorded editor cannot identify the calling chat. MCP `add` therefore requires
explicit `files` (inside each row when batching), writes no writer session, and
`find`/`add` never consume a hook session's said-once state. A later edit may repeat
a row read through MCP. For session attribution, use the shell CLI with its
verified thread id. Concurrent rows can make file derivation return `files is
required`; naming the files explicitly is the fallback, never another chat's edits.

Evidence baseline: CLI `0.166.0-alpha.5.1`, bundled with Desktop `26.1027.30824`,
checked on 2026-10-31. Redacted real start/edit/stop payloads live in
`fael/tests/hook/fixtures/codex-*.json`. Concurrent CLI probes and resume verified
shell thread ids versus unbound MCP writes. A real child tool/shell probe
confirmed parent/agent identity fields. An app-server compact plus continuation
emitted the real `SessionStart` with `source: compact`, retained as a fixture.
Workspace-write probes received
`lock: Operation not permitted` at the `.git/fael` journal; attribution writes
were checked in an unrestricted scratch repo. Desktop GUI lifecycle and newly
trusted subagent lifecycle wiring still require the runtime acceptance pass;
contract tests alone do not establish full parity.

- `session` — an RFC 3339 time the session started (`2026-09-25T10:00:00.000Z`), or a transcript
  file whose birthtime is the start. **Send the exact same string to `read`, `edit` and `stop`**: it keys the
  session's edit list (`fael add` without `--files` files them), stop's bug line for the next push, and the read/edit push
  says each row once per session — without it every read re-pushes the same rows at full budget.
- `files` — absolute, or relative to `cwd`. Paths outside the repo are dropped.
- `notice` — one line for the **user**, never the agent: which issues the brief handed over, which
  decision or issue a push reminded the agent of (once per file per session), and the turn's receipt
  on `stop`. Show it where only the user sees it (a toast, a status line); a client with no such
  channel drops it (Codex is dropped too: its `systemMessage` is a UI/event-stream warning not shown to stay out of model context). Never put it in `context`. `[notify] user = false` turns it off.
- `reply` — the assistant's **last message only**. fael files its `fael decision|issue|note: <text> [files: a,b]`
  lines (column 0, outside code fences); leave it out and fael reads the last message of `session` as a Claude
  transcript. Send it once per turn end — the same lines twice in a session file once.
- `text` — the assistant's text in this session (or at least the last message). The issue rule
  looks for "found a bug", "inconsistent", "might break", and similar; leave it out and fael reads `session` as a Claude transcript.
- `agent` — only when the event fires inside a sub-agent: any id that is stable for that sub-agent. A
  sub-agent is its own context window, so `read`/`edit` keep a separate said-once list per `agent`
  (it gets the rows its parent was already told), and a `stop` with `agent` only notes that `reply`'s
  cited ids — it never blocks, stashes no bug line, sends no receipt. Leave it out on the session's
  own thread. A client whose sub-agents already have their own `session` needs nothing here, except
  OpenCode: its sub-agents are child sessions (`parentID` names the parent), so the child keeps its
  own `session` string — its own said-once list, shared with its brief and pushes — and sends `agent`
  = the serving root session only on `stop`.
- `source` — on `session-start`, send `"compact"` right after the client compacted its context: the
  pushed rows are gone from it, so fael starts the said-once list over.
- `client` — a name for `fael stats`.

Any error, missing field or repo without `.fael/` is `{"block": false}`. Integration cannot break the agent.

Also register `fael mcp` (stdio, tools `find` / `add` / `close`, plus `chunk` for fael crew). If the agent can load skills,
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
fael import ../other-clone               # that clone's journal (+ any .fael/log); importing twice is safe
fael import old.jsonl --map src/=lib/      # one log file, a path prefix rewritten on the way in
```

To share between clones through git instead, see [sync-format.md](sync-format.md):

```bash
git config fael.remote git@host:team/repo-ledger.git   # a private remote, set on purpose
fael sync                                              # push this writer's rows, ingest the others'
```

Hosted sync with a web app for non-dev members is the separate fael-sync service, not part of fael.

## A shared node for non-dev members (free VM, no secrets here)

An independent fael node on a free VM lets a Claude chat (iPhone included) read and add rows
in a repo's ledger. It is not the production host, never touches its DB or secrets, and
converges with your machine only through `fael sync`. fael adds no daemon: the bridge and the
tunnel are off-the-shelf processes you run.

```text
Claude (iPhone app / claude.ai) ──HTTPS──> Cloudflare Access ──> cloudflared (outbound only)
   └ request headers: service token            └ refuses without it   └ stdio->HTTP MCP bridge
                                                                         └ fael mcp --pin  (cwd = the clone)
```

### 1. Pick the VM — the invoice must read $0.00

Checked 2026-10-21 against the providers' own pages:

* **Oracle Always Free** ([resources](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm)):
  `VM.Standard.E2.1.Micro` (1 GB, one VNIC with one public IP, 10 TB/month outbound, 200 GB
  block storage) or `VM.Standard.A1.Flex` (up to 2 OCPU / 12 GB). Idle reclamation applies
  (7 days under 20% CPU/network, plus memory on A1) — a cron plus a live tunnel is traffic, but
  watch for it. Pick this one.
* **Google e2-micro** ([free tier](https://docs.cloud.google.com/free/docs/free-cloud-features)):
  compute (1 VM, `us-west1`/`us-central1`/`us-east1`) and 30 GB standard disk are free, but that
  page says nothing on an external IPv4 or Cloud NAT, which bill on their own pricing pages.
  Use it only if you confirm those two lines on the invoice are $0.

Do not trust the SKU: after the first month open the real invoice and check it reads $0.00.
Open no inbound port (security list: ingress none) — `cloudflared` dials out.

### 2. Install on the VM

```bash
# fael: the linux release binary from the GitHub release, on PATH
fael --version
git clone <vela-repo-url> ~/vela && cd ~/vela     # deploy key, read/write only the ledger remote
git config fael.remote <the private ledger remote>  # set on purpose; see Rows from another clone
fael sync
```

No DB, no Redis, no Docker.

### 3. Serve the ledger

Run `fael mcp --pin` from the clone behind any stdio->HTTP MCP bridge that serves Streamable
HTTP (e.g. `npx -y supergateway --stdio "fael mcp --pin" --outputTransport streamableHttp
--port 8000`), bound to `127.0.0.1` only. `--pin` makes the server act on its own cwd: a call
with a `cwd` arg is rejected (`this server is pinned to one workspace — drop cwd`), so no caller
can reach another directory on the host. Never expose a non-pinned `fael mcp` over HTTP.

### 4. Tunnel and auth

1. Cloudflare Zero Trust: create a Tunnel (`cloudflared tunnel create fael-node`), route
   `fael.<your-domain>` to `http://127.0.0.1:8000`, run it under systemd.
2. Access: add a self-hosted application for that hostname, a policy with action **Service Auth**
   whose selector is one service token, and create the token (Client ID + Secret).
3. In Claude (web, Settings > Connectors > Add custom connector): URL
   `https://fael.<your-domain>/mcp`, no sign-in, **Request headers**
   `CF-Access-Client-Id` and `CF-Access-Client-Secret` ([connector docs](https://support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp)).
   Custom connectors are available on Free (one connector), Pro, Max, Team and Enterprise, and are
   brokered through the account, so they work in the mobile apps; add it on the web. Each person
   uses their own Claude account. Static headers match Access service-token auth; the OAuth
   alternatives (Access MCP servers / managed OAuth) need an IdP and a server that validates the
   Access JWT, which `fael mcp` does not.
4. The secret lives only in Cloudflare and the connector; never in this repo or a row.

Check Cloudflare's current free-plan limits and the connector's mobile behaviour on the pages
above before relying on them; neither was readable in full when this was written.

### 5. Keep it converged

```bash
*/5 * * * * cd ~/vela && fael sync >/dev/null 2>&1   # crontab -e
```

### 6. Verify

```bash
curl -s -o /dev/null -w '%{http_code}\n' https://fael.<your-domain>/mcp                       # refused: 302/401/403
curl -s https://fael.<your-domain>/mcp -H 'CF-Access-Client-Id: …' -H 'CF-Access-Client-Secret: …' \
  -H 'content-type: application/json' -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'                                       # find, add, close, chunk
```

A `find` with `"cwd":"/tmp"` must return `isError` (pinned).

### Stop and tear down

```bash
sudo systemctl disable --now cloudflared fael-bridge
cloudflared tunnel delete fael-node      # then delete the Access app and the service token
```

Then delete the VM and the clone; rows already synced remain on the ledger remote.
