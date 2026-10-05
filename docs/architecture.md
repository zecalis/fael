# fael architecture

> **Status:** the log format and storage (§2, [format.md](format.md)) are implemented in `fael-core`, and so are
> `add` `close` `find` `keys` `kickoff` `mv` in the `fael` CLI (`find` and `kickoff` take
> `--branches` to read unmerged branches without a checkout), `fael mcp`
> (stdio, 3 tools), `fael hook <stop|session-start|read|edit|search|prompt>` (neutral + claude/codex adapters) with
> per-machine usage accounting (`fael stats`, `fael report`), `fael install` (Claude Code, Codex, OpenCode),
> and the maintenance commands `fael doctor [--fix]` · `fael compact` · `fael import` (SPEC §6, §11) ·
> `fael purge` (delete a leaked row for good — the one deliberate exception to the append-only log).
> This page is the contract the code is built against —
> when code and this page disagree, fix one of them in the same commit.

fael is a shared work ledger for agents that lives **inside the repo**: every agent (Claude Code, Codex, OpenCode, a chat
host speaking MCP, …) and every person on the team reads and writes the same log, git carries it between machines, and there is no server.

Three ideas carry the whole design:

1. **The log is the truth; everything else is derived** — like Redis's append-only file or a Kafka topic.
2. **Agents write in the same message, never a turn of their own** — `fael add` beside the next tool call, or `fael <kind>:` lines closing the reply; fael never refuses to end a turn.
3. **Memory comes to the agent** — when an agent reads a file, the rows about that file are attached to the read.

---

## 0. Product invariant

Fael is the **team's shared work ledger**: the context of the work, and the work handed
between sessions and agents — hand-offs, requirements, assigned issues (`--to`), claims,
come-backs (`--revisit`). It is not a personal notebook, not a generic AI memory store, and not a
process or git guard.

The unit of value is work context:

```
task (its `key`) → decision → evidence → outcome → closure
```

Fael resolves, reconciles, or self-heals **deterministic** context before asking an agent
to reason again; when it cannot decide, it exposes the evidence and never guesses (§6).
The goal is not to store more memory — it is memory that **points the right way**. Git owns
what changed; fael keeps only what git and the code cannot say (why, what was rejected, what is
unfinished), and retires a row once the code says it or contradicts it. It is measured by
repeat mistakes (redoing what was decided or rejected), the share of pushed rows actually used,
how many open rows the code has outgrown, and how much of what one agent wrote reached
another (`fael stats` → `across agents`). Rounds and tokens are a cost to keep low, never
a promise.

A feature ships only if it does at least one of these **and** gives nothing back:

- preserves work context
- recovers or repairs context
- retrieves the right task context
- reduces repeated agent work
- improves team reuse or coordination

A feature must never:

- start a new agent turn, or re-prompt on idle. Capture rides the agent's own reply
  (`fael <kind>: … [files: …]` lines the Stop hook files). An informational line must ride existing context or be dropped, never become a
  prompt the user cannot cancel
- add a daemon or a server the local binary must babysit (§7)
- guess when the log is ambiguous
- police the developer's process — worktree, branch, or git flow. Fael carries the work
  between agents, never decides how it is done: an assignment or a claim informs. A claim is race-safe (one winner) but gates only the
  claim: `--force` takes it over, and no edit is ever blocked.

## 1. Parts

```
┌──────────────────────── fael (one binary) ────────────────────────┐
│                                                                    │
│   adapters            core                         log             │
│   ─────────           ────                         ───             │
│   CLI         ──┐     normalize·validate·append ▶ .fael/log/**    │
│   MCP (stdio) ──┼──▶  find · rank · budget ◀────  (jsonl, in git) │
│   hook        ──┘     decide (block / context)                     │
│    ├ claude                                                        │
│    ├ codex                                                         │
│    ├ opencode                                                      │
│    └ neutral  ← public protocol for any other agent                │
└────────────────────────────────────────────────────────────────────┘
```

| Part | Job | Knows about |
|---|---|---|
| **log** | stores rows — the only source of truth | nothing (plain files) |
| **core** | validates, appends, finds, ranks, decides — `add_row` · `close_row` · `query` · `Config::from_toml` | the row format · never a client, never git or cwd: the adapter passes `Stamp { by, branch, sha }` and the config text |
| **adapters** | turn each client's input/output into core calls | one client each · never the rules — paths go through `core.normalize`, never an adapter's own cleanup |

Adding a client touches only an adapter. Changing a rule touches only core. Changing the format is a spec change.
`fael sync` is adapter-side transport, not storage: it reads the journal through core and carries it in
`refs/fael/<repo-id>/<writer>` (or a future cloud transport) — the row format never changes in transit
([sync-format.md](sync-format.md)).

## 2. Storage

```
.fael/
  config.toml                  optional — every field has a default
  log/
    <writer>/
      2026-09.jsonl            rows written this month — append-only
      2026-09.close.jsonl      closes written this month — append-only
      compact.<ULID>.jsonl     immutable, from `fael compact`
    _import/<ULID>.jsonl       immutable, from `fael import`
  .lock                        not in git — serialises local writers
<git-common-dir>/fael/log/     the journal — same layout, shared by every
  <writer>/2026-09.jsonl       worktree of the clone; the commit point of `add`
```

- `<writer>` = `<git user.name slug>-<4 hex of sha256(email)>` — a writer is a **logical author, not a machine**: the same person on two machines shares a folder (their appends meet in git via union merge), and two people who share a name do not.
- One file per month: last month's file is never written again, so rotation needs no command.
- `.gitattributes` (tracked store only): `.fael/log/**/*.jsonl merge=union` — two branches that both appended keep both sides locally; readers remove the duplicates by `id`. GitHub ignores merge drivers, so PRs that append to the same month file still conflict there — the reason `local` is the default.

**Row** (one JSON object per line — full spec in [format.md](format.md)):

```json
{"v":1,"id":"01J8ZQ3K4M7N2P5R8T1V4X6Y9A","ts":"2026-09-25T10:00:00Z","by":"delamind-3f9a",
 "kind":"decision","text":"…","files":["src/a.rs"],"key":"auth:session"}
```

- `id` is a ULID — time-sortable, and it never collides across machines.
- `kind` is `decision`, `issue` or `note`, plus any kinds the repo declares in `config.toml`.
- `files` is fael's **locality index**: stable references the row is about — it is what makes memory come to the agent, and why fael needs no tags, links or graph. At least one is enforced, in one of two forms:
  - **path** `src/auth/session.ts` — normalised to repo-relative
  - **anchor** `scheme:ref` (`doc:pricing`, `issue:#12`, `customer:acme`) — for memory with no file behind it. fael checks only the syntax; the ref is opaque (`/` in `doc:pricing/2026` is not a path) and there is no registry of schemes — what a scheme means belongs to the agent.
- `key` is optional — a row with only `files` is complete. When used it is a stable namespace with Redis-style names (`auth:session:timeout`) and is queried with glob patterns (`auth:*`).
- A close is its own row, written to the `.close.jsonl` file. It never edits the row it closes.

**Write safety:**
- A line is committed once its trailing `\n` is written.
- Local writers are serialised by `File::lock` on `.fael/.lock`.
- Files are rewritten only with tmp-then-rename, and only when no one writes to them any more.
- Durability comes from the journal, not the tree: `add` writes
  `<git-common-dir>/fael/log/` first (same line bytes), then `.fael/log` —
  unless `store = "local"` (the default), which skips the tree so PRs carry no
  log lines and gitignored or public repos
  carry no memory. A failed tree write is a warning, never a retry (the row is
  already durable; a retry would file it twice under a new id). Reads union
  both, tree wins on duplicate ids; journal-only rows tag `@<branch>`.
- An unset `store` is `local`, also where a `.fael/log` already sits in the
  tree: that log stays read (the union above) as frozen history and nothing
  appends to it, so no memory file is ever left to commit (decision
  01M3YQT4). Only an explicit `store = "tracked"` writes the tree. A repo whose rows live only in the journal keeps
  its alias cache there too (`<git-common-dir>/fael/cache`): the tree never grows a `.fael/`.
- Across clones durability still comes from git; rows are not fsynced one by one.
- Across clones and machines journals travel through `fael sync`: each writer's
  journal is pushed to `refs/fael/<repo-id>/<writer>` on a remote (any git URL
  in `git config fael.remote`, never committed) and every writer's ref is
  ingested back by id-union. The working tree and checked-out branches are
  never touched; the full contract is [sync-format.md](sync-format.md).

## 3. API

### Integration modes

The storage and core are identical; only how the agent reaches fael differs.

- **Enforcement** — coding agents with lifecycle hooks: `agent → hook → fael`. Session brief, memory attached to reads and edits, capture from the reply. The agent reads git and the code first and calls `find` only for what they cannot say (why, what was rejected, what is unfinished) — never as a start-of-task ritual.
- **Tool** — chat agents and MCP hosts without hooks: `agent → MCP → fael`. With no push, the agent calls `find` once at the start (the session brief) and `add` on its own for durable things (an explicit "remember", a rule, a stable preference, a correction) — never for chatter. That judgement is agent behaviour (skill / tool description), not a core rule. Under `store = "tracked"` tool mode has no commit step: rows reach git only when the host or the user commits `.fael/`.

A standard-compliant MCP host needs no adapter — `fael mcp` is the whole integration.

### CLI

| Command | What it does |
|---|---|
| `fael add <kind> "<text>" --files a,b [--key k] [--title t] [--to who] [--revisit date\|text] [--urgent\|--urgent-before id] [--supersedes id [--replace old --with new]] [--force] [--dry-run] [--json]` | append a row (`--title` = the ≤15-word headline lists show; `--revisit` = a date `kickoff` surfaces when due, or free text; self-heal first — a repeat on the same files or key supersedes the open row, `Supersedes <id>` in the text fills `--supersedes`, and the one key on these files is reused, each said in one info line; `--supersedes id --replace old --with new` re-files that row with one passage changed (text, files, key and title carry over; `old` must occur in the body exactly once, else rejected); `--dry-run` prints the verdict heal would act on and the row it would write, rejects what the add would, and writes nothing; batch many at once — `rows: [...]` over MCP, `fael add --json -` with a JSON array on stdin over CLI, a bad row reports alone while the rest save) |
| `fael close (<id> \| --key <key>) "<why>"` | append a close row; a row that supersedes others closes the whole chain it names (oldest first), and closing an old version is allowed once its newest one is already closed. `--key` closes the one open row on that key (MCP `close` takes `key` too): none → `no open row on <key>`; several → lists each with its ready `fael close <id>` and closes nothing, fael never picks; not combinable with an id. The edit push also names up to 2 open issues it just showed, each with a ready `fael close <short id> "<why>"` |
| `fael bump <id> [--to who] [--revisit date\|text] [--urgent\|--urgent-before id\|--not-urgent]` | move an open row under its own id: same text/files, new `to`/`urgent`/`revisit`, file hashes restamped — one bump event (`docs/format.md` §Bump) |
| `fael claim <id> [--force]` | an open issue held by this branch: a bump stamping `held`, so `find` shows `(held @<branch>)`. The check and the write are one step under `.claim.lock` in the clone's journal: of two agents racing for one issue (any worktree of the clone) the first wins and the second is told who holds it. It gates the claim, never the work: `--force` takes an issue over, a hold whose branch no longer exists in the clone is taken over with a warning, no edit is ever blocked. Across machines a hold is only as fresh as the last `fael sync`. The holder's session start adds one line when an open issue it holds sits on a branch gone from the clone (merged and deleted, or dropped) — the close command, never a guess that it merged; a squash-merged branch kept locally is not seen |
| `fael next` | claim the best free issue and print it: open, not waiting, routed to nobody or to this reader, not held by a live branch; ranked like every list (yours, urgent, newest) |
| `fael find [text\|id] [--files …] [--key glob] [--kind …] [--since …] [--by writer] [--to who] [--revisit[=text]] [--all] [--branches] [--full] [--limit N] [--offset M] [--groups] [--text query]` | query; closed and superseded rows are hidden unless `--all`; lists show titles, `<id>`/`--full` show bodies and, for a closed row, why it closed (`closed: <text> (<sha>)`); an id-shaped query is never a text search — it rejects when no row owns it, naming the rows that only mention it; `--text` forces a text search even for an id-shaped query; text is every whitespace-split word, any order (§4 Ranking), and a blank one narrows nothing; `--branches` also reads branches not yet merged into HEAD, tagging their rows `@<branch>` without a checkout — only rows committed to their `.fael/log` that the union read lacks, none under `store = "local"` or a gitignored log, so when it adds none it says why (stderr on the CLI, a trailing line over MCP); a cut list prints the exact next call (`--offset M`; under `--full` it adds `--limit <rest>` for the rest in one call); an explicit `--limit N` is not cut by `budget.find_tokens`; `--kind issue` lists the issues ready to work first, those waiting on a revisit (free text or a date ahead, shown `(waiting: …)`) last; `--groups` prints every match, unpaged, grouped by shared files (union-find; `*.md` and anchors never link) — what to fix in one PR |
| `fael keys [glob]` | list keys, with a count and last use for each — to reuse a key that already exists |
| `fael mv <old> <new>` | record a move git can't see — an anchor, an uncommitted rewrite, or one file split into several (one old path may point at many new ones). Adds matches only, never hides a row |
| `fael restore [<id>] [--edge id]` | revert a supersede edge with an event row — the row keeps its id and opens again; `--edge` names the superseding row when several edges still hide it; an already-open row or an already-reverted edge is info, never an error |
| `fael purge <id>` | permanently remove a leaked test row or a mistake: the row and its close and bump events go from every month file, tree and journal; refused when another row supersedes or restores it, when the id names a close event, or when it lives in an immutable compact file; the id is kept as a tombstone (`purged.txt` in the writer's ref) so sync never carries the row back; copies already in a teammate's journal stay until purged there |
| `fael migrate local` | move a tracked repo to `store = "local"`: fold `.fael/log` into this clone's journal — the tree copy wins on every id it holds (a row edited in the tree replaces the journal's stale original in place), rows only the tree holds are copied — then set `store = "local"` in `.fael/config.toml`; idempotent; each clone runs it before the tree log is removed, since the fold reads the working tree |
| `fael kickoff [anchor] [--branches] [--full] [--limit N] [--offset M]` | the session brief: urgent first, then issues, decisions, notes by freshness (newer of the row and its files' last change); rows whose files are all gone are left out; an explicit `--limit N` is not cut by `budget.kickoff_tokens` |
| `fael hook <event> [--client c]` | hook entry point (see below) |
| `fael mcp [--pin]` | MCP server on stdio; `--pin` keeps every call on this cwd (for HTTP exposure) |
| `fael install [--client c] [--dry-run] [--replace-fapony]` | detect installed clients and wire MCP, hooks and skill into each one; `--replace-fapony` takes out fapony's Stop/session-start hooks and MCP (opt-in: they are user scope and still serve repos without `.fael/`) |
| `fael upgrade [--client c] [--dry-run] [--yes] [--replace-fapony]` | `install` that looks first: lists what is out of date, counts it, asks `[y/N]` before writing (`--yes` or no terminal skips the question; `update` is an alias) |
| `fael compact [--writer id] [--before yyyy-mm] [--prune]` | maintenance: fold old rows into per-writer summaries |
| `fael import <path> [--map old/=new/]` | maintenance: import a fapony log |
| `fael sync [--remote url]` | push this writer's journal to `refs/fael/<repo-id>/<writer>` on the remote and ingest every writer's ref back (`--remote` wins, else `git config fael.remote`); a `--remote` that synced becomes `fael.remote` when none is set; a run against `fael.remote` leaves its outcome in the journal dir (`synced` watermark, `sync-error`) for the late line |
| `fael doctor [--fix] [--fat]` | find and repair damaged logs — `--fix` moves bad lines to quarantine (never deletes them) and closes the confirmed `[Shipped]` notes — only status-shaped ones (the note's title or first line says PR opened/rebased, chunk N done, what shipped, not yet committed), never one with a `revisit` or a `*:handoff` key (those wait past the merge); a note on a landed branch that does not read as a status (a rule, a fact, `half done`) is listed as `[Shipped kept]` and left open; `--json` prints each problem's full row ids for a cleanup pass; prose in open rows, close reasons and every `*.md` is checked for dead id citations (`[Phantom]`); `[Superseded]` reports a legacy chain hidden by a supersede marker whose newest version is already closed (`fael close <id>` on each repairs it); `[Drifted]` lists open rows whose files took 10+ commits since they were written, for a check against the code; `[NoVerdict]` lists open rows that name a real file over the 1 MiB edit-push cap (`fael add` stamps up to 16 MiB, push compares up to 1 MiB), so push never says whether those files changed and the generic hint stands — one `metadata()` per file, a note only, never `--fix` |
| `fael stats [--json] [--rows] [--day] [--misses] [--since d]` | how many bytes and tokens fael has put into agents' context (`--day` = today's panels per repo and summed; `--since` = only usage from then on; `--misses` = the newest empty text searches, machine-local — the plain page ends with a count line once any exist) |
| `fael tune [--json] [--since d]` | read-only replay of candidate push rules (`touch@1`, `touch-yield@1`) over the search pushes in `usage.jsonl`, beside `baseline@1`: exposure, outcomes kept, rows the agent pulled itself after the cut, missed pushes (x/n with 95% interval), coverage, strata — names no winner, writes nothing; lists each repo's push-gate stage, and once an arm exists reports the holdout verdict — see `docs/learn-loop.md` |
| `fael report [--out f] [--open] [--since d]` | one offline HTML page for a lead: what memory reached the agents, what is noise, did fael add friction — numbers from `fael stats --json` |

`--files` in `find` matches a row's `files[]` only — exactly, as a directory (a zone), or by glob; an anchor's ref
never matches as a directory. It does not fall back to searching text (fapony did); text is `find <text>`.
Queries expand through rename aliases first, so a row filed under a path that was renamed since (`git log -M`,
cached in `.fael/cache/aliases.json`, plus `fael mv` rows) still matches at the new path.
Ids are accepted as a unique prefix and printed at the shortest length that stays unique (≥ 8).

`.fael/config.toml` — every field is optional:

```toml
kinds = ["risk"]              # extra kinds on top of decision/issue/note
key_domains = ["auth", "db"]  # first key segment; outside the list = warning, never a reject
resolve = true                # follow renames (git log -M + fael mv rows); false = match files[] literally
store = "local"               # journal only (default, tree log or not); "tracked" also writes .fael/log to commit
# push_policy unset = auto: the repo's own stages (shadow → canary → ramp, rolled back on evidence; docs/learn-loop.md). A set value is a pin that always wins:
push_policy = "touch@1"       # "touch@1" = the validation experiment: search pushes of candidate sessions lose the rows it cuts (usage `cut: gate`); "baseline@1" opts out
push_holdout = 20             # percent of sessions (by hash of the session id) that keep baseline@1 while touch@1 is pinned
[budget]
kickoff_tokens = 800          # kickoff, and find with no filter (unless --limit is given)
find_tokens = 800
push_tokens = 800             # read/edit hook push: rows first, then the edit hint and a stashed notice in what is left
push_rows = 5               # at most this many rows per push (0 = token budget only)
session_decisions = 0         # session-start lists this many freshest open decisions above the count line
[warn]
row_tokens = 400
row_chars = 1200              # a single-topic-looking row can still run long
[anchor]
prefixes = ["PLAN-"]          # <PREFIX><name>.md widens kickoff to <prefix>:<name> (e.g. HANDOFF-); PLAN- is the plain default, not fapony knowledge
[limit]
row_bytes = 10240             # hard cap, never above 10 KiB
[sync]
auto = true                   # session start and Stop run `fael sync` once per session and newest row when fael.remote is set; false = manual only
[notify]
user = true                   # one line per beat for the user only (Claude systemMessage, OpenCode toast); false = off
[hint]
stop = []                     # prompt words the key hint never matches, case-insensitive; copy the word from the hint's `via "<word>"` when it is a generic head (e.g. ["workspace"]). A key typed whole still hints
[lang]
marker = ["english", "thai"]  # Stop-hook phrase packs (default); [] switches the bug rule off
rows = ["english"]            # accepted row-writing languages; anything else warns once, never rejects ([] switches the check off)
```

### MCP (3 tools on stdio — each schema is paid for in every session, so the list stays short)

| Tool | Input | Notes |
|---|---|---|
| `find` | `id?` `ids[]?` `files[]` `text` `key` `kind` `since` `to` `by?` `all?` `revisit?` `branches?` `limit` `offset` | read-only, `ids` pulls several bodies in one call under the same `find_tokens` budget as `full` (the first body always shows, the cut line names the ids left; a bad id reports alone, the rest print, the call is an error); a first page of one or two rows with no `limit` shows bodies like `full` when they fit that budget, else titles; `--json` is the machine shape and is never cut — over the budget with no `--limit` it prints one `fael: N rows, ~T tokens of JSON …` line on stderr instead; an empty find says why — each word, `files` and filter counted on its own — and an empty *text* search is also kept in the machine-local `find-misses.jsonl` (`fael stats --misses`), the recorded miss decision 01M3ST4V waits for; cut to `budget.find_tokens` unless `limit` is given — a named `limit` wins over the budget. No filter = the session brief (what `kickoff` shows) — so there is no `kickoff` tool. `branches: true` also reads unmerged branches (rows tagged `@<branch>`). A cut list prints `next: offset=N` — repeat the call with it |
| `add` | `kind` `text` `files[]` (required, non-empty) `key?` `to?` `title?` `revisit?` `urgent?` `urgent_before?` `supersedes?` `force?` `rows[]?` | a bad value is rejected with an error message that says how to fix the call. Its description tells the agent to reuse an anchor `find` already showed rather than invent a new one. `rows` batches many rows in one call — a bad row reports alone while the rest save |
| `close` | `id` or `ids[]` `text` | `ids` closes many with one reason — a bad id reports alone, the rest close |

`bump` is CLI-only (`fael bump`): re-routing a row is rare and mostly a human call, so its schema is not paid in every session. The server still answers a `bump` call from a client that sends one.

### Hook protocol

Each client speaks its own hook format. The binary contains the adapters for the supported clients; everyone else uses the neutral format.

```
fael hook <stop|session-start|read|edit|search|prompt> [--client claude|codex]   < stdin  > stdout

neutral Event  {"cwd","session","client","files":[…],"text","reply","agent","source","tool","tool_input","tool_response"}
neutral Reply  {"block":false,"context"?:str,"notice"?:str}
```

**The user channel** (`notice`, PLAN-fael-visible-secretary): fael does its work in the agent's context, so the user never saw it. Each hook can now carry one line for the user only. Claude Code gets it as `systemMessage`, which is shown to the user and never added to the model's context. OpenCode shows it as a toast. Codex and the bare neutral protocol have no such channel, so they stay silent. There are three beats. Session start names the open issues the brief handed over in full. A push names the first decision or issue it reminded the agent of, at most once per file per session. Stop gives the turn's receipt (`filed · reminded · retired · closed`), and every count names up to two ids or keys that `fael find` takes back. A row that has not reached its destination adds one late line (PLAN-fael-local-first): under `store = "tracked"`, on a turn that filed or closed, the count of uncommitted `.fael/log` files; under `local`, once per session, this writer's rows newer than the last good sync after a sync failed (no count when there is no watermark yet), or, with `store` unset, a `.fael/log` in git and no `fael.remote`, that new rows stay in this clone (the repo used to share them by commit). A pending row is only late; a failed sync, or sharing that stopped, must not stay silent. `doctor` shows the same line as `[Late]`. When nothing happened there is no line: fael never says "nothing new", and never claims anything was prevented or saved. The receipt reads a per-session tally in the state dir (`<seen key>.tally`). `add`, `close`, capture and the push append to it, and each Stop reads the lines since its last marker. The tally is never part of a row, and `additionalContext` is the same byte for byte with the channel on or off. `[notify] user = false` turns the channel off.

OpenCode has no Stop hook and runs plugins in-process: `fael install` writes a JS plugin that speaks the neutral format (stop runs on `session.idle` and never prompts back). How to wire any other agent: [integrate.md](integrate.md).

The hook always exits 0. If fael hits an internal error it replies with an empty Reply, because a memory tool must never break the agent's tool call.

## 4. Data flow

**Write** — an agent records something:
```
agent ─(MCP add | CLI add | hook)─▶ core.normalize ─▶ core.validate ─✗─▶ error that says how to fix the call
                                                          │✓
                                                          ▼
                           journal append ─▶ tree append (skipped when store = local)
                  (<git-common-dir>/fael/log/…)      (.fael/log/<writer>/<month>.jsonl)
```

The write path self-heals before it validates (`fael/src/selfheal.rs`): `core.validate` sees one row and no log, so filling an absent `--supersedes` — from `Supersedes <id>` in the text, from the same kind + key of the caller's own row, or from a repeat on the same files when neither note has a key — and adopting the one key the row's files already carry happen here, where the log is readable. An `issue` is a finding, not a topic: a key may hold several, so a key match supersedes an issue only when it is the same finding re-filed (same words, a shared file), and a distinct issue sharing the key is kept and named. Shared files prove two notes related, not that one replaces the other: a key on either side, or several overlapping notes, files the row and names what stays open (`kept all`) — a silent hide costs the next reader a todo, a kept note costs one `close`. Each choice is reported in one info line, which is not an ask — except a cross-key act (the superseded row's key differs from the new row's, reachable only by naming the row in the text): under `[selfheal] cross_key = "warn"` (the default) it prints one `warning:` line instead, which counts as an ask; `"info"` keeps the info line, `"off"` acts silently. Every act stamps `decision_source` (`explicit:text`, `identity:key`, `heuristic:files`, each with `:cross-key` when the key moved, `caller:flag` for a resolving flag) so restore can trace an edge back to its cause; older rows read as `unknown`. Several candidates file the row and name what was kept: fael never picks and never rejects for it. Fael doesn't try to understand everything. It makes only the decisions it can justify, exposes the evidence when it can't, and makes every automatic decision reversible.

**Push** — the agent reads a file, and the memory for that file comes with it:
```
client ─(read event)─▶ adapter.parse ─▶ core.find(files) ─▶ rank ─▶ cut to token budget ─▶ adapter.render ─▶ context attached to the read
```
Ranking: an exact file match beats the same directory, which beats the same key. Open `issue` and `decision` rows go first, then newer before older by `id`. Text matching splits the query on whitespace: every word must be a case-insensitive substring of the row's text or title, in any order — no stemming, fuzzy match or hit-count ranking.
Ranking is **deterministic**: the same log, query and budget give the same output on any machine and any day — recency comes from `id` order, never from the clock, and ties break by `id`. No fuzzy, BM25 or semantic ranking.

Rows are then bucketed by the session **Focus** — the start branch and the keys of the open rows filed on it, written once at session start to
`~/.local/state/fael/sessions/<session+worktree>.focus.json`: **Now** (an open issue on the queried file, an urgent row, a row on the session branch, or sharing one of those keys) always renders, **File** (the queried file) fills the row cap next — unless more than `PUSH_HUB_ROWS` (8) File rows compete (a hub: a spec, a plan, PRODUCT.md), where freshness alone would pick off-topic rows, so only `PUSH_HUB_PEEK` (3) render, rows with a waiting `--revisit` first, once per file per session — **Background** (same directory, open issues included) never renders — the push header counts every hidden row (`(shown of total)`). The `… +N more — fael find …` count lines under the rows were dropped (PLAN-fael-say-gate chunk 4): 7 of 336 were followed by the call they printed. A shared-key row (the key of an exact hit, on another file) joins only when that key is in the Focus, where it is Now; otherwise it is neither pushed nor counted (a broad key spanning plans would read as an off-task push). The push reads that file back and rebuilds it only when `<gitdir>/HEAD` names another branch (another session switched the worktree): no git spawn, one small read. No session, no file or an unparsable file is `Focus::default()` — the ranking above, capped at `budget.push_rows`.

When rows ride along, the push ends its row block with one line — `memory: ~<used>/<budget> tokens · <n> rows` — the `est_tokens` of the rendered row lines against the push budget. The session-start brief carries no such line: its rows are the reader's own work, and the brief rides every session. It is an estimate (hence the `~`), never called savings; the real bill stays in `fael stats`. No rows, no line, and it rides the context already being injected — no extra turn, no spawn.

A row is said once per **context window**, not once per session: the pushed ids are kept per session, worktree and `agent` (`<state>/sessions/<key>.seen`), keyed by the session id (a transcript path counts as its file stem, so `add` and `find` — which only know `$CLAUDE_CODE_SESSION_ID` — mark the same list), and read-filter-append runs under a file lock so parallel reads push a row once. A sub-agent starts with an empty context, so it has its own list — what its parent was told says nothing about what it knows (Claude Code sends `agent_id` on tool events inside a sub-agent; an OpenCode sub-agent is a child session with its own id). A session-start with `source: "compact"` drops the thread's list, because compaction removed the pushed rows from context.

**Capture** — the agent ends its turn. The Stop hook reads the last assistant message (`reply`, else the tail of a Claude transcript) and files every line of the form `fael decision|issue|note: <text> [files: a,b]` at column 0, outside a code fence, through the same validation as `fael add` (secrets, ids, size, paths). `[files: …]` is required and never inferred from the session's edits. A line that cannot be filed is dropped and becomes one hint on the next push — never a turn. The same lines seen twice in one session file once. `block` is always `false`: fael never stops a turn. A sub-agent's stop (`agent` set — Claude Code `SubagentStop`) files its own `reply` the same way and does nothing else: no bug rule, no sync, no transcript fallback (the transcript is the parent's).

The edit hook appends `{"path","at"}` to `~/.local/state/fael/sessions/<session+worktree>.jsonl` — per-machine runtime state, never in `.fael/`; `fael add` without `--files` files the session's edits since its newest row. The `session` string sent with `edit` must equal the one sent with `stop`.

**Bug line** — a bug or risk announcement in the turn's text (the transcript tail after the latest user message) with no issue row since the words stashes one line for the next push, shown once; never a turn. The phrases come from the `[lang] marker` packs (`english` + `thai` by default, `marker = []` switches the rule off), never from hardcoded lists. The Stop-block mode (`[capture] block = true`) was removed 2026-10-03: 55 blocks, 12 followed by a row; an old config key is ignored.

Session start, and every stop, start one detached `fael sync` per session and newest row when `fael.remote` is set (`[sync] auto`): fail-quiet, never awaited, the hook still exits 0. Session start goes first, so teammates' rows reach the session's reads (not its kickoff context, already built) and a Stop with no new row starts nothing.

**Session start:**
```
client ─(session-start)─▶ write focus.json (start branch + the keys of the rows filed on it)
                        ─▶ open issues to you, urgent unassigned, or tied to this branch in full · due revisits in full · N freshest open decisions (opt-in) · count line for the rest ─▶ context
```
An issue is tied to this branch when it was filed on the start branch, carries a Focus key, or names an anchor that a row filed on the start branch names too (`plan:<name>`, or the `PLAN-<name>.md` that names it). A count line is easy to skim past, and that is how an agent missed the one open issue on its own plan. The anchors come only from what the branch's own rows say, so nothing about the session's plan is guessed.
fael never infers which plan a session is in. `plan:<name>` anchors and `plan:<name>:handoff` / `plan:<name>:chunk-<n>` keys are a fapony convention: fael stores and matches them like any other anchor or key, and fapony's kickoff asks for them itself (`fael find --key 'plan:<name>:*'`, `fael kickoff PLAN-<name>.md`). Which filename prefixes widen kickoff that way is repo config (`[anchor] prefixes`, default `PLAN-`) — a plain common name, not fapony knowledge; the key scheme fapony seeds stays fapony's. A session's intent is not a fact the shared log can answer — any rule that picks one plan from it is a guess, and a wrong guess pushes another task's rows into Now.

**Across branches** (one branch per person or per agent):
```
git for-each-ref ─▶ git cat-file --batch  .fael/log/** on each ref ─▶ dedupe by id ─▶ find / kickoff
```
Reading needs no checkout. Rows are written only to your own branch and reach `main` through the normal pull request.
Rows that only exist on another branch render with `@<branch>`; once merged they list once, untagged
(HEAD wins on duplicate ids).

## 5. Tokens

Tokens are the unit of value, and they are spent when reading, not when storing. So:

- Storage is JSON, so any tool can parse it and it merges cleanly in git.
- What an agent is shown (kickoff, push, `find`) is one markdown line per row: `- [id] kind #key text → files`. On real rows this adds 18.5% on top of the text, against 42.6% for raw JSON and 17.7% for TOON. The text is most of the size, so the savings come from choosing fewer rows, not from the format. One cut on top: a file push drops the file the agent just opened from each row's list (`→ also: <others> +N` past two), 14% fewer bytes over 25 file reads in one repo (2026-10-03, bytes not tokens). A pushed row that is not about the opened file says how it came, `- [id] (same dir) …` or `(same key) …`, and a push that cut rows names the cut in its header, `fael mem for <files> (shown of total):`.
- Every output is cut to a token budget (configurable per repo). The estimate is computed at read time and never stored, because every model's tokenizer counts differently. `est_tokens` stays the anchor unit (ASCII ≈ 4 bytes/token, non-ASCII ≈ 1 char/token) — a ruler, not a scale; convert with the frozen exchange table below — no per-model config, no formula tuning.

  | model | EN (× est) | TH (× est) |
  |---|---|---|
  | Claude 5 (Opus 5.5 / Sonnet 5, same tokenizer) | × 1.56 | × 1.05 |
  | Claude Haiku 4.5 | × 1.16 | × 1.01 |
  | o200k (tiktoken, offline) | × ~1.06 (est tracks within ±20%) | mixed rows track; pure-Thai est is the upper bound (~2.4× over o200k) |

  Measured 2026-09-28 via `count_tokens` over every row in the log (271 EN + 2 TH-mixed + 10 pure-Thai plan lines as proxies, framing overhead subtracted); Sonnet 5 and Opus 5.5 returned identical counts. Frozen 2026-09-28, not maintained — newer numbers come free from `real_tokens`, never from re-running this table.
- Every injection is recorded per machine (`~/.local/state/fael/usage.jsonl`, never in git), so `fael stats` shows fael's real cost in context. This is **local telemetry, not memory**: it may be lost, is never read to answer a query, and a failure to write it never fails a command. `.fael/` is the only semantic state.
- A row longer than about 400 estimated tokens triggers a warning when it is written, because it is paid for every time it is pushed. The hard limit is 10 KiB per row.
- The similar-key warning matches by parent only at ≥ 3 segments (`a:b:c` vs `a:b:d`); 2-level keys (`a:b` vs `a:c`) rely on levenshtein ≤ 2. This is an intentional trade-off, not a gap: with 2-level keys the shared parent is the bare domain, so a parent match would warn every new topic under it (domain reuse is the `key_domains` check instead), and a mixed-depth pair (`a:b` vs `a:b:c`) stays silent unless it is a typo away.
- A re-file (`--supersedes`, or a self-heal supersede) hears only the shape warnings the old row did not already earn — no key, several topics, long text, long title — so a translation or wording pass is not a wall of warnings agents learn to skip.

## 6. Self-healing

Reading never fails: broken lines, leftover merge-conflict markers, duplicate ids, CRLF and BOM are all handled in memory. Writing seals a torn last line before it appends. `fael doctor` reports problems, and `--fix` repairs them with tmp-then-rename. Bad lines go to `.fael/quarantine/`, so no byte is ever deleted.

`doctor` reads prose as well as bytes: open rows, close reasons and every `*.md` under the repo (outside `.git`/`target`/`node_modules`) are checked for citations of ids with no row behind them — `[Phantom]`, the dead citation the next reader takes as confirmation. Fenced code blocks are skipped there: a ULID inside a fence is an example, never a citation. This doctor sees only this repo's log, so an id from another repo's log reads as dead: cite that row by its key (`fael find --key <key>` there), which also survives a supersede.
`[Drifted]` is the safety net for rows the code outgrew: an open row whose real files took 10+ commits since it was written (one `git log` spawn in `doctor`, never in core). It is a fact, never a verdict — the reader checks each row against the code: the code says it now → close it (`now in <file>`); wrong now → re-file with `--supersedes`; still true → `fael bump <id>` (same id, restamped), which restarts the count. The edit push asks for the same check while the agent has the code in front of it, so most rows retire there and `doctor` catches the rest. The ask names a row once per session (the same words on every later edit would be noise) and leaves out rows that session filed itself.

`fh` is the same fact without the git spawn: at write time `fael add` and a bare `fael bump` hash each real file it names (`sha1("blob <len>\0" + bytes)`, CRLF read as LF in text files, the first 12 hex of `git hash-object` in a repo that stores LF, for a file with no lone `\r` and no NUL past byte 8000 — fael applies one rule on both sides, so the difference never reads as a change) into the row's `fh` map, so a reader can compare the file on disk to the bytes the row was written against and know whether this file changed — no commit count, no timer (`docs/format.md` §Rows). A row with no `fh`, or whose file left the repo, is unknown, never changed: fael never guesses. `fael claim` and a bump that only re-routes (`--to`, `--urgent`, `--revisit`) carry the old map forward on purpose — claiming or re-prioritising an issue is not checking it, and a restamp there would read as one. The map is small (≤ 8 files, 16 MiB each) because the row talks about the files it touches, not every path in the tree. A real file the stamp leaves out (over 16 MiB, unreadable, past the 8th) is named once, as an info line in the `add` / bare-`bump` receipt (CLI and MCP; `claim` and a routing bump print nothing, they restamp nothing) — the row is still filed, and anchors, globs, directories and missing files stay silent.

Every row — open, closed or superseded — whose line trips `validate::secret` (the one check add, import and sync ingest share) reports as `[Secret] <id>`: an error, `fixable = false`, never touched by `--fix`. The text says rotate first, then `fael purge <id>` — purge does not undo git history or other clones — and names the label and location, never the token.
Open rows with a letter outside every accepted `[lang] rows` script report as one `[NotEnglish]` batch (info, never `--fix`ed) with the full ids a translate pass supersedes (`fael add --supersedes <id>`, batched over `fael add --json -`); `fael stats` counts the same rows with the same detector against the running repo's accepted scripts (`rows not in English` under the default, never `rows with Thai`).

## 7. Non-goals

A query language, a daemon, embeddings, and hand-written tags or links. Links come for free from shared `files`, shared `key` and `supersedes`.

The local tool never needs a daemon or a server. A hosted server (MCP over HTTP for web chat hosts) is a separate product built on `fael-core` and this same format — it is not part of this binary. Three rules bind it:
- **A repo, when there is one, is the truth** — the hosted side is a git client that commits rows into it; it is canonical storage only for workspaces with no repo. Syncing is a union of lines deduped by `id` (append-only + ULID), so there is nothing to resolve.
- **It calls the same core entry points as the CLI** — `Stamp.by` from the signed-in user, no branch/sha. Hook session state (`~/.local/state/fael`) is CLI-only: HTTP clients have no edit or stop events, so there is nothing to carry to a phone.
- **Export and import go through this public format without loss of semantic memory** — no data stays locked in the hosted side.
