# fael architecture

> **Status:** the log format and storage (§2, [format.md](format.md)) are implemented in `fael-core`, and so are
> `add` `close` `find` `keys` `kickoff` `mv` in the `fael` CLI (`find` and `kickoff` take
> `--branches` to read unmerged branches without a checkout), `fael mcp`
> (stdio, 3 tools), `fael hook <stop|session-start|read|edit>` (neutral + claude/codex adapters) with
> per-machine usage accounting (`fael stats`), `fael install` (Claude Code, Codex, OpenCode),
> and the maintenance commands `fael doctor [--fix]` · `fael compact` · `fael import` (SPEC §6, §11).
> This page is the contract the code is built against —
> when code and this page disagree, fix one of them in the same commit.

fael is a memory log for agents that lives **inside the repo**: every agent (Claude Code, Codex, OpenCode, a chat
host speaking MCP, …) and every person on the team reads and writes the same log, git carries it between machines, and there is no server.

Three ideas carry the whole design:

1. **The log is the truth; everything else is derived** — like Redis's append-only file or a Kafka topic.
2. **Agents are made to write, not asked** — hooks refuse to end a turn that committed work without a memory row.
3. **Memory comes to the agent** — when an agent reads a file, the rows about that file are attached to the read.

---

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
- `.gitattributes`: `.fael/log/**/*.jsonl merge=union` — two branches that both appended keep both sides; readers remove the duplicates by `id`.

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
  unless `store = "local"`, which skips the tree so gitignored or public repos
  carry no memory. A failed tree write is a warning, never a retry (the row is
  already durable; a retry would file it twice under a new id). Reads union
  both, tree wins on duplicate ids; journal-only rows tag `@<branch>`.
- Across clones durability still comes from git; rows are not fsynced one by one.

## 3. API

### Integration modes

The storage and core are identical; only how the agent reaches fael differs.

- **Enforcement** — coding agents with lifecycle hooks: `agent → hook → fael`. Session brief, memory attached to reads, stop enforcement.
- **Tool** — chat agents and MCP hosts without hooks: `agent → MCP → fael`. The agent calls `find` at the start and `add` on its own for durable things (an explicit "remember", a rule, a stable preference, a correction) — never for chatter. That judgement is agent behaviour (skill / tool description), not a core rule. Tool mode has no commit step: rows reach git only when the host or the user commits `.fael/`.

A standard-compliant MCP host needs no adapter — `fael mcp` is the whole integration.

### CLI

| Command | What it does |
|---|---|
| `fael add <kind> "<text>" --files a,b [--key k] [--title t] [--to who] [--revisit date\|text] [--urgent\|--urgent-before id] [--supersedes id] [--force] [--json]` | append a row (`--title` = the ≤15-word headline lists show; `--revisit` = a date `kickoff` surfaces when due, or free text; self-heal first — a repeat on the same files or key supersedes the open row, `Supersedes <id>` in the text fills `--supersedes`, and the one key on these files is reused, each said in one info line; batch many at once — `rows: [...]` over MCP, `fael add --json -` with a JSON array on stdin over CLI, a bad row reports alone while the rest save) |
| `fael close <id> "<why>"` | append a close row; a row that supersedes others closes the whole chain it names (oldest first), and closing an old version is allowed once its newest one is already closed |
| `fael bump <id> [--to who] [--revisit date\|text] [--urgent\|--urgent-before id\|--not-urgent]` | new version of an open row: same text/files, new `to`/`urgent`/`revisit`, superseding the old one |
| `fael find [text\|id] [--files …] [--key glob] [--kind …] [--since …] [--by writer] [--to who] [--revisit[=text]] [--all] [--branches] [--full] [--limit N] [--offset M] [--text query]` | query; closed and superseded rows are hidden unless `--all`; lists show titles, `<id>`/`--full` show bodies; an id-shaped query is never a text search — it rejects when no row owns it, naming the rows that only mention it; `--text` forces a literal text search; `--branches` also reads branches not yet merged into HEAD, tagging their rows `@<branch>` without a checkout; a cut list prints the exact next call (`--offset M`) |
| `fael keys [glob]` | list keys, with a count and last use for each — to reuse a key that already exists |
| `fael mv <old> <new>` | record a move git can't see — an anchor, an uncommitted rewrite, or one file split into several (one old path may point at many new ones). Adds matches only, never hides a row |
| `fael restore [<id>] [--edge id]` | revert a supersede edge with an event row — the row keeps its id and opens again; `--edge` names the superseding row when several edges still hide it; an already-open row or an already-reverted edge is info, never an error |
| `fael kickoff [anchor] [--branches] [--full] [--limit N] [--offset M]` | the session brief: urgent first, then issues, decisions, notes by freshness (newer of the row and its files' last change); rows whose files are all gone are left out |
| `fael hook <event> [--client c]` | hook entry point (see below) |
| `fael mcp` | MCP server on stdio |
| `fael install [--client c] [--dry-run] [--replace-fapony]` | detect installed clients and wire MCP, hooks and skill into each one; `--replace-fapony` takes out fapony's Stop/session-start hooks and MCP (opt-in: they are user scope and still serve repos without `.fael/`) |
| `fael upgrade [--client c] [--dry-run] [--yes] [--replace-fapony]` | `install` that looks first: lists what is out of date, counts it, asks `[y/N]` before writing (`--yes` or no terminal skips the question; `update` is an alias) |
| `fael compact [--writer id] [--before yyyy-mm] [--prune]` | maintenance: fold old rows into per-writer summaries |
| `fael import <path> [--map old/=new/]` | maintenance: import a fapony log |
| `fael doctor [--fix] [--fat]` | find and repair damaged logs — `--fix` moves bad lines to quarantine (never deletes them) and closes the confirmed `[Shipped]` notes; `--json` prints each problem's full row ids for a cleanup pass; prose in open rows, close reasons and every `*.md` is checked for dead id citations (`[Phantom]`); `[Superseded]` reports a legacy chain hidden by a supersede marker whose newest version is already closed (`fael close <id>` on each repairs it) |
| `fael stats [--json] [--rows]` | how many bytes and tokens fael has put into agents' context |

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
store = "tracked"             # or "local": journal only, no .fael/log writes (gitignored/public repos)
[budget]
kickoff_tokens = 800          # kickoff, and find with no filter
find_tokens = 800
push_tokens = 800             # read/edit hook push
push_rows = 5               # at most this many rows per push (0 = token budget only)
session_decisions = 0         # session-start lists this many freshest open decisions above the count line
[warn]
row_tokens = 400
row_chars = 1200              # a single-topic-looking row can still run long
[anchor]
prefixes = ["PLAN-"]          # <PREFIX><name>.md widens kickoff to <prefix>:<name> (e.g. HANDOFF-); PLAN- is the plain default, not fapony knowledge
[limit]
row_bytes = 10240             # hard cap, never above 10 KiB
[lang]
marker = ["english", "thai"]  # Stop-hook phrase packs (default); [] switches the bug rule off
rows = ["english"]            # accepted row-writing languages; anything else warns once, never rejects ([] switches the check off)
```

### MCP (3 tools on stdio — each schema is paid for in every session, so the list stays short)

| Tool | Input | Notes |
|---|---|---|
| `find` | `files[]` `text` `key` `kind` `since` `to` `by?` `all?` `revisit?` `branches?` `limit` `offset` | read-only, cut to `budget.find_tokens`. No filter = the session brief (what `kickoff` shows) — so there is no `kickoff` tool. `branches: true` also reads unmerged branches (rows tagged `@<branch>`). A cut list prints `next: offset=N` — repeat the call with it |
| `add` | `kind` `text` `files[]` (required, non-empty) `key?` `to?` `title?` `revisit?` `urgent?` `urgent_before?` `supersedes?` `force?` `rows[]?` | a bad value is rejected with an error message that says how to fix the call. Its description tells the agent to reuse an anchor `find` already showed rather than invent a new one. `rows` batches many rows in one call — a bad row reports alone while the rest save |
| `close` | `id` `text` | |

`bump` is CLI-only (`fael bump`): re-routing a row is rare and mostly a human call, so its schema is not paid in every session. The server still answers a `bump` call from a client that sends one.

### Hook protocol

Each client speaks its own hook format. The binary contains the adapters for the supported clients; everyone else uses the neutral format.

```
fael hook <stop|session-start|read|edit> [--client claude|codex]   < stdin  > stdout

neutral Event  {"cwd","session","client","files":[…],"stop_active","text"}
neutral Reply  {"block":bool,"reason"?:str,"context"?:str}
```

OpenCode has no Stop hook and runs plugins in-process: `fael install` writes a JS plugin that speaks the neutral format (a stop block becomes a prompt on `session.idle`). How to wire any other agent: [integrate.md](integrate.md).

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

The write path self-heals before it validates (`fael/src/selfheal.rs`): `core.validate` sees one row and no log, so filling an absent `--supersedes` — from `Supersedes <id>` in the text, from a repeat on the same files, or from the same kind + key of the caller's own row — and adopting the one key the row's files already carry happen here, where the log is readable. An `issue` is a finding, not a topic: a key may hold several, so a key match supersedes an issue only when it is the same finding re-filed (same words, a shared file), and a distinct issue sharing the key is kept and named. Each choice is reported in one info line, which is not an ask — except a cross-key act (the superseded row's key differs from the new row's): under `[selfheal] cross_key = "warn"` (the default) it prints one `warning:` line instead, which counts as an ask; `"info"` keeps the info line, `"off"` acts silently. Every act stamps `decision_source` (`explicit:text`, `identity:key`, `heuristic:files`, each with `:cross-key` when the key moved, `caller:flag` for a resolving flag) so restore can trace an edge back to its cause; older rows read as `unknown`. Several candidates file the row and name what was kept: fael never picks and never rejects for it. Fael doesn't try to understand everything. It makes only the decisions it can justify, exposes the evidence when it can't, and makes every automatic decision reversible.

**Push** — the agent reads a file, and the memory for that file comes with it:
```
client ─(read event)─▶ adapter.parse ─▶ core.find(files) ─▶ rank ─▶ cut to token budget ─▶ adapter.render ─▶ context attached to the read
```
Ranking: an exact file match beats the same directory, which beats the same key. Open `issue` and `decision` rows go first, then newer before older by `id`. Text matching is plain substring.
Ranking is **deterministic**: the same log, query and budget give the same output on any machine and any day — recency comes from `id` order, never from the clock, and ties break by `id`. No fuzzy, BM25 or semantic ranking.

Rows are then bucketed by the session **Focus** — the start branch and the keys of the open rows filed on it, written once at session start to
`~/.local/state/fael/sessions/<session+worktree>.focus.json`: **Now** (an open issue, an urgent row, a row on the session branch, or sharing one of those keys) always renders, **File** (the queried file) fills the row cap next, **Background** (same directory, shared key) never renders — each hidden class gets one count line naming the exact `fael find` call that reaches it. The push only reads that file: no git spawn, one small read. No session, no file or an unparsable file is `Focus::default()` — the ranking above, capped at `budget.push_rows`.

**Enforce** — the agent tries to end a turn:
```
client ─(edit event)─▶ append {"path","at"} to ~/.local/state/fael/sessions/<session+worktree>.jsonl

client ─(stop event)─▶ edits recorded this session?
                          │yes                                │no
                          ▼                                   ▼
                any edit after the session's        git commits since start
                newest row (or any, if none)?       and no row this session?
                          │                                   │
                   no ─▶ allow  yes ─┐            no ─▶ allow  yes ─┐
                                     ▼                              ▼
                        block once per last row — a markdown list of the files
                        (or commits) and the exact command, --files prefilled
```
Edits, not commits, are the primary signal: many agents are told never to commit, and a commit-only rule never fires for them. Git is only the fallback for edits the hook never saw (a shell `sed`, a heredoc). Measuring from the newest row, not the session start, keeps a row filed early from covering hours of work after it; a new row reopens one more block. The edit list is per-machine runtime state, never in `.fael/`. The `session` string sent with `edit` must equal the one sent with `stop`.
A bug announcement blocks only without an issue row since the words — and the phrases that count as one come from the `[lang] marker` packs (`english` + `thai` by default, `marker = []` switches the rule off), never from hardcoded lists.

**Session start:**
```
client ─(session-start)─▶ write focus.json (start branch + the keys of the rows filed on it)
                        ─▶ open issues to you in full · due revisits in full · N freshest open decisions (opt-in) · count line for the rest ─▶ context
```
fael never infers which plan a session is in. `plan:<name>` anchors and `plan:<name>:chunk-<n>` keys are a fapony convention: fael stores and matches them like any other anchor or key, and fapony's kickoff asks for them itself (`fael find --key 'plan:<name>:*'`, `fael kickoff PLAN-<name>.md`). Which filename prefixes widen kickoff that way is repo config (`[anchor] prefixes`, default `PLAN-`) — a plain common name, not fapony knowledge; the key scheme fapony seeds stays fapony's. A session's intent is not a fact the shared log can answer — any rule that picks one plan from it is a guess, and a wrong guess pushes another task's rows into Now.

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
- What an agent is shown (kickoff, push, `find`) is one markdown line per row: `- [id] kind #key text → files`. On real rows this adds 18.5% on top of the text, against 42.6% for raw JSON and 17.7% for TOON. The text is most of the size, so the savings come from choosing fewer rows, not from the format.
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

## 6. Self-healing

Reading never fails: broken lines, leftover merge-conflict markers, duplicate ids, CRLF and BOM are all handled in memory. Writing seals a torn last line before it appends. `fael doctor` reports problems, and `--fix` repairs them with tmp-then-rename. Bad lines go to `.fael/quarantine/`, so no byte is ever deleted.

`doctor` reads prose as well as bytes: open rows, close reasons and every `*.md` under the repo (outside `.git`/`target`/`node_modules`) are checked for citations of ids with no row behind them — `[Phantom]`, the dead citation the next reader takes as confirmation. Fenced code blocks are skipped there: a ULID inside a fence is an example, never a citation.
Open rows with a letter outside every accepted `[lang] rows` script report as one `[NotEnglish]` batch (info, never `--fix`ed) with the full ids a translate pass supersedes (`fael add --supersedes <id>`, batched over `fael add --json -`); `fael stats` counts the same rows with the same detector against the running repo's accepted scripts (`rows not in English` under the default, never `rows with Thai`).

## 7. Non-goals

A query language, a daemon, embeddings, and hand-written tags or links. Links come for free from shared `files`, shared `key` and `supersedes`.

The local tool never needs a daemon or a server. A hosted server (MCP over HTTP for web chat hosts) is a separate product built on `fael-core` and this same format — it is not part of this binary. Three rules bind it:
- **A repo, when there is one, is the truth** — the hosted side is a git client that commits rows into it; it is canonical storage only for workspaces with no repo. Syncing is a union of lines deduped by `id` (append-only + ULID), so there is nothing to resolve.
- **It calls the same core entry points as the CLI** — `Stamp.by` from the signed-in user, no branch/sha. Hook session state (`~/.local/state/fael`) is CLI-only: HTTP clients have no edit or stop events, so there is nothing to carry to a phone.
- **Export and import go through this public format without loss of semantic memory** — no data stays locked in the hosted side.
