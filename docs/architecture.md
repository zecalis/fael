# fael architecture

> **Status:** the log format and storage (§2, [format.md](format.md)) are implemented in `fael-core`, and so are
> `add` `close` `find` `keys` `kickoff` `mv` in the `fael` CLI, `fael mcp`
> (stdio, 3 tools), `fael hook <stop|session-start|read|edit|search|prompt>` (neutral + claude/codex adapters) with
> per-machine usage accounting (`fael stats`, `fael report`), `fael install` (Claude Code, Codex, OpenCode),
> and the maintenance commands `fael doctor [--fix]` · `fael compact` · `fael import` · `fael sync` (SPEC §6, §11) ·
> `fael purge` (delete a leaked row for good — the one deliberate exception to the append-only log).
> This page is the contract the code is built against —
> when code and this page disagree, fix one of them in the same commit.

fael is the lifecycle ledger for agent work (§0). It lives **inside the clone** (`.git/fael/`): every agent (Claude Code, Codex, OpenCode, a chat
host speaking MCP, …) and every worktree on one machine reads and writes the same log, and there is no server. Across machines the log moves by `fael sync` over the git remote (`refs/fael/*`, no server, never on code branches; free for dev teams) or by hand with `fael import`. Committing `.fael/log` into git was dropped (scattered diffs, heavy merges with many agents); hosted sync and a non-dev web app are a separate paid service, and this binary adds no daemon (decision 01M5C2S7).

Three ideas carry the whole design:

1. **The log is the truth; everything else is derived** — like Redis's append-only file or a Kafka topic.
2. **Agents write in the same message, never a turn of their own** — `fael add` beside the next tool call, or `fael <kind>:` lines closing the reply; fael never refuses to end a turn.
3. **Memory comes to the agent where it acts** — at `fael kickoff`, in a plan brief, at session start, and when an agent edits a file (the rows about that file are attached to the edit). A read or a search says nothing: read and search pushes were ~74% of the tokens fael put into context (`fael stats`, 2026-09-25 → 2026-10-10).

---

## 0. North star and product invariant

**North star: fael is the lifecycle ledger for agent work.** Its primary user is the agent; the
goal is wide adoption by other teams. The ledger holds what passes between sessions: the
hand-off where work stopped, the decision and why, the issue still open, the claim, the note the
next session needs, what the user asked for. A row is pushed to the agent at the edit that
touches its files, closes with the commit that settled it, is superseded, or is flagged stale
when the code moves on. Git owns what changed; fael keeps only what git and the code cannot
say (why, what was rejected, what is unfinished). Memory remembers; fael closes (decision
01M5ABTD, key `product:mission`).

**Core and fael crew.** Core (public): add, find, close, bump, claim, doctor, stats, hooks, MCP —
boring-reliable, no daemon, never blocks an edit. **fael crew** is the owner's layer: plans cut
into chunks (`fael plan|chunk|board`, hidden from default help, plan `cli-tiers`), the board app,
and the lead and content skills; each chunk's brief, area (dev or marketing), size, model hint and
state live in the SQLite store, and an agent starts, waits on the owner and finishes a chunk
through `fael chunk start|wait|done`. Core never depends on crew; crew reads core through
`fael board --json`. The crew code is the `fael-crew` workspace crate (it depends on `fael-core`, never the
other way) and ships a `fael-crew` binary: `fael plan|chunk|board|sweep|intake|run` exec it with the
same argv (`FAEL_BIN` names `fael` for the way back), so the typed commands never change. `fael` itself
does not link `fael-crew` and hooks never run it; the two things crew needs from the ledger's own binary
(the held-rows text in `chunk start|done`, closing a linked row) are the hidden `fael held` and `fael close`. Public docs say "fael
crew", never bare "crew", and never link its repo.

Decision `product:mission` (01M5ABTD): plans and chunks move from
markdown files into fael's SQLite store (`<git-common-dir>/fael/crew.db`, English); the ledger stays
jsonl. Planner skills (fael-plan, fael-content, part of fael crew, decision 01M5AF2H) cut the chunks; the agent
judges size and model hint, fael stores them, the owner picks. A core change ships when it fixes a
bug, cuts noise, removes code, strengthens lifecycle or stats, or brings work in
from the team's other systems (intake, decision 01M4TJTW). The owner's
window is a separate SwiftUI app reading `fael board --json`: it watches, notifies, and starts
an agent only on the owner's click or from a queue of chunks the owner approved (dev chunks:
the owner, or the project's lead — the fael-lead skill, part of fael crew, decision 01M4SQ9B — after the owner approves its set, a lone ready one as a set of 1, decision 01M4Z88Y); the planner
agent applies merge / split / pair itself, and every git merge is the owner's. `fael stats` still shows what fael handed over and what
followed (capture → acted → outcome), never the counterfactual, so claims about value say
only that.

**The mechanism is the team's shared work ledger** (local first; `fael sync` moves it over git refs, `fael import` by hand): the context of the work, and the work handed
between sessions and agents — hand-offs, requirements, assigned issues (`--to`), claims,
come-backs (`--revisit`). It is not the agent's memory, not a personal notebook, not a generic
AI memory store, and not a process or git guard.

The unit of value is work context:

```
task (its `key`) → decision → evidence → outcome → closure
```

The agent discovers, judges and records; fael captures, carries and surfaces. Fael never infers
a bug or a lesson from test output, diffs, commits, CI or what prose means, never judges, and
never fixes for the agent. Fael resolves, reconciles, or self-heals **deterministic** context
before asking an agent to reason again; when it cannot decide, it exposes the evidence and never
guesses (§6). Rounds and tokens are a cost to keep low, never a promise.

A change must never:

- start a new agent turn, or re-prompt on idle. Capture rides the agent's own reply
  (`fael <kind>: … [files: …]` lines the Stop hook files). An informational line must ride existing context or be dropped, never become a
  prompt the user cannot cancel
- add a daemon or a server the local binary must babysit (§7)
- guess when the log is ambiguous
- police the developer's process — worktree, branch, or git flow. Fael carries the work
  between agents, never decides how it is done: an assignment or a claim informs. A claim is race-safe (one winner) but gates only the
  claim: `--force` takes it over, and no edit is ever blocked.

**Intake** (plan `fael-intake`, decisions 01M4V3AD, 01M4TNGS). Fael reaches the outside world
only through an app's adapter — never a user or a user's channel (Facebook, LINE, email, a
support desk): every channel goes channel → the app (which holds the customer's data, the draft
and the send button) → its adapter → fael, so a new channel is work in the app. The app commits
`.fael/sources.json` naming its adapter's `list`, `read`, `claim`, `draft` and `open`; `fael
intake` runs only `list` (strict JSONL fields, so no customer text comes in) and writes one
chunk per ticket round in one transaction: dedup, labels, the brief (a reopened round
prints the round before), and a ticket the source closed drops its open chunk or accepts its
review one (`by = intake`; a running one waits for the next run). The agent
prepares (a fix, a draft, a verdict); a person sends from the app. Fael never writes to the
source. The contract is the plan's `contract` section (`fael plan export fael-intake --section
contract`).

## 1. Parts

```
┌──────────────────────── fael (one binary) ────────────────────────┐
│                                                                    │
│   adapters            core                         log             │
│   ─────────           ────                         ───             │
│   CLI         ──┐     normalize·validate·append ▶ .fael/log/**    │
│   MCP (stdio) ──┼──▶  find · rank · budget ◀────  (jsonl, in git) │
│   hook        ──┘     decide (context, never blocks)               │
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

Where code belongs — three workspace crates, one direction (decision 01M5ABTD):

| Crate | Holds | Depends on |
|---|---|---|
| `fael-core` | log format, find, rank, rules, the read-only seam onto `crew.db` — no git, no cwd | nothing of ours |
| `fael` | CLI, MCP, hooks, install — the `fael` binary | `fael-core`; never `fael-crew` |
| `fael-crew` | plans, chunks, board, sweep, intake, `crew.db` schema and writes — the `fael-crew` binary | `fael-core`; reaches the ledger's own binary only through `FAEL_BIN` |

A change that writes plan or chunk state goes in `fael-crew`. A change a user of core needs with no
plan goes in `fael-core` or `fael`. Core never depends on crew, so `cargo tree -p fael -e normal` and
`cargo tree -p fael-core` show no `fael-crew`.
Free fael is one machine, many agents and worktrees; git sync (decision 01M5C2S7, restoring the transport
cut in #432) carries the same rows between clones over the git remote (`refs/fael/*`, no server), never
a second row format. `fael sync` is adapter-side transport, not storage: it reads the journal through
core and carries it in `refs/fael/<repo-id>/<writer>` ([sync-format.md](sync-format.md)).
`fael import <path>` stays as the by-hand route.

Where a work habit lives (decision 01M4X4DZ) — deleting crew must leave each chunk's quality
unchanged, only slower:

| Layer | Lives in | Holds |
|---|---|---|
| contract | `rules()` in `fael-crew/src/brief.rs`, printed by every `fael chunk start` | habits every chunk needs, any area or client: checkpoint, ask the owner, hand-off, a fresh-session check against Done before done |
| bar | the plan's Done and Constraints sections, printed in every brief | what "good" means for this plan: dev gates and criteria, marketing VOICE.md and source rules |
| judgment skills | fael-plan, fael-review, fael-content | how to cut, check, plan posts |
| orchestration | fael-lead (fael crew) | only running chunks in parallel without collisions |

No area-specific code in fael; a dev-only habit goes in a dev plan's Done or Constraints.

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
  `<git-common-dir>/fael/log/` only, so PRs carry no log lines and
  gitignored or public repos carry no memory; the tree log takes rows only
  without git (no journal). Reads union both, tree wins on duplicate ids;
  journal-only rows tag `@<branch>`.
- A `.fael/log` already in the tree stays read (the union above) as frozen
  history and nothing appends to it, so no memory file is ever left to commit
  (decision 01M4V3A02W). `store = "tracked"` (which wrote the tree too) is gone:
  a config that sets it is rejected with a pointer to `fael import <path>` and `fael sync`;
  `store = "local"` still parses and changes nothing. The journal keeps
  the alias cache too (`<git-common-dir>/fael/cache`): the tree never grows a `.fael/`.
- Across clones durability still comes from git; rows are not fsynced one by one.
- Across clones and machines journals travel through `fael sync`: each writer's
  journal is pushed to `refs/fael/<repo-id>/<writer>` on the remote named by
  `git config fael.remote` (never committed, never defaulted — not even to
  `origin`, decision 01M5D4KX) and every writer's ref is ingested back by
  id-union. The working tree and checked-out branches are never touched; the
  full contract is [sync-format.md](sync-format.md). `fael import <path>`
  copies another clone's log in by hand, by the same id-union.

### Row state (decision 01M5528X)

State is derived from the log and the disk on a pull, never stored: **live** (open), **held** (open, some stamped file changed but not all — the Held line asks once), **dormant**, **retired** (closed or superseded). A row is *dormant* when it has a non-empty `fh` map and **every** stamped file is on disk (through renames) and differs from its stamp. All, not any: a file that still matches, is gone, unreadable or over the cap keeps the row live — unknown is never changed, fael never guesses. A bare `fael bump` restamps `fh`, so "no bump since" needs no event scan and a bump revives. Issues and claimed rows (`held @branch`) are never dormant. Dormant rows are never pushed (edit push, held line, chunk start contracts, chunk done list), never in session start or kickoff, hidden from plain `find` (the empty hint names the count) and shown `(dormant)` under `find --all`. A retire row (close, bump, superseder) carries `extra.shown`: the captured fact of which lines (`held`, `done`) had shown its target in the writer session — never inferred, never a time window.

**Retired notes** (decision 01M5528X, D1): a note is *retired* when `fael chunk done` ended the chunk it was filed in, or git proves its branch merged. Derived on every pull, never written (the log is append-only): `hook::retired::ids` fills `Log.retired`, and also `Log.dormant`, so the same hiding applies — plain `find`, kickoff, session start, chunk briefs and the edit push skip it, `find --all` shows `(retired)`. Proofs, never guesses: (1) a done run — `crew.db` run ended on a chunk now in `review`/`done` — whose branch is the note's `row.branch()`, or whose session is the note's and the note's key is `plan:<that plan>:handoff|chunk-<n>`; a note written after the run ended is later work; (2) `find::view::prove`, the proof behind kickoff's `(merged)`, on the note's branch — one view of the refs per pull (a fixed number of git spawns however many branches the repo holds; facts that are a pure function of object ids — patch-ids, `merge-base HEAD <tip>` — persist as flat files under `<git-common-dir>/fael/cache/`, decision 01M5CRPS) — pull paths only, the push path spawns no git but reads the ids a pull proved from a cache file in the state dir (`retired/<repo>.txt`), valid only while HEAD's sha, its branch and the log's row count are the pull's, recomputed on a pull and never written into the log; when the branch ref is gone (no local, no `origin/`: GitHub auto-delete, local cleanup) the note's stamped `sha` (HEAD when it was filed) stands in for the tip if that commit is still in the object store, proved by patch-id only — the diff from `merge-base(HEAD, sha)` to `sha` must match a commit on `base..HEAD`, so a squash merge proves it and an ancestor test, which a stale tip would pass, does not. A note filed before the branch's last commit carries an older tip and stays live; a row without `sha`, or a gc'd commit, is unproven. A handoff-keyed note waits for its chunk, not the merge; a note with a revisit or a claim (`held @branch`) is never retired. Unproven = live.

**Lifecycle panel**: `fael stats` leads with this repo's rows by state per kind (decision, issue, note): live, held (open, some but not all stamped files changed — what the Held line asks about), dormant, retired (closed, superseded, or retired as above). The four add up to every row of the kind. `Stats.lifecycle` (`fael-core/src/stats/lifecycle.rs`, counts; `fael/src/hook/lifecycle.rs`, the disk half), read now from the cwd's repo, never from usage.

**Cap** (decision 01M5528X): `add decision` (CLI and MCP) rejects before the write when a real file in `--files` already holds 3 live decisions — open, not dormant, excluding the row this add supersedes. The reject lists those 3 with ready `--supersedes <id>` and `fael close <id>` commands; `--force` files it as is, like the other shape gates. Anchors are not capped.

## 3. API

### Integration modes

The storage and core are identical; only how the agent reaches fael differs.

- **Enforcement** — coding agents with lifecycle hooks: `agent → hook → fael`. Session brief, memory attached to reads and edits, a bug or fix said in the turn raised on the next push. The agent reads git and the code first and calls `find` only for what they cannot say (why, what was rejected, what is unfinished) — never as a start-of-task ritual.
- **Tool** — chat agents and MCP hosts without hooks: `agent → MCP → fael`. With no push, the agent calls `find` once at the start (the session brief) and `add` on its own for durable things (an explicit "remember", a rule, a stable preference, a correction) — never for chatter. That judgement is agent behaviour (skill / tool description), not a core rule.

A standard-compliant MCP host needs no adapter — `fael mcp` is the whole integration.

### CLI

`fael plan`, `chunk`, `board`, `sweep`, `intake` and `run` are served by the `fael-crew` binary:
`fael` execs it with the same argv and passes its exit code on. With no `fael-crew` next to `fael`
or on `PATH` they exit non-zero with the line `cargo install --path fael-crew`; every other command,
and every hook, works without it. The rows for those verbs below describe `fael-crew`.

| Command | What it does |
|---|---|
| `fael add <kind> "<text>" --files a,b [--key k] [--title t] [--to who] [--from user] [--revisit date\|text] [--urgent\|--urgent-before id] [--supersedes id [--replace old --with new, no <text>]] [--force] [--dry-run] [--json]` | append a row (`--title` = the ≤15-word headline lists show; `--revisit` = a date `kickoff` surfaces when due, or free text; self-heal first — a repeat on the same key supersedes the open row, a repeat on the same files files beside it and names what stays open, `Supersedes <id>` in the text fills `--supersedes`, and the one key on these files is reused, each said in one info line; `--supersedes id --replace old --with new` re-files that row with one passage changed (text, files, key and title carry over; `old` must occur in the body exactly once, else rejected); a topic list (`;`/`·`), a long text with no `--title`, or a plan key off the handoff convention (`plan:<name>:handoff`; `chunk-<n>` only for a chunk run in parallel; a note under it needs `--force`) is rejected before the write, naming every other fat reason too, so one re-run fixes them — `--force` files it as is (a re-file through `--supersedes` is judged only on what it adds); `--dry-run` prints the verdict heal would act on and the row it would write, rejects what the add would, and writes nothing; batch many at once — `rows: [...]` over MCP, `fael add --json -` with a JSON array on stdin over CLI, a bad row reports alone while the rest save) |
| `fael close (<id> \| --key <key>) "<why>"` | append a close row; a row that supersedes others closes the whole chain it names (oldest first), and closing an old version is allowed once its newest one is already closed. `--key` closes the one open row on that key (MCP `close` takes `key` too): none → `no open row on <key>`; several → lists each with its ready `fael close <id>` and closes nothing, fael never picks; not combinable with an id. The edit push also names up to 2 open issues it just showed, each with a ready `fael close <short id> "<why>"` |
| `fael bump <id> [--to who] [--revisit date\|text] [--urgent\|--urgent-before id\|--not-urgent]` | move an open row under its own id: same text/files, new `to`/`urgent`/`revisit`, file hashes restamped — one bump event (`docs/format.md` §Bump) |
| `fael claim <id> [--force]` | an open issue held by this branch: a bump stamping `held`, so `find` shows `(held @<branch>)`. The check and the write are one step under `.claim.lock` in the clone's journal: of two agents racing for one issue (any worktree of the clone) the first wins and the second is told who holds it. It gates the claim, never the work: `--force` takes an issue over, a hold whose branch no longer exists in the clone is taken over with a warning, no edit is ever blocked. Another clone never sees the hold. |
| `fael next` | claim the best free issue and print it: open, not waiting, routed to nobody or to this reader, not held by a live branch; ranked like every list (yours, urgent, newest) |
| `fael find [text\|id] [--files …] [--key glob] [--kind …] [--since …] [--by writer] [--to who] [--revisit[=text]] [--all] [--full] [--limit N] [--offset M] [--groups] [--text query] [--recall wide]` | query; `--key` is a glob, and a bare key also matches its `k:*` children; closed and superseded rows are hidden unless `--all`; lists show titles, `<id>`/`--full` show bodies and, for a closed row, why it closed (`closed: <text> (<sha>)`); a query that is exactly a file or anchor some row is filed on (`plan:<name>`, a path) is that `--files`, as `kickoff` reads it — a name no row is filed on stays text; an id-shaped query is never a text search — it rejects when no row owns it, naming the rows that only mention it; `--text` forces a text search even for an id-shaped query; text is every whitespace-split word, any order (§4 Ranking), and a blank one narrows nothing; a key-shaped text (`a:b`, `#k`, `key:k`, `a:*`) that matches nothing as words retries as `--key` before the partial-word guesses, and an empty answer names the `--key` to repeat with; a cut list prints the exact next call (`--offset M`; under `--full` it adds `--limit <rest>` for the rest in one call); an explicit `--limit N` is not cut by `budget.find_tokens`; `--kind issue` lists the issues ready to work first, those waiting on a revisit (free text or a date ahead, shown `(waiting: …)`) last; `--groups` prints every match, unpaged, grouped by shared files (union-find; `*.md` and anchors never link) — what to fix in one PR |
| `fael keys [glob]` | list keys, with a count and last use for each — to reuse a key that already exists |
| `fael mv <old> <new>` | record a move git can't see — an anchor, an uncommitted rewrite, or one file split into several (one old path may point at many new ones). Adds matches only, never hides a row |
| `fael plan new <name> "<title>" [--app A] [--area dev\|marketing] [--parent <plan>] \| fael plan export [<plan>] [--json \| --section <key>] \| fael plan export --since <event-id> \| fael plan apply <file\|-> [--dry-run] \| fael plan park\|unpark\|done <plan>` | a plan is sections plus chunks in `<git-common-dir>/fael/crew.db` (SQLite, WAL, shared by every worktree; crew owns the whole file and the name `fael.db` stays free for core; schema v8, an older db upgrades on open — the rename renamed `fael.db` to `crew.db` on first open and left a stub `fael.db` at `user_version` 1000000, so a fael from before the rename refuses it instead of starting an empty db (the stale `plans.db` stub from epoch v8 is removed when seen); `user_version` is the breaking epoch — only a rename, drop or retype bumps it, run alone, and the db is copied to a backup first; a new table or nullable column goes in the `TABLES`/`COLUMNS` lists of `fael-crew/src/schema.rs`, added on any open that lacks it, so builds with different additions share one db both ways and a higher epoch is refused) — no file; `section` (plan fael-v8, a `TABLES` addition) holds a plan's text as rows `(plan, key, title, body, seq)`, `key` unique per plan and stable (`1`, `§3`, a heading slug), and every insert, edit and drop writes a plan event `<key>: <title>`, so rev moves and the feed rebuilds it; a spec is sections of the plan it serves, never a separate doc; `new` writes a plan whose chunks live in the db with four skeleton sections (1 Goal, 2 Scope, 3 Done criteria, 4 Constraints) and refuses a taken name; `--parent <plan>` (or `"parent"` in the bill header, `""` clears it) nests the plan under an umbrella plan of the same app, one level — `plan.parent`, a `COLUMNS` addition holding the umbrella's name; a missing parent, the plan itself, a parent that is a child already and a plan that has children are refused (issue 01M51KW0); the board shows it, nothing else reads it; `export` writes a plan as markdown (uid, state, fields per chunk, then the plan's sections in order, each under its heading with its key and the day its newest event was written; `--section <key>` prints one) for review, backup and pasting into a chat; `export <plan> --json` writes it as one versioned bill (`"v": 1`: rev, header, `sections` `[{key, title, body}]`, a line per planned chunk with its `reads`) and `apply <file\|->` takes it back (decision fael-board:plan-bill) in one `BEGIN IMMEDIATE` on the `chunk apply` engine — a stale rev rejects it and writes nothing, sections are matched by key (a new key needs a title, `{key, drop: true}` drops one, a key left out is left alone — so a bill can carry one section), a line without uid adds a chunk, a line with uid edits it (a change clears its approval), `{uid, drop: true}` drops it, a line missing from the bill is left alone, any change to a running or finished chunk rejects the whole bill, `state` is read back and never set, an optional `ops` array carries merge/split/pair/after, every row shares one `event.batch`, `--dry-run` prints what it would write and rolls back; `export --since <event-id>` is the change feed backup and fael-sync read instead of crew.db (decision fael-board:sync-backup, `"v": 1`): `db_id`, `head` (the next `--since`), every `event` after the id and the current rows they touched, whole — plans, every section of those plans (one missing was dropped), chunks, the edges out of those chunks, their runs — `0` = everything, read-only; `park`/`unpark`/`done` set a plan's state (parked or done leaves the board; refused while one of its chunks runs); the md era (`PLAN-*.md` read by `import`, `next`, `cutover` and a one-time `migrate` into sections) ended with fael-v8, and epoch v8 dropped what carried it or nothing (`plan.truth`, `source`, `kind`, `rank`, `refs`, `repo`, `doc`, `spec`; `chunk.due`, `pin`, `client_hint` — back as a `COLUMNS` addition once a reader needed it, 01M4XM98; the `tr` and `contract` tables), cutting a `plan.doc` into sections first on a db that predates them; schema v6 (decision fael-board:master-detail): `event.plan` — the plan a row belongs to, backfilled on upgrade, written by every new row; `MAX(id)` of a plan is its rev (`plan export <plan> --json` gives it, `plan apply` checks it) · `event.batch` — one ulid on every row one `chunk apply` wrote · `meta` `db_id` — this crew.db's stable id, set once, kept when the repo moves |
| `fael chunk <verb> … [--plan p] [--label l] [--title t] [--brief b] [--size S\|M\|L] [--model m] [--client c] [--scope a/,b.rs] [--reads key,plan/key] [--after uid] [--run R] [--force] [--no-worktree] [--on owner\|data] [--until date] [--option A] [--pr N] [--out path]` | the chunk contract (SPEC-fael-board §1) over crew.db (`inbox` or a plan from `plan new`): `add "<title>"` prints the new uid (no `--brief` = `draft`; `--plan inbox` is the default and is created on first use); `add`/`edit --label l` names it (one word, not uid-shaped, unique in its plan) and every verb takes the label for the uid — a label in two plans asks for the uid, or `--plan p` on any verb but `add`/`move` picks it (a uid given with `--plan` must sit in that plan, else rejected); `show <uid\|label> [--json]` prints the board's chunk object plus the brief, owner answers and notes its next start prints (a running chunk: the ones its start printed); `start <uid>` claims the chunk and every chunk paired with it in one `BEGIN IMMEDIATE` — all or none, an `after` between two of them met by plan order — writes one run row each under R (`--run`, else a new id; a reused R is rejected) and prints the brief: R on the first line, the model the run uses (`--model`, else the approval's, else the hint — the board's `model` key carries the same value for the launcher's `opencode run --model`), the plan's Goal and Done sections plus each section the chunk names with `add`/`edit --reads <key>[,<plan>/<key>]` (`chunk.reads`, one per line; a missing one prints `section <ref> not found`), then an index of the plan's other sections (key, title, updated day) with `fael plan export <plan> --section <key>` — no `--reads` prints Goal/Scope/Done/Constraints — the owner's answers and the `note`s since the last start, the done/review hand-off of each chunk it is `after` with `files it touched:` when that chunk's done claimed a diff (`chunk.touched`; issue 01M4TPHMR), the chunk rules; the loser of a race prints nothing and is told who holds it, `--force` ends the holder's run and takes it; one start per worktree, unless `--no-worktree` (a client that writes a file and exits: the run holds no worktree, so no slot); `start --client c --model m` keep what ran in `run.client`/`run.model` (a flag left out takes the approval's, else the chunk's hint, issue 01M4XN3S); `add`/`edit --size S\|M\|L --model m --client c` keep the hints (free strings but size; `chunk.client_hint`, board `client_hint`, absent = none, is the client `approve` takes without `--client`, else claude, issue 01M4XM98); `wait --on owner\|data [--until d] [--option A …]` (`--option`, only with `--on owner`: the answers offered, `chunk.wait_options`, board `wait.options`; issue 01M4NWFS), `after <other>` and `done [--pr N \| --out path]` (`--pr` also takes `owner/repo#N` or the PR URL; the number is kept, an `owner/repo` other than this repo's origin is rejected, issue 01M4QFJ6V) answer only to the worktree of the latest run, or to `--run R` naming it, and end it — `done --pr N` lands in `done` (the owner's push pr is the ok), `done` without a PR in `review`, and on a branch other than the base the branch's `git diff --name-only origin/HEAD...<branch>` replaces the declared scope — `<branch>` is the run's recorded branch (`--run R`'s, else the latest run's; none, or the trunk itself — a run started there, then switched — = the cwd's, issue 01M4TPHMN), so a lead's done per lane from the wave branch claims that lane only (issues 01M4RRD1K, 01M4TFB5; a path scope refuses is left out, no diff keeps the declared one; of that diff only the paths changed since `run.base`, HEAD at the chunk's first start, count — a lane stacked on another's branch claims its own, issue 01M4TVDGG) — and first prints `declared scope: <paths>` and one `outside declared scope: <path>` or `outside declared scope: <path> (overlaps open chunk <plan>/<label>)` (another unfinished chunk's scope covers it, board's prefix rule; `.fael/shared` paths stay silent) per diff path not under it, nothing when all are inside — a fact, never a refusal, `--out` is copied to `<git-common-dir>/fael/out/<uid>/<R>/`; `done --review clean\|fixed\|failed` keeps the fresh review's outcome on the latest run (`run.review`, board `run.review`, absent = none; any chunk, unlike the ticket-only `--verdict`, issue 01M4XN3S); `answer` (waiting or review → open), `accept` (review or running → done), `drop`, `park`, `unpark` are the owner's; `edit` changes fields and clears an approval — except a `--scope` alone whose every old path stays under a new one (a widening keeps it, owner 2026-10-15) — and the state only to open a draft that gains a `--brief`; `note <uid> "<text>"` leaves a word for the next start from any worktree — state and approval stay, a done chunk is rejected; `move <uid> --plan <p>` puts a chunk at the end of another db plan of this repo in one transaction — only `plan` and `seq` change, uid, runs, notes, edges and approval stay — with a `moved from <old> to <p>` event on the chunk and one on the old plan so its rev moves; refused while it runs, into a parked or done plan, a plan not in this db, the same plan, or onto a label the target has; `ready` is one function in core (open, or a data wait past its date; every `after` met; plan not parked); the Stop hook fills `session` and `last_seen` on its worktree's live run; a start inside Claude Code with no `--run` (the agent's own, not a launcher's) binds `$CLAUDE_CODE_SESSION_ID`, so a run that `done` ends in the same turn keeps its session, and leaves `<state>/held/<session>` naming the crew.db, so that session's Stop hook stamps the run from any cwd (a `--no-worktree` run has no worktree to match) |
| `fael intake [<source>]` | each source's `list` from `.fael/sources.json` (`sh -c`, `cmd /C` on Windows, at the repo root); every line checked before one row is written (an unknown or missing field, a bad line, a nonzero exit: nothing written, exit 1); one `BEGIN IMMEDIATE` per run and a partial unique index on `chunk(ticket_source, ticket_ref, ticket_reopen)`: one chunk per ticket round in any plan — an open one with none is added to inbox (label `lower(ref)[-rN]`, none when an inbox chunk holds that label — another source's ref or one differing by case — printed `(no label: <uid> holds it)`, issue 01M4W0R3; size S, the brief template; a chunk without a ticket is never taken over, issue 01M4Z4WK), events by `intake`; `board --json` shows `chunk.ticket`, its `verdict` the latest run's `chunk done --verdict` (`run.verdict`, a ticket chunk only) |
| `fael board [--json] [--open] [--plan p] \| fael board --json --watch \| fael board --drop <root>` | the owner's view across every registered repo (`~/.local/state/fael/projects`, one main-checkout root per line, added by every `fael plan` and `fael chunk` command that writes; `--drop <root>` takes a finished repo off until a chunk command there adds it back), so it runs from anywhere: "needs you" (owner waits, review, ended runs, inbox drafts — SPEC-fael-board §5), the queue (ready chunks by unblocks, then plan order — §3; the text line says `no scope` for one whose scope is empty, which a lead's overlap check cannot place, issue 01M4WZ4M), running, blocked; per project the worktree pool (linked worktrees, else the main checkout; `run` = the live R on it), `branches` (every local branch but the base's: `local` never pushed, `pushed` a live upstream, `merged` its content on the base or its upstream gone — git alone, offline, whether a PR is open is the app's — with the unfinished chunks whose run branch it holds, so a lead's local stack names its lanes; decision 01M4V0FY), plans with per-state counts (an archived plan: counts only) and chunks with the derived §2 fields and the agent's last hand-off, and the `ledger` — open issues (in `find --groups` order, each with its `group`), decisions and notes, each with `signals` (`drifted`, doctor's rule) — read with no find recorded and no row marked seen (PLAN-fael-board l1); `ready` comes from the one core function; every plan carries `"truth": "db"`, a constant the app still reads, and `"parent"` (the umbrella plan's name, same app, one level; absent = top level — the app nests children under it, no name-prefix guessing); `--json` is the app's contract (`"v": 1`, `fael/tests/golden/board-v1.json`), `--open` keeps only open chunks — the planner's one read; `--plan p` keeps that plan's chunks (an agent's cut); `.fael/shared` (one repo-relative path prefix per line, scope's rules, `#` comments; absent = none) lists the files every chunk touches — registries, docs, goldens — and a shared path is nobody's claim: it never makes an overlap, `git merge-tree` stays its guard, the project carries it as `shared`, and `fael doctor` asks each one for a `merge` attribute (`merge=union` for an append-only file, else `merge=text`; a golden is re-blessed after a rebase), never writing it; a repo whose crew.db cannot be read (a newer schema) stays listed with `error` and empty lists, and the command exits 1 — never an empty board that reads as live; live git: one `worktree list`, one `origin/HEAD` and one `git log` (drift) per project; `--json --watch` streams board lines for a terminal or agent — the app reads `board --json` only (decision 01M59WYN) (chunk board-watch): a child of its caller, never a daemon — it exits when its stdin closes — that each second stats every project's sources (the db's `PRAGMA data_version` on a held read connection and the crew.db file, the ledger jsonl with `.fael/shared` and `config.toml`, and every ref file under `refs/` with `HEAD`, `packed-refs`, `FETCH_HEAD` and each `worktrees/*/HEAD`, recursively because a dir's mtime moves only for its own entries) and once a minute for the clock (`stalled`, `until`); it rereads only the project whose sources moved, answers git and the parsed ledger from memory until that project's refs or jsonl move, and prints one compact board line at start and then only when the board but for `at` changed — idle it spawns nothing and writes nothing |
| `fael sweep --input` | the sweeps one read — this repo only: untriaged issues with bodies and naming commits, open/running/waiting scopes, claimed issues, drifted decisions/notes with from, top file and count |
| `fael run end R [--cost USD] \| fael run beat R` | `end`: ends every live run row of start R — the launcher's cleanup; never another start, never a state; none left exits 0; claims each row's branch diff as `done` would, says each path outside the declared scope (`outside declared scope: …`, with `(overlaps open chunk …)` when another chunk holds it) so a lead sees it before landing, and `done --run R` keeps it — a lead ends a lane before rebasing it onto the lanes landed before it; `--cost` keeps what the client reported R cost (`run.cost` on the board); `beat`: start R's live rows are seen now — the launcher's heartbeat while a headless client runs, so a long run never reads stalled |
| `fael restore [<id>] [--edge id]` | revert a supersede edge with an event row — the row keeps its id and opens again; `--edge` names the superseding row when several edges still hide it; an already-open row or an already-reverted edge is info, never an error |
| `fael purge <id>` | permanently remove a leaked test row or a mistake: the row and its close and bump events go from every month file, tree and journal; refused when another row supersedes or restores it, when the id names a close event, or when it lives in an immutable compact file; copies already in another clone stay until purged there |
| `fael kickoff [anchor] [--full] [--limit N] [--offset M]` | the session brief: urgent first, then issues, decisions, notes by freshness (newer of the row and its files' last change); rows whose files are all gone are left out; a plan doc (`PLAN-x.md`) shows 5 rows without `--limit` and leaves out rows keyed `plan:x:chunk-<n>` once the doc's Progress ticks chunk n (`- [x] chunk n` or `[~]`; every other row stays, and the plan's `:handoff` row is listed first so the cap never cuts it); when crew.db holds that doc as a plan whose chunks live in the db, the chunk this worktree's live run holds (with the chunk rules) or else its first ready chunk comes first, with `fael chunk start <uid>`; an explicit `--limit N` is not cut by `budget.kickoff_tokens`; a shown row's `@<branch>` tag reads `@<branch> (merged)` when git proves that branch's work is in HEAD — its tip merged (a branch still on HEAD's first-parent line has no commits of its own and is not marked) or, for a squash merge, its whole diff from the merge base matching a commit's patch-id on HEAD — so a handoff written there may be stale; a deleted branch or anything unproven gets no mark, and `find` never spawns git for it; a handoff (a `:handoff` key) whose code files moved since it was written reads `(files changed since)` after the tag — the same channel, no new renderer, and the note's own plan file never counts |
| `fael hook <event> [--client c]` | hook entry point (see below) |
| `fael mcp [--pin]` | MCP server on stdio; `--pin` keeps every call on this cwd (for HTTP exposure) |
| `fael install [--client c] [--dry-run]` | detect installed clients and wire MCP, hooks and every skill in `fael/skill/` (fael, fael-debug, fael-review; fael-plan and fael-sweep moved to fael crew, decision 01M5AF2H — install removes the old embedded copy but never a symlink) into each one; a skill dir linked into a fael checkout is replaced by the release copy, and a link under a retired name (plan-with-marketing) into a checkout goes |
| `fael upgrade [--client c] [--dry-run] [--yes]` | updates the binary by its channel read from the exe path (brew → `brew upgrade zecalis/tap/fael`, npm → `npm i -g @zecalis/fael`, shell/ps1 → the `fael-update` beside it; unknown = none), then the new binary runs `install` that looks first: lists what is out of date, counts it, asks `[y/N]` before writing (`--yes` or no terminal skips the question; `update` is an alias). Session-start writes pending wiring itself and says one Notice; a failed write keeps the "run `fael upgrade`" warning. Session-start also starts `fael upgrade --auto` detached, at most once a day (`<state>/update.json`): it installs a release tag only after seeing it for a day, and the next session-start says one Notice (updated · failed · on Windows only "is out") and takes the result out. Off: `FAEL_NO_AUTO_UPDATE=1` or `auto_update = false` in `~/.config/fael/config.toml` |
| `fael compact [--writer id] [--before yyyy-mm] [--prune]` | maintenance: fold old rows into per-writer summaries |
| `fael sync` | push this writer's journal to `refs/fael/<repo-id>/<writer>` on `git config fael.remote` and ingest every other writer's ref back (id-union); no remote set → refuses and prints how, nothing pushed; a secret-looking row is neither pushed nor ingested; `fael purge` leaves a tombstone so a purged row does not come back; one sync at a time per clone; also run once per session by the session-start and Stop hooks (below), and plain `fael sync` works with no hook (cron on a hook-less node) |
| `fael import <path> [--map old/=new/]` | maintenance: import another clone's log (a log file, a log dir or a repo root — its journal plus any `.fael/log`, never other jsonl under it; a close folds into its row; twice is safe) |
| `fael doctor [--fix] [--fat] [--json]` | find and repair damaged logs — `--fix` moves bad lines to quarantine (never deletes them) and closes the confirmed `[Shipped]` notes — only status-shaped ones (the note's title or first line says PR opened/rebased, chunk N done, what shipped, not yet committed), never one with a `revisit` or a `*:handoff` key (those wait past the merge); a note on a landed branch that does not read as a status (a rule, a fact, `half done`) is listed as `[Shipped kept]` and left open; `--json` prints each problem's full row ids for a cleanup pass; prose in open rows, close reasons and every `*.md` is checked for dead id citations (`[Phantom]`); `[Superseded]` reports a legacy chain hidden by a supersede marker whose newest version is already closed (`fael close <id>` on each repairs it); `[Drifted]` lists open rows whose files took 10+ commits since they were written, for a check against the code; `[NoVerdict]` lists open rows that name a real file over the 1 MiB edit-push cap (`fael add` stamps up to 16 MiB, push compares up to 1 MiB), so push never says whether those files changed and the generic hint stands — one `metadata()` per file, a note only, never `--fix`; `[Unstamped]` lists open rows with no file stamp (`fh`) that name a real file push could compare, so push never gives them a verdict — once checked against the file, a bare `fael bump <id>` stamps it (a note only, never `--fix`: a bump says "still true", the reader's call) |
| `fael stats [--json] [--rows] [--day] [--misses] [--since d]` | how many bytes and tokens fael has put into agents' context (`--day` = today's panels per repo and summed; `--since` = only usage from then on; `--misses` = the newest empty text searches, machine-local — the plain page ends with a count line once any exist) |
| `fael report [--out f] [--open] [--since d]` | one offline HTML page for a lead: what memory reached the agents, what is noise, did fael add friction — numbers from `fael stats --json` |

`--files` in `find` matches a row's `files[]` only — exactly, as a directory (a zone), or by glob; an anchor's ref
never matches as a directory. It does not fall back to searching text; text is `find <text>`.
Queries expand through rename aliases first, so a row filed under a path that was renamed since (`git log -M`,
cached in `.fael/cache/aliases.json`, plus `fael mv` rows) still matches at the new path.
Ids are accepted as a unique prefix and printed at the shortest length that stays unique (≥ 8).

`.fael/config.toml` — every field is optional:

```toml
kinds = ["risk"]              # extra kinds on top of decision/issue/note
key_domains = ["auth", "db"]  # first key segment; outside the list = warning, never a reject
resolve = true                # follow renames (git log -M + fael mv rows); false = match files[] literally
store = "local"               # the only value (journal only, the default); "tracked" is rejected — share with fael sync or copy rows in with fael import <path>
[budget]
kickoff_tokens = 800          # kickoff, and find with no filter (unless --limit is given)
find_tokens = 800
push_tokens = 800             # edit hook push: rows first, then the edit hint and a stashed notice in what is left
push_rows = 5               # at most this many rows per push (0 = token budget only)
[warn]
row_tokens = 400
row_chars = 1200              # a single-topic-looking row can still run long
[anchor]
prefixes = ["PLAN-"]          # <PREFIX><name>.md widens kickoff to <prefix>:<name> (e.g. HANDOFF-); PLAN- is the plain default
[limit]
row_bytes = 10240             # hard cap, never above 10 KiB
[notify]
user = true                   # one line per beat for the user only (Claude systemMessage, OpenCode toast); false = off
[hint]
stop = []                     # ignored since the prompt key hint was cut (01M5528X)
[plan]
worktrees = "wt-fael-agent-"  # board pool: only linked worktrees whose path or dir name starts with it; unset = every linked worktree
[sync]
auto = true                   # session start and Stop run `fael sync` once per session and newest row when fael.remote is set; false = manual only
[lang]
marker = ["english", "thai"]  # Stop-hook phrase packs (default); [] switches the bug rule off
rows = ["english"]            # accepted row-writing languages; anything else warns once, never rejects ([] switches the check off)
```

### MCP (3 tools on stdio — each schema is paid for in every session, so the list stays short)

| Tool | Input | Notes |
|---|---|---|
| `find` | `id?` `ids[]?` `files[]` `text` `key` `kind` `since` `to` `by?` `all?` `revisit?` `limit` `offset` | read-only, `ids` pulls several bodies in one call under the same `find_tokens` budget as `full` (the first body always shows, the cut line names the ids left; a bad id reports alone, the rest print, the call is an error); a first page of one or two rows with no `limit` shows bodies like `full` when they fit that budget, else titles; `--json` is the machine shape and is never cut — over the budget with no `--limit` it prints one `fael: N rows, ~T tokens of JSON …` line on stderr instead; an empty find says why — each word, `files` and filter counted on its own — and an empty *text* search is also kept in the machine-local `find-misses.jsonl` (`fael stats --misses`), the recorded miss decision 01M4RREHH waits for; cut to `budget.find_tokens` unless `limit` is given — a named `limit` wins over the budget. No filter = the session brief (what `kickoff` shows) — so there is no `kickoff` tool. A cut list prints `next: offset=N` — repeat the call with it |
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

**The user channel** (`notice`, PLAN-fael-visible-secretary): fael does its work in the agent's context, so the user never saw it. Each hook can now carry one line for the user only. Claude Code gets it as `systemMessage`, which is shown to the user and never added to the model's context. OpenCode shows it as a toast. Codex and the bare neutral protocol have no such channel, so they stay silent. There are three beats. Session start names the rows the brief handed over. A push names the first decision or issue it reminded the agent of, at most once per file per session. Stop gives the turn's receipt (`filed · reminded · retired · closed`), and every count names up to two ids or keys that `fael find` takes back. When nothing happened there is no line: fael never says "nothing new", and never claims anything was prevented or saved. The receipt reads a per-session tally in the state dir (`<seen key>.tally`). `add`, `close` and the push append to it, and each Stop reads the lines since its last marker. The tally is never part of a row, and `additionalContext` is the same byte for byte with the channel on or off. `[notify] user = false` turns the channel off.

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

The write path self-heals before it validates (`fael/src/selfheal.rs`): `core.validate` sees one row and no log, so filling an absent `--supersedes` — from `Supersedes <id>` in the text or from the same kind + key of the caller's own row — and adopting the one key the row's files already carry happen here, where the log is readable. An `issue` is a finding, not a topic: a key may hold several, so a key match supersedes an issue only when it is the same finding re-filed (same words, a shared file), and a distinct issue sharing the key is kept and named. Shared files prove two notes related, not that one replaces the other: a repeat on the same files files the row and names what stays open (`kept all`) — a silent hide costs the next reader a todo, a kept note costs one `close`. Each choice is reported in one info line, which is not an ask — except a cross-key act (the superseded row's key differs from the new row's, reachable only by naming the row in the text): under `[selfheal] cross_key = "warn"` (the default) it prints one `warning:` line instead, which counts as an ask; `"info"` keeps the info line, `"off"` acts silently. Every act stamps `decision_source` (`explicit:text`, `identity:key`, each with `:cross-key` when the key moved, `caller:flag` for a resolving flag) so restore can trace an edge back to its cause; `heuristic:files` is a legacy label only — no new row mints it, but old rows carrying it still restore and doctor still counts them; older rows read as `unknown`. Several candidates file the row and name what was kept: fael never picks and never rejects for it. Fael doesn't try to understand everything. It makes only the decisions it can justify, exposes the evidence when it can't, and makes every automatic decision reversible.

**Push** — the agent edits a file (the Edit tool, or a shell call that wrote a file it names), and the memory for that file comes with it:
```
client ─(edit event)─▶ adapter.parse ─▶ core.find(files) ─▶ rank ─▶ cut to token budget ─▶ adapter.render ─▶ context attached to the edit
```
The `read` and `search` hooks still fire (they count tool use for `fael stats`) but reply empty: a read or a search names no change, and the row reaches the agent at the edit if one follows.
Ranking: an exact file match beats the same directory, which beats the same key. Open `issue` and `decision` rows go first, then newer before older by `id`. Text matching splits the query on whitespace: every word must be a case-insensitive substring of the row's text or title, in any order — no stemming or fuzzy match. A find that comes back empty answers in the same call (`nearest`): the same call's closed or superseded rows, else — for two or more words — the rows matching the most of them (at least half), labelled as such; `--json` never gets those guesses.
Ranking is **deterministic**: the same log, query and budget give the same output on any machine and any day — recency comes from `id` order, never from the clock, and ties break by `id`. No fuzzy, BM25 or semantic ranking.

Rows are then bucketed by the session **Focus** — the start branch and the keys of the open rows filed on it, written once at session start to
`~/.local/state/fael/sessions/<session+worktree>.focus.json`: **Now** (an open issue on the queried file, an urgent row, a row on the session branch, or sharing one of those keys) always renders, **File** (the queried file) fills the row cap next — unless more than `PUSH_HUB_ROWS` (8) File rows compete (a hub: a spec, a plan, PRODUCT.md), where freshness alone would pick off-topic rows, so only `PUSH_HUB_PEEK` (3) render, rows with a waiting `--revisit` first, once per file per session — **Background** (same directory, open issues included) never renders — the push header counts every hidden row (`(shown of total)`). The `… +N more — fael find …` count lines under the rows were dropped (PLAN-fael-say-gate chunk 4): 7 of 336 were followed by the call they printed. A shared-key row (the key of an exact hit, on another file) joins only when that key is in the Focus, where it is Now; otherwise it is neither pushed nor counted (a broad key spanning plans would read as an off-task push). The push reads that file back and rebuilds it only when `<gitdir>/HEAD` names another branch (another session switched the worktree): no git spawn, one small read. No session, no file or an unparsable file is `Focus::default()` — the ranking above, capped at `budget.push_rows`. The default branch (`origin/HEAD`'s, else `main` or `master`) is no start branch: every session shares it and every merge lands there, so its rows name the repo, not the session's work — a session there builds `Focus::default()` too (onoff pilot: on `main` they rode every push as Now, past the cap).

A push carries no cost line: the old `memory: ~<used>/<budget> tokens · <n> rows` footer repeated the header's row count and gave the agent nothing to act on, so it was dropped; the bill stays in `fael stats` (`est_tokens` per event).

A row is said once per **context window**, not once per session: the pushed ids are kept per session, worktree and `agent` (`<state>/sessions/<key>.seen`), keyed by the session id (a transcript path counts as its file stem, so `add` and `find` — which only know `$CLAUDE_CODE_SESSION_ID` — mark the same list), and read-filter-append runs under a file lock so parallel edits push a row once. A sub-agent starts with an empty context, so it has its own list — what its parent was told says nothing about what it knows (Claude Code sends `agent_id` on tool events inside a sub-agent; an OpenCode sub-agent is a child session with its own id). A session-start with `source: "compact"` or `"clear"` drops the thread's list, because the context lost the pushed rows.

**Capture** — the agent ends its turn. The Stop hook reads the last assistant message (`reply`, else the tail of a Claude transcript) and files every line of the form `fael decision|issue|note: <text> [files: a,b]` at column 0, outside a code fence, through the same validation as `fael add` (secrets, ids, size, paths). `[files: …]` is required and never inferred from the session's edits. A line that cannot be filed is dropped and becomes one hint on the next push — never a turn. The same lines seen twice in one session file once. `block` is always `false`: fael never stops a turn. A sub-agent's stop (`agent` set — Claude Code `SubagentStop`) files its own `reply` the same way and does nothing else: no bug rule, no transcript fallback (the transcript is the parent's).

The edit hook appends `{"path","at"}` to `~/.local/state/fael/sessions/<session+worktree>.jsonl` — per-machine runtime state, never in `.fael/`; `fael add` without `--files` files the session's edits since its newest row; more than 8 rejects with the list (`--force` files them all), since a row on every file it touched is pushed at each one. The `session` string sent with `edit` must equal the one sent with `stop`.

**Bug line** — a bug or risk announcement in the turn's text (the transcript tail after the latest user message) with no issue or close since the words stashes one line for the next push, shown once; never a turn. **Fix lines** (PLAN-fael-experience-loop chunk 5) catch the fix itself, the moment its cause → fix is worth keeping: a fix phrase (`fixed the bug`, `root cause was`, from the same packs) with no issue or close after it takes the bug line's place and offers `fael add issue` + `fael close`, and so does a `git commit` whose subject opens `fix:`/`fix(` and whose text names no row, in a session that closed none. Each is said once per session (`fixed`, `fixcommit` in `fael stats` `said`); fael never decides which issue a commit fixes. **Held line** (PLAN-fael-row-retire w1): an open decision or note — any writer, the session's own first, at most three — naming a file the session edited whose stamped blob differs from disk (no stamp counts, the edit is all fael knows), is named on the next push after Stop with its three verdicts ready: `fael bump <id>` (still true), `fael close <id> "now in `<path>`"` (the code holds it), re-file with `--supersedes <id>` (wrong now), a `from: user` row saying to ask the user first, once per session (`held`); issues stay out (they have the edit ask). The agent judges; an unjudged row whose files all changed goes dormant (§Row state) and this line is the only place it is asked about, once — silence means dormant. Never at the edit (decision 01M4RRNWC). `fael chunk done` prints the same over the files the chunk's diff touched, at most five, then `+N more: fael find --files <file>` — its own output, no hook line. The phrases come from the `[lang] marker` packs (`english` + `thai` by default, `marker = []` switches the rule off), never from hardcoded lists. The Stop-block mode (`[capture] block = true`) was removed 2026-10-03: 55 blocks, 12 followed by a row; an old config key is ignored.

Session start, and every stop, start one detached `fael sync` per session and newest row when `fael.remote` is set (`[sync] auto`): fail-quiet, never awaited, the hook still exits 0. Session start goes first, so teammates' rows reach the session's edits (not its kickoff context, already built) and a Stop with no new row starts nothing.

**Session start:**
```
client ─(session-start)─▶ write focus.json (start branch + the keys of the rows filed on it)
                        ─▶ open hand-offs of this branch's work · the chunk this worktree's live run holds · open rows `to` the reader ─▶ context (nothing = nothing said)
```
Decision 01M5528X (2026-10-17): session start says only what is handed to the reader. A hand-off is an open row with a `:handoff` key filed on the start branch or carrying a Focus key; the chunk line names the live run `fael chunk start` bound to this worktree and points at `fael chunk show <uid>`; then open rows `--to` the reader (or its agent client). Each is said once per context (`~start:<id>` in the seen list): a resume says nothing new, a compact or clear says it again. Nothing else rides session start — the count line, urgent unassigned, branch-tied issues, due revisits, decisions and the commit-named-issue line were cut (session-start hit 8% in `fael stats`); those rows reach the agent at the edit, or by `fael kickoff` / `fael find` when it asks. Kickoff is a pull, never a start ritual.
fael never infers which plan a session is in. `plan:<name>` anchors and `plan:<name>:handoff` / `plan:<name>:chunk-<n>` keys are a ledger convention: fael stores and matches them like any other anchor or key, and an agent asks for them itself (`fael find --key 'plan:<name>:*'`, `fael kickoff PLAN-<name>.md`). Which filename prefixes widen kickoff that way is repo config (`[anchor] prefixes`, default `PLAN-`) — a plain common name. A session's intent is not a fact the shared log can answer — any rule that picks one plan from it is a guess, and a wrong guess pushes another task's rows into Now.

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
- What an agent is shown (kickoff, push, `find`) is one markdown line per row: `- [id] kind #key text → files`. On real rows this adds 18.5% on top of the text, against 42.6% for raw JSON and 17.7% for TOON. The text is most of the size, so the savings come from choosing fewer rows, not from the format. One cut on top: a file push drops the file the agent is editing from each row's list (`→ also: <others> +N` past two), 14% fewer bytes over 25 file reads in one repo (2026-10-03, bytes not tokens). A pushed row that is not about the opened file says how it came, `- [id] (same dir) …` or `(same key) …`, and a push that cut rows names the cut in its header, `fael mem for <files> (shown of total):`.
- Every output is cut to a token budget (configurable per repo). The estimate is computed at read time and never stored, because every model's tokenizer counts differently. `est_tokens` stays the anchor unit (ASCII ≈ 4 bytes/token, non-ASCII ≈ 1 char/token) — a ruler, not a scale; convert with the frozen exchange table below — no per-model config, no formula tuning.

  | model | EN (× est) | TH (× est) |
  |---|---|---|
  | Claude 5 (Opus 5.5 / Sonnet 5, same tokenizer) | × 1.56 | × 1.05 |
  | Claude Haiku 4.5 | × 1.16 | × 1.01 |
  | o200k (tiktoken, offline) | × ~1.06 (est tracks within ±20%) | mixed rows track; pure-Thai est is the upper bound (~2.4× over o200k) |

  Measured 2026-09-28 via `count_tokens` over every row in the log (271 EN + 2 TH-mixed + 10 pure-Thai plan lines as proxies, framing overhead subtracted); Sonnet 5 and Opus 5.5 returned identical counts. Frozen 2026-09-28, not maintained — newer numbers come free from `real_tokens`, never from re-running this table.
- Every injection is recorded per machine (`~/.local/state/fael/usage.jsonl`, archived monthly under `usage/`, never in git), so `fael stats` shows fael's real cost in context. This is **local telemetry, not memory**: it may be lost, is never read to answer a query, and a failure to write it never fails a command. `.fael/` is the only semantic state.
- A row longer than about 400 estimated tokens triggers a warning when it is written, because it is paid for every time it is pushed. The hard limit is 10 KiB per row.
- The similar-key warning matches by parent only at ≥ 3 segments (`a:b:c` vs `a:b:d`); 2-level keys (`a:b` vs `a:c`) rely on levenshtein ≤ 2. This is an intentional trade-off, not a gap: with 2-level keys the shared parent is the bare domain, so a parent match would warn every new topic under it (domain reuse is the `key_domains` check instead), and a mixed-depth pair (`a:b` vs `a:b:c`) stays silent unless it is a typo away.
- A re-file (`--supersedes`, or a self-heal supersede) hears only the shape warnings the old row did not already earn — no key, several topics, long text, long title — so a translation or wording pass is not a wall of warnings agents learn to skip.

## 6. Self-healing

Reading never fails: broken lines, leftover merge-conflict markers, duplicate ids, CRLF and BOM are all handled in memory. Writing seals a torn last line before it appends. `fael doctor` reports problems, and `--fix` repairs them with tmp-then-rename. Bad lines go to `.fael/quarantine/`, so no byte is ever deleted.

`doctor` reads prose as well as bytes: open rows, close reasons and every `*.md` under the repo (outside `.git`/`target`/`node_modules`) are checked for citations of ids with no row behind them — `[Phantom]`, the dead citation the next reader takes as confirmation. Fenced code blocks are skipped there: a ULID inside a fence is an example, never a citation. This doctor sees only this repo's log, so an id from another repo's log reads as dead: cite that row by its key (`fael find --key <key>` there), which also survives a supersede.
`[Drifted]` is the safety net for rows the code outgrew: an open row whose real files took 10+ commits since it was written — `.md` files count only for a row that names no code, since docs move with nearly every PR (one `git log` spawn in `doctor`, never in core). It is a fact, never a verdict — the reader checks each row against the code: the code says it now → close it (`now in <file>`); wrong now → re-file with `--supersedes`; still true → `fael bump <id>` (same id, restamped), which restarts the count. The edit push asks for the same check on open issues while the agent has the code in front of it; decisions and notes get no edit ask — a row names only its file, so fael cannot tell whether the edit touched what it says, and asking on every edit of a hub file taught agents to skip the line (`say:ask-issues-only`) — so they retire at the Held line, at `chunk done`, and in `doctor`. The ask names a row once per session (the same words on every later edit would be noise) and leaves out rows that session filed itself. It has no per-turn limit: an open issue on the edited file is asked at that edit. The per-turn kind (the gone-check ask) speaks once a turn: the prompt hook writes a fresh turn id (`<seen key>.turn`) and the line said spends `~turn:<id>`, so a task that edits ten files gets one, not ten — a line held back keeps its key for a later turn. A client with no prompt hook marks no turn and keeps the per-key limit only.

`fh` is the same fact without the git spawn: at write time `fael add` and a bare `fael bump` hash each real file it names (`sha1("blob <len>\0" + bytes)`, CRLF read as LF in text files, the first 12 hex of `git hash-object` in a repo that stores LF, for a file with no lone `\r` and no NUL past byte 8000 — fael applies one rule on both sides, so the difference never reads as a change) into the row's `fh` map, so a reader can compare the file on disk to the bytes the row was written against and know whether this file changed — no commit count, no timer (`docs/format.md` §Rows). A row with no `fh`, or whose file left the repo, is unknown, never changed: fael never guesses. `fael claim` and a bump that only re-routes (`--to`, `--urgent`, `--revisit`) carry the old map forward on purpose — claiming or re-prioritising an issue is not checking it, and a restamp there would read as one. The map is small (≤ 8 files, 16 MiB each) because the row talks about the files it touches, not every path in the tree. A real file the stamp leaves out (over 16 MiB, unreadable, past the 8th) is named once, as an info line in the `add` / bare-`bump` receipt (CLI and MCP; `claim` and a routing bump print nothing, they restamp nothing) — the row is still filed, and anchors, globs, directories and missing files stay silent.

Every row — open, closed or superseded — whose line trips `validate::secret` (the one check add and import share) reports as `[Secret] <id>`: an error, `fixable = false`, never touched by `--fix`. The text says rotate first, then `fael purge <id>` — purge does not undo git history or other clones — and names the label and location, never the token. crew.db takes the same check: temp triggers on every connection reject a plan title, a chunk title, label, brief or wait text, and an event text (hand-off, note, answer) that trips it, so no write path stores one — a brief goes to the model provider and out through `fael board --json` and `fael plan export`. `validate::secret` is a pattern list (common token prefixes, a password in a URL), not a scanner: a plain password in prose passes.
Open rows with a letter outside every accepted `[lang] rows` script report as one `[NotEnglish]` batch (info, never `--fix`ed) with the full ids a translate pass supersedes (`fael add --supersedes <id>`, batched over `fael add --json -`); `fael stats` counts the same rows with the same detector against the running repo's accepted scripts (`rows not in English` under the default, never `rows with Thai`).

## 7. Non-goals

A query language, a daemon, embeddings, and hand-written tags or links. Links come for free from shared `files`, shared `key` and `supersedes`.

The local tool never needs a daemon or a server. A hosted server (MCP over HTTP for web chat hosts) is a separate product built on `fael-core` and this same format — it is not part of this binary. Three rules bind it:
- **A repo, when there is one, is the truth** — the hosted side is a git client that commits rows into it; it is canonical storage only for workspaces with no repo. Syncing is a union of lines deduped by `id` (append-only + ULID), so there is nothing to resolve.
- **It calls the same core entry points as the CLI** — `Stamp.by` from the signed-in user, no branch/sha. Hook session state (`~/.local/state/fael`) is CLI-only: HTTP clients have no edit or stop events, so there is nothing to carry to a phone.
- **Export and import go through this public format without loss of semantic memory** — no data stays locked in the hosted side.
