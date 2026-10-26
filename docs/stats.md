# fael stats JSON contract

> **Status:** `schema: 5` — `fael stats --json` prints one `fael-core::stats::Stats`
> struct as-is, so the CLI, the desktop app and any outside reader share the
> shape by construction. The human text (`fael stats`) is unstable by design;
> only `--json` is a contract.

## Source

Every number comes from two inputs, joined as pure functions in
`fael-core::stats` (no spawn, no clock, no filesystem):

- `usage.jsonl` under the state dir (`FAEL_STATE_DIR`, else
  `~/.local/state/fael`) — one row per injection into context, with `ask`
  (`warning`, with its `code` — `fael/src/hook/codes.rs`, never the text; old
  rows may carry `reject`, which no field reads since schema 3, or
  `stop-block`, which no field reads since schema 2), `session`, `agent` (the sub-agent
  whose context the push landed in — absent on the session's own thread),
  `real_tokens` on some rows, and `row` on an add's `warning` (the id just
  filed — not in `ids`, which count pushes; `session` there is its writer).
  A line leaves out a zero or empty `bytes`, `est_tokens`, `ids` and `npos`:
  absent = 0 / empty (`flags` stays on a call line — its presence is the window
  `usage.by_flag` counts over). An edit writes one 0-byte `event: "in-context"` row when decisions or issues
  about that very file were already in the session's context — their ids go
  under `in_context`, never `ids` (nothing was pushed), each id once per
  session. Notes in context at that edit ride the same row under
  `in_context_notes` (nothing in `value` reads them). It is no injection: no
  count above includes it. A push line (`read`, `search`, `edit`, `shell-edit`),
  and the `in-context` row of an edit, also carry `files` — the repo-relative
  files the push was about (every other line carries none); an edit where fael said
  nothing and had nothing in context writes no line at all. A push line is also its
  decision record (PLAN-fael-learn-loop chunk 1; `rows[].outcomes` reads `cut`):
  `trigger` (`read` · `edit` · `shell-edit` · `search` (the client named
  the files, so how it found them is not ours to say) · a search's `reader-arg` / `hitlist` /
  `glob`), `feat` (per row said, and the first 20 cut: `tier`, `hub`,
  `kind`, `age_d`) and, when rows were cut, `cut` (`[{id, r}]`, at most 20,
  `r` = `cap` · `hub_peek` · `budget`) with `cut_n` (all of them). A cut row is
  a system limit, not a policy's verdict. Lines written before 2026-10-14 may also carry
  `policy`, `arm`, `feat.touch` and `would_drop` (`{policy, ids}`: the said rows the
  retired shadow policy `touch@1` would have cut, the agent saw them all); `rows[].outcomes`
  still reads `would_drop` there. A hook reply also carries `said` — one
  `{kind, key?}` per line it said: `row` (row id), `ask` (row id; `*` in
  older lines, the generic clause before every ask named its row), `pointer` (key), `count` (usage before PLAN-fael-say-gate chunk 4 dropped the line — one per count line:
  `<files>|file`, `<files>|dir:<dirs>`, `<files>|key:<key>` or `<files>|keys`),
  `brief` (no key — its rows are the line's `ids`),
  `bodies` and `notice` (no key). A pull that showed rows (`find`,
  `mcp-find`, `kickoff`) writes a 0-byte row with the shown ids under `found`
  (not `ids`) and its query's `key`/`files`/`id` under `q` — never free text;
  like `in-context` it is no injection. So is an `outcome` row (PLAN-fael-learn-loop
  chunk 2): `cited` lists the ids from the session's seen list that a tool input or
  the closing reply typed, once per id per session. So is a `call` row
  (PLAN-fael-agent-ergonomics chunk 1, the `friction` field below): one per agent
  call, `cmd` and `outcome` only. So is a `silent-start` row (PLAN-fael-say-gate
  chunk 4c): a session-start that said nothing, 0 bytes, so a count per
  session-start sees every start. A reject is counted once, by its `call` row;
  a batch with several bad items is one reject (`partial`). Torn lines are
  skipped; temp-dir repos (the OS temp dir and `/tmp`, where agent
  scratchpads live) are skipped unless the state dir itself is scratch.
  The live file is `usage.jsonl`; at the first write of a new month it moves
  to `usage/<YYYY-MM>.jsonl` (the month of its last write), so a read never
  has to open the whole history. With no `--since` a read covers the newest
  archive and the live file — this and last month; `--since all` reads every
  month. `--since <YYYY-MM-DD | RFC 3339>` opens the archives from that month
  on and keeps only the lines stamped at or after it, before anything is counted — first use per repo, rows added and
  `capture` then all read as that window. `fael report` renders this same
  `Stats` (with `--rows`) as one offline HTML page.
- the repos' logs (tree + journal union) — for row statuses, rows added
  and language share. A repo that no longer resolves reads empty
  (its rows resolve `unknown`).

## Schema rule

`schema` bumps **only** when a field is removed, renamed, retyped or
redefined. Adding a field never bumps — readers skip unknown keys — it just
gets a changelog line below. A bumped number without a reader for the old
shape is a breaking change: ship the reader first.

## Fields (`fael stats --json`)

| Field | Type | Meaning |
|---|---|---|
| `schema` | u32 | contract version, currently `5` |
| `events` | u32 | usage rows counted |
| `bytes` | u32 | summed `bytes` |
| `est_tokens` | u32 | summed `est_tokens` (estimate, never `token`) |
| `skipped_temp` | u32 | rows skipped as temp-dir repos |
| `by_event` | map name → `{events, est_tokens}` | per `event` (`read`, `edit`, `stop-work`, `capture`, …) |
| `by_client` | map name → `{events, est_tokens}` | per `client` (`claude`, `codex`, …) |
| `top_rows` | `[{id, pushes}]` × ≤10 | most-pushed row ids, pushes desc then id asc |
| `asks` | `{warning}` → `{events, bytes, codes}` | warning lines fael put in front of the agent, `codes` by warning code (`untitled`, `long_text`, `no_key`, `similar_key`, …; `uncoded` = a line from before codes); plain pushes never count. Rejects are in `friction` |
| `constants` | `{skill_bytes, skill_est, mcp_schema_bytes, mcp_schema_est}` | bytes the agent pays every session before saying anything; SKILL.md is LF-normalised before measuring so the count is checkout-independent |
| `rounds` | `{rows_added, since}` | `rows_added` = rows filed at or after each repo's first usage · `since` = the first-usage day (`YYYY-MM-DD`), empty when no rows |
| `non_english_rows` | `{rows, non_english}` | rows outside the running repo's accepted `[lang] rows` scripts (deduped by id) |
| `capture` | object | adds and silent sessions (fields below) |
| `retired` | object | `pushed` = distinct rows a `read`/`edit` push handed over · `at_touch` = of those, closed, superseded or bumped within a day after one of those pushes — how many rows the edit-push ask retires where they went stale |
| `value` | object | the line `fael stats` prints first: `in_context_at_edit` = distinct (session, row) pairs from `in-context` rows whose row an earlier push of that session handed over (a row the agent filed or found itself does not count) · `issues_closed` = issues closed at or after the repo's first usage, a close `fael compact` folded into its row included (deduped by id) · `handoffs_picked_up` = distinct `*:handoff`-keyed rows a push handed over · `by_event` = map push event → `{pushed, in_context_at_edit}`: each (session, row) pair counts once, for the event of the push that first handed it over — hit rate per event, over the usage lines stamped at or after each client's first `in-context` line (before it no hook could write one, so those pushes could never score and are left out of both numbers; a client with no such line has no entry, and the sums can be lower than the plain totals). Three limits: a lower bound (an `in-context` line exists only when the agent edits the row's file, so a read-only session scores every row a miss); the first push claims the pair and a push never repeats a row the session already holds, so a later event carries only what is new, and an `edit` push can only hit on a later edit. The cost per event is the top-level `by_event`. · `cross_agent` = `{other_session, other_worktree, written_during_session, other_client, by_client, writer_unknown}`, each of the first four and each `by_client` value `{pushed, in_context_at_edit}`: pushed (session, row) pairs whose row names a writer session (`fael add` inside a hook session tags it; older rows and rows filed outside a session do not count), over the same window as `by_event` — `other_session` = the writer is not the session handed the row · `other_worktree` = and the writer session's usage lines name a worktree the receiver is not in (a writer with no usage lines is unknown, not counted) · `written_during_session` = and the row was written after the receiver's first usage line (two agents at once, one's row reaching the other); the three are not exclusive; `other_client` = and both sessions' usage lines name a client (the agent's, not `cli`/`mcp`) and they differ — `by_client` splits it `"<writer>→<receiver>"`, e.g. `claude→opencode`; `writer_unknown` = pushed pairs whose row names no writer session, which none of the above can place (a client that gives `fael add` no session id, or a row older than the tag) — read it first: a row it hides is never counted as crossing; `in_context_at_edit` = of those pairs, rows still in context when the agent edited their file · `same_file` = `{sessions_seen, files, session_pairs}`, two sessions at one file, read from the `files` on edit usage lines (`edit`, `shell-edit` and the `in-context` row an edit writes; independent of the writer-session tag above and of the `by_event` window): two distinct sessions are **concurrent** on a file when both have such a line for the same repo path and file and the two lines are at most **10 minutes** apart (`OVERLAP_WINDOW_MS`; nearest pair decides, a longer idle is not shown to be at the file). `sessions_seen` = distinct sessions with at least one such line (the base: 0 = nothing recorded yet) · `files` = distinct (repo path, file) pairs with two concurrent sessions · `session_pairs` = distinct unordered session pairs concurrent on at least one file. A lower bound, never a collision count: only edits that wrote a line are seen (an edit where fael said nothing and the row was not in context writes none), lines from before `files` was recorded name nothing, a session without an id is skipped, and worktrees have their own repo path so one file edited in two worktrees is never matched — nothing is guessed beyond what the rows show. A count of what crossed agents, never of what it saved. The text line adds `retired.at_touch` |
| `said` | map kind → `{said, earned}` (`held` also carries `acted`) | yield per line kind a hook says, every kind listed (zeros included): `row`, `note` (a `row` entry whose row is a note), `brief`, `ask`, `pointer`, `bodies`, `notice`, `cited` (a `git commit` naming an open issue), `finding` (a ReportFindings entry offered as a ready `fael add issue`), `fixed` (the agent said it fixed a bug — a `[lang]` fix phrase such as `fixed the bug` or `root cause was` — with no issue or close after the words; stashed by Stop, said on the next push, once per session), `fixcommit` (a `git commit` whose subject opens `fix:`/`fix(` and whose text names no row, in a session that closed none; once per session), `overlap` (an edit, in a worktree holding a chunk run, of a file outside that chunk's declared scope and inside another unfinished chunk's: one line with a ready `fael chunk note`, once per file per session; `earned` = the session ran `fael chunk note` after the line — a chunk note is no ledger row, so its `call` usage line is the evidence), `held` (no longer said: the Stop → next push line was cut by decision 01M5H5AQV; the entry stays for old usage rows and carries `acted`, below). `said` = `said` entries of that kind; `earned` = of those, acted on later in the same session — `row`/`note`/`brief`: the id in an `in-context` row (`in_context` or `in_context_notes`) or closed/superseded within a day · `ask`/`cited`/`held`: the id closed, superseded or bumped within a day (`*` is not counted) · `pointer`: a pull whose `q.key` is that key · `bodies`: a pull by id · `finding`: an issue naming the finding's file filed by the session after it · `fixed`/`fixcommit`: an issue the session filed after it (a close carries no session, so it cannot earn) · `notice`: a row the session filed after it. Usage entries of the retired `count`, `merge`, `carry` and `promote` lines are skipped. `held` also carries `acted`: per `shown` source (`held`, `done`, later `add`/`start`) the retire events stamped at or after the repo's first usage whose row names that source, split `close` / `supersede` (reverted edges skipped) / `bump` — one entry per stamped event, so a row bumped then closed counts in both (the fix for 01M57RES on the new count). The old `earned` rule is unchanged: it still reads the row's earliest retire event within a day, rows filed before r1 have no stamp, and the default window spans two months; m2 reads `acted`. `ask:hub` / `ask:file` split `ask` by the file the row sat on, hub as the push recorded it (`feat.<id>.hub`), with the same earned — listed only once an ask carries `feat`, so the two read against one bar instead of a hand count. An upper bound (the agent may have done it anyway): read it only to cut a kind, never as proof one works |
| `unused_rows` | `[{id, kind, pushes}]` × ≤20 | open decisions and issues handed over ≥ 20 times (counted over the usage lines stamped at or after each client's first `in-context` line) with no `in-context` line ever naming them, pushes desc then id asc — a row to close, bump or `--supersede`. A note never lists (`in-context` does not measure notes). Never-seen-used, not proven useless: a read-only session writes no `in-context` line. The text line prints the first 10 with the commands; nothing to list = no line |
| `friction` | `{calls, rejects, help, find_repeat, first_call_ok, reasons, by_command, by_client, shape_gate, shape_gate_rules}` | what a call to fael cost the agent (PLAN-fael-agent-ergonomics chunk 1), from the `event: "call"` usage lines: one per agent call — `cmd` (the command, `?` when none), `sub` (the `chunk`/`plan` verb, when a real one), `outcome` (`ok` · `reject` with its `reason` · `help`), `empty` (a find that matched nothing) and `sig` (a hash of the arguments, never the text). A `reason` is a code from the closed table in `fael/src/hook/codes.rs` (`unknown_flag`, `chunk_missing`, `chunk_prefix`, `chunk_ambiguous`, `chunk_other_plan`, `id_not_shaped`, `id_is_path`, `id_missing`, `id_ambiguous`, `id_partial`, `scope_overlap`, `held`, `chunk_state`, `missing_value`, `bad_format`, `row_state`, `shape_gate`, …; `other` = a message no needle knows, which the source test forbids); a `shape_gate` reject adds `rules` (the faults it named, as warning codes) and an `unknown_command` one `near` (the real command it was a near miss of, from the command table — never what was typed). `bad_value` and a gate with no `rules` are lines from before 2026-10-13. `no_chunk` and `bad_id` are lines from before this change. The CLI writes one only inside an agent session (a hook session id in the env) and only for `add find close kickoff bump claim next keys mv chunk plan board` (plus any reject or help); MCP calls always write one, session-less. A `call` line is no injection: it never counts in `events`/`by_event`. `calls` = lines · `rejects` = `reject` lines, split by `reasons` · `help` = `help` lines · `find_repeat` = a `find` within 2 calls after an empty `find` of the same stream whose arguments differ (an identical repeat is a re-poll) · `first_call_ok` = `ok` calls that are no `find_repeat` and have no reject, help or `find_repeat` among the next 2 calls of their stream (`FIRST_CALL_WINDOW`; a stream is one session, or one repo and client when the line has no session). Per call, not per task: fael cannot see the task, and an empty find on a file nothing was filed on counts as a repeat when another find follows. `by_command` = the same counts, `reasons` included, per `cmd` (`chunk start`, `plan export`: per verb when the line has `sub`), so a change is read where it happened. Friction per 100 calls = `(rejects + help + find_repeat) / calls × 100`; first-call success = `first_call_ok / calls`. `by_client` = the same counts per the call line's `client`: a CLI call reads it from the env keys `session.rs` reads (`FAEL_SESSION` → `opencode`, `CLAUDE_CODE_SESSION_ID` → `claude`, `CODEX_THREAD_ID` → `codex`, none → `cli`), an MCP call is `mcp`; lines before 2026-10-26 are all `cli`; the text prints a `friction by client` block when there are two or more. `shape_gate` = rejects that wrote nothing on purpose ("rejected: nothing written —": the add shape gate and the derived-files cap): a round fael asks for to get a readable row, so it is in no count above — not in `calls`, `rejects` or `reasons`, and it spoils no first call; lines written before 2026-10-09 carry `bad_value` for it. `shape_gate_rules` = those gates by the rules they named (one gate can name several). The text line prints both, total and per command with its reasons most first, plus `shape gate ×N: untitled ×a, … (not friction)` when there is one; gate rejects with no other call print that line with `n/a` for the rate; no call lines and no gate reject = no text |
| `usage` | `{since, by_flag}` | which flags are really used (PLAN cli-tiers chunk measure), from the `event: "call"` lines that carry a `flags` list (every CLI call since 2026-10-19 — `ok`, `help` and `reject` alike; names only, never a value). `by_flag` = one entry per `cmd` (`chunk start`: per verb; `flag` null = every call to it) and per `flag`: `{cmd, flag, calls, agent, human, ok, help, reject}`, most calls first. `human` = a line tagged `who: "human"`: a call with no agent session in the env, or a clean call to a command outside `add find close kickoff bump claim next keys mv chunk plan board` (so an agent running `stats` counts as human) — these lines are in `usage` only, never in `friction`. `since` = day of the first counted line (empty = none): the window, since older lines carry no flags. Cannot see: MCP calls (no flags in their lines), the board app (it runs `board` from `/`, outside any repo, and a human line needs a `.git` or `.fael` above the cwd), calls from a repo with no session that is not a repo, hooks, and a flag a human never typed because the docs told them not to. Counts show use, not value. The text line prints the 8 most-called entries with `since` |
| `context_loop` | `{confirmed_repeats, edits_after_close, useful_shows}` | PLAN-fael-context-loop §3, read together — fewer rows or tokens with more repeats is worse, not better. `confirmed_repeats` = issues filed at or after the repo's first usage whose `supersedes` names an issue already closed (a close row or a close `fael compact` folded in; reverted edges skipped; deduped by id) — the agent said the bug came back, fael never decides two bugs are one · `edits_after_close` = distinct (session, closed issue) pairs where an edit usage line of the session (`edit`, `shell-edit`, `in-context`) named one of the issue's files after its close — the base the repeats read against, in events never days; a lower bound (an edit where fael said nothing and found nothing in context writes no line), so the repeat rate is an upper bound · `useful_shows` = (repo, session, row) units fael said that were then cited or acted on (`rows[].outcomes` definitions, union: both counts once), read against the top-level `est_tokens`. The text line prints `context loop: confirmed repeats a of b edits after close · useful shows c (~x per 1k tokens)` |
| `experience` | `{fixed, fixed_from_review, closed_with_check, repeats_with_check, edits_after_close_with_check, fix_commits, fix_commits_linked}` | PLAN-fael-experience-loop chunk 4, read as capture → acted → outcome beside numbers other fields hold (capture: `said.finding`, `capture.edit_session_issues`; outcome: `context_loop`, `said.check`) — never inferred. A **check** is a backticked path holding a `/` in a close text (a close row, or the close `fael compact` folded in); a **fix** is, for a close before the fix-evidence cutoff, a close naming a sha (7–40 hex digits with a digit and a letter) or `(#N)`, or a default-branch commit whose message names the issue id (full, or a ≥ 8-char prefix); for a close at or after it, a default-branch commit citing `(fael:<prefix>)` that resolves to the issue alone, or holding the close's `(#N)` — the fix-evidence rule (`docs/format.md` § fix reached main). `fixed` = issues closed at or after the repo's first usage with a fix (H4; deduped by id) · `fixed_from_review` = of those, the issue's writer session had a `finding` line on one of its files before filing it · `closed_with_check` = issues closed at or after first usage whose close names a check · `repeats_with_check` / `edits_after_close_with_check` = the part of `context_loop.confirmed_repeats` / `edits_after_close` whose closed issue named a check; the rest named none (H2) · `fix_commits` = commits on the default branch (`origin/HEAD`'s branch, else `main`; `origin/<branch>` first, else the local one) since the repo's first usage whose subject starts `fix:` or `fix(`, merges skipped, deduped by sha · `fix_commits_linked` = of those, the message names a row id, or a close names its sha (a prefix) or the `(#N)` its subject carries (H5 capture recall). `fael stats` reads the commits with one `git log` per repo — never on a push; no git or no branch = none. A fix inside a `feat:` commit and a repo without conventional commits are outside the base, so the recall is the hole that is left, never a promise. The review half of capture is Claude-Code-limited: a finding is seen only when the reviewing model calls ReportFindings, and a review that prints its findings as text is not seen (`/code-review` did so at level low and called ReportFindings at level high, both 2026-10-09), so `said.finding` and `fixed_from_review` are a floor, never the share of reviews filed — the capture line says `(ReportFindings calls only)`. fael does not read findings out of free text: that would be inferring them. The text block prints four lines (`experience — capture: …`, `acted: …`, `outcome: …`, `recall: …`); nothing counted = no block |
| `lifecycle` | `{decision, issue, note}` → `{live, held, dormant, retired}` | this repo's rows by state per kind, read at the call from the log and the disk (not from usage): `held` = open, some but not all stamped files changed · `dormant` = open, every stamped file changed, no bump since · `retired` = closed, superseded, or a note whose chunk is done / branch merged · `live` = the rest; the four add up to every row of the kind. All zeros outside a repo. `report_closed` (omitted at 0) = issues filed `from: report` that are closed or superseded. The text page leads with it |
| `flow` | `{filed_to_close, start_to_done, done_to_merge}` each `{median, p90, n}` in seconds + `{runs_with_pr, runs_total}` | PLAN-fael-flow chunk f1: how long the loop takes, read for the owner's weekly review (ship faster or cut scope?) — never anyone else's norm. `filed_to_close` = journal add ts → close row ts for the same id (deduped by id, both ends at or after the repo's first usage) · `start_to_done` = run `started` → `ended` per chunk in this clone's crew.db (in the usage window) · `done_to_merge` = run `ended` → the PR's merge time on the default branch, matched by PR number only (`(#N)` in squash subjects/merge commits, `gh`'s merged list number) — never a sha (a squash rewrites it); `runs_with_pr` of `runs_total` runs carry a PR, unmappable chunks shrink `n`, never imputed. Window = same as `--since`; no db, git or `gh` = zeros |
| `throughput` | `[{week, done}]` | PLAN-fael-flow chunks f1+f2: distinct chunk dones per calendar week (Monday, UTC, `YYYY-MM-DD`) from `Run.ended` in the usage window, read by the owner for plan sizing (how many chunks per week is real?). A restarted chunk (several runs, one uid) lands once, in the week of its latest `ended`. Weeks between the first and last done week print zero, never missing; no runs = `[]` |
| `days` | `[{day, events, est_tokens}]` | issue 01M5S63N: push events and tokens fael put in front of agents per UTC day (`YYYY-MM-DD`), first to last usage day of the window, zero-filled; no usage = `[]`. One pass, so a chart reads the last N days from one call instead of looping `--since`. `fael stats --days --json` prints just `{"days": [...]}` — no repo logs, git or `gh` — for callers that refresh often New per-day measures join as fields of the same record, never a new series |
| `outcomes_v` | u32 | version of the outcome definitions behind `rows[].outcomes`, currently `1` — a changed definition bumps it |
| `rows` | array, only with `--rows` | `[{id, pushes, status, noise, outcomes}]` × ≤20; `status` is `open` · `closed` · `superseded` · `unknown`; `noise` = pushed ≥ 10 times; rows only ever cut (`pushes` 0) rank after pushed ones. `outcomes` = what happened to the row after fael said or cut it, each a count of **sessions** (one session, one row = one observation, however many pushes repeated it), observations only — no weights, no causal claim: `shown` = sessions fael said it in · `cut` = map reason → sessions it was cut in (`cap`, `hub_peek`, `budget` are system limits; `gate` and `would_drop` are a policy's decision, none exists yet) · `cited` = said, then its id (≥ 8 chars) typed into a later tool input or the closing reply (`outcome` usage lines; a `fael …` shell command, a tool response and an id fael never said do not count) · `pulled` = `{agent_initiated, fael_induced}`: said, then a later `find`/`kickoff` showed it; `fael_induced` = the query is one a line fael said earlier in the session printed (a pointer's key, an edit hint's id, or a count line's call in older usage), `agent_initiated` = anything else · `acted` = said, then closed, superseded or bumped within a day · `retrieved_after_cut` = cut (any reason), then the agent pulled it itself (never a `fael_induced` pull) while it was still unsaid — a `would_drop` row was said, so its pull counts whenever it comes · `missed_push` = a `gate`/`would_drop` row that was `retrieved_after_cut` and then cited or acted on (the rate is over gate-cut rows, not every cut). A pull that does not note the row in the session's seen list (`kickoff`) cannot be followed by a cite, so `missed_push` is a lower bound. `dup` (SPEC §B) is not measured yet |

`capture` (PLAN-fael-dev-adoption): `manual_adds` = rows added since the repo's first usage, by any writer · `sessions_with_edits` = sessions that edited a file ·
`sessions_with_edits_no_row` = of those, sessions with no row filed during them (+10 min) — a signal to
look at, not a verdict. Worktrees share one journal, so a row counts for a session only when it is that session's: its writer
`session` (transcript stem) when the row has one, else its `branch` against the session's edit events
(`branch` on `edit` usage rows; either side absent = it counts) ·
`sessions_with_edits_gone` = of `sessions_with_edits`, sessions whose repo path no longer exists (a
removed worktree): its journal cannot be found from the path, so they are unknown, never counted in
`sessions_with_edits_no_row` ·
`edit_session_issues` = issues the `sessions_with_edits` filed (same window and ownership as above, deduped by id) — issues per edit session, PLAN-fael-experience-loop chunk 4 ·
`no_row_sessions` = the newest ≤10 of those as `[{repo, session, from, to}]`, for a human to judge. Older usage holds `event: "capture"` rows from the reply capture removed in schema 4; only `by_event` still counts them.

`null` never appears in `Stats`: an absent `rows` means
`--rows` was not passed (not zero pushes). A zero `share`-like value is always
a measured zero, never "unknown" — `DayView` keeps the same rule (`null` =
unknown, see its section below).

## Fields (`fael stats --day --json`)

`fael stats --day` prints one local day as `fael-core::stats::DayView` — the
same struct the desktop popover reads over IPC (PLAN-fael-desktop chunk 4),
so the CLI and the app cannot disagree. Pure: `now`, the timezone offset and
the loaded logs are parameters; the caller reads the environment
(`FAEL_TZ_OFFSET` like `+07:00` wins over the machine zone) and the clock.
No state dir → zeros with `repos: []`, exit 0.

| Field | Type | Meaning |
|---|---|---|
| `schema` | u32 | contract version of **this** shape, currently `2` (`DAY_SCHEMA`, versioned apart from `Stats`) |
| `day` | `YYYY-MM-DD` | the local day `now` falls in |
| `tz_offset` | `±hh:mm` | the offset used (`+00:00` for UTC) |
| `all` | object | the five panels + `timeline` over every repo |
| `repos` | `[{repo, …panels}]` | per repo with activity today, path-sorted |

Each panel (identical shape under `all` and inside `repos[]`):

| Panel | Field | Type | Meaning |
|---|---|---|---|
| Delivered | `rows` | u32 | usage pushes carrying ≥1 id today (per push, not distinct ids) |
| | `by_client` | map client → u32 | pushes per client |
| | `last` | `[{id, title, file}]` × ≤5 | newest pushed rows; `title`/`file` from the repo's log, `""`/id when the repo is gone |
| Light on context | `fael_tokens` | u32 | sum of `est_tokens` today |
| | `session_tokens` | u64 | summed input-side `real_tokens` (in + cache-create + cache-read) of rows that measured |
| | `share` | f64, else `null` | `fael_tokens / session_tokens` — `null` when nothing measured (or the sum is 0): shown as `—`, never estimated |
| Memory | `added` | map kind → u32 | log rows **filed today** by kind (carriers and alias rows excluded) |
| | `closed` | u32 | close rows filed today |
| | `open_issues` | u32 | current open issues (not day-scoped) |
| | `superseded` | u32 | current rows hidden by an active supersede edge |
| For you | — | object, else `null` | `null` when no writer is set (hidden, never guessed); else `rows`, `from` (map writer → u32), `urgent`, `revisit_due` over open rows routed to the viewer |
| Health | `stale_issues` | u32 | open issues older than 14 days |
| Timeline | `bucket_min` | u32 | `15` — bucket size, 96 buckets per day |
| | `delivered` | [u32] × 96 | pushes per 15-minute bucket of the local day |
| | `fael_tokens` | [u32] × 96 | `est_tokens` per bucket |

All-repos vs per-repo: `context` and `timeline` recompute over the union (so
`share` is exact, not averaged); `delivered.rows`/`by_client` are
summed; the log panels (`memory`, `for_you`,
`health.stale_issues`) are built once from the repos' logs deduped by id, so
worktrees that share one journal count its rows once; `delivered.last` is
newest-first across repos.

## Changelog

Newest first.

- `5` (2026-10-24): board gains a Models read (run-telemetry t5): `Board.models` joins crew.db run rows (client/model/cost/started, worktree as the agent-started gate, session as the usage join key) with unknown-% beside every cost number — new key only, so no bump (`STATS_SCHEMA` frozen for this plan; old NULL rows never backfilled).

- `5` (2026-10-25): added `days` (events and `est_tokens` per UTC day, issue 01M5S63N); no bump.
- `5` (2026-10-19): throughput dedupes restarts by chunk uid (latest `ended` wins), so the series sums to the distinct done-chunk count; no bump.

- `5` (2026-10-19): added `usage` (calls per command and flag, agent and human, ok/help/reject; PLAN cli-tiers chunk measure) — a new key only, so no bump (`STATS_SCHEMA` bumps only on a removed or redefined field). `ok` call lines now carry `verb`/`flags`/`npos`, and a call outside an agent session writes a `who: "human"` line that `friction` skips.
- `5` (2026-10-19): added `flow` (filed→closed, start→done, done→merge spans with merge coverage) and `throughput` (chunk done per calendar week); no bump.

- `5` (2026-10-15): `file_verdict`, `incidents` and `experience.label` removed with their text lines. `file_verdict` read the `changed`/`unchanged` keys that read pushes stopped writing on 2026-10-10, so it was a frozen number; no row was ever keyed `incident:<kind>`; `label` measured the label contract for a parked plan (decision 01M4V39ZF6) and `find_hit` was never measurable. `close_shape()` stays as the contract's tested table. Bump: fields were removed.
- `4` (2026-10-15): `capture.reply_lines`, `capture.reply_stored` and `capture.reply_rejected` removed with the reply capture itself — the Stop hook no longer files `fael <kind>:` lines (zero stored since 2026-09-30; decision 01M4YRAT had already dropped the syntax from the skill). `manual_adds` now counts every add, and the text line drops `filed from replies`. Bump: fields were removed.
- `3` (2026-10-14): `said.held` widened (PLAN-fael-row-retire w1): any writer's open decisions and notes on an edited file that changed since the row was written, not only decisions the session wrote — a window across this day reads two lines; same shape, no bump.
- `3` (2026-10-14): push lines no longer write `policy`, `arm`, `feat.touch` or `would_drop`, and the `dup` outcome line is gone — the retired push gate (`touch@1`, its stages and holdout) wrote them and only `fael tune` read them; `rows[].outcomes` still counts `would_drop` on older lines. Usage lines only, `Stats` is unchanged; no bump.
- `3` (2026-10-14): `fael tune` removed with the measurement plans it served (decision 01M4V39ZF6, issue 01M4V3JC); it was a separate command, `Stats` is unchanged; no bump.
- `3` (2026-10-13): `asks.reject` removed — a reject was written twice (an `ask` row with no reason or session, and its `call` row), and the two disagreed; the `call` row is the one count now, agent sessions only. `asks.warning` gained `codes`; `friction` reasons are closed codes (`bad_value` retired), split per command in `by_command[].reasons`, `chunk`/`plan` per verb; `friction` gained `shape_gate_rules`; the CLI counts clean `chunk`, `plan` and `board` calls too, so their reject rate has a denominator. Bump: a field was removed (issues 01M4RRD1F, 01M4RRD1A).
- `2` (2026-10-10): `said` gained `held` (PLAN-fael-decision-held chunk 2, the held-by close); new map key only; no bump.
- `2` (2026-10-09): `friction` gained `shape_gate`; a pre-write gate reject (reason `shape_gate`, was `bad_value`) leaves `calls`, `rejects`, `reasons` and first-call success (decision 01M4FNQ1); no bump.
- `2` (2026-10-09): `experience` gained `label` (`close_core`, `guard`, `key_reuse`, `find_hit`, each `{state, num, den, since}`, plus `gone_repos`, PLAN-fael-label chunk 3); read from the logs only; no bump.
- `2` (2026-10-21): `said` gained `overlap` (issue 01M5D9GB); new map key only; no bump.
- `2` (2026-10-08): `said` gained `carry` (PLAN-fael-experience-loop chunk 6, the carry-back line); new map key only; no bump.
- `2` (2026-10-08): `said` gained `fixed` and `fixcommit` (PLAN-fael-experience-loop chunk 5, the fix-time lines); new map keys only; no bump.
- `2` (2026-10-08): added `experience` (fixes, checks, repeats split by check, fix-commit recall) and `capture.edit_session_issues` (PLAN-fael-experience-loop chunk 4); `fael stats` now reads the default branch's commits with `git log`; no bump.
- `2` (2026-10-08): added `capture.sessions_with_edits_gone`. A session whose repo path no longer exists (a removed worktree) used to read as an empty log and count in `sessions_with_edits_no_row`; it now counts as unknown. This is a correction, not a redefinition, so no bump. On the author's usage: 73 of 144 became 24, with 49 gone. Removed paths no longer get a log entry at all, which every other join already read as no rows.
- `2` (2026-10-08): added `context_loop` (confirmed repeats, edits after close, useful shows, PLAN-fael-context-loop chunk 6); read from existing usage lines and logs; no bump.
- `2` (2026-10-07): added `friction` (rejects by reason, helps, repeat finds and first-call success per agent call, total and per command, PLAN-fael-agent-ergonomics chunk 1); usage gains the `call` line, which no count includes; no bump.
- `2` (2026-10-05): `file_verdict` gained `no_fh` (of `no_verdict`, rows with no stamp in the log) and `retire` (`{changed, unchanged}` of `{pairs, retired}`, PLAN-fael-file-hash chunk 4b); nested keys only, the top-level keys are unchanged; no bump.
- `2` (2026-10-05): `fael tune` (read-only replay of candidate push rules, PLAN-fael-learn-loop chunk 4) reads the same usage lines and writes none; `Stats` is unchanged; no bump.
- `2` (2026-10-05): added `outcomes_v` and `rows[].outcomes` (shown, cut by reason, cited, pulled by provenance, acted, retrieved_after_cut, missed_push); usage gains the 0-byte `outcome` line (`cited`), which no count includes; no bump.
- `2` (2026-10-04): added `file_verdict` (shown rows by file-hash verdict: `changed`, `unchanged`, `no_verdict`), read from the `changed`/`unchanged` keys read pushes already write on usage lines; lines without them are not measured; nothing a hook says changes; no bump.
- `2` (2026-10-04): added `incidents` (rows keyed `incident:<kind>` per week); no bump.
- `2` (2026-10-04): added `value.cross_agent.same_file` (sessions editing one file within 10 minutes of each other); edit and `in-context` usage lines gain `files`, which readers of older rows never see and old readers ignore; no bump.
- `2` (2026-10-04): added `said` (yield per line kind); usage rows gain `said`, `in_context_notes` and the pull rows (`found`, `q`), none of which changes an existing count; no bump.
- `2` (2026-10-04): added `unused_rows`; no bump. The text `fael stats` prints it in place of the top-10 `row X: pushed ×N` lines (`top_rows` in `--json` is unchanged).
- `2` (2026-10-03): the Stop-block mode is gone (fael never blocks a turn), so
  every field that read its usage rows went with it: `stop_blocks`,
  `repeat_blocks`, `real_tokens` (post-block cost), `asks.stop-block`,
  `rounds.after_block`, `capture.post_stop_rounds`. Old `stop-*` usage rows
  still count as events in `events`/`by_event`. No reader of `1` ships: the
  CLI and `fael report` use the `Stats` struct itself, and the desktop app is
  not built yet.
- `DayView: 2` (2026-10-03): `health.ignored_blocks` removed with the
  Stop-block mode; `health` is `{stale_issues}`.
- `1` (2026-10-03): added `value.by_event` (hit rate per push event, windowed to when each client could write `in-context` lines); no bump. The text `fael stats` prints it beside each event's cost.
- `1` (2026-10-17): added `lifecycle` (rows by state per kind); no bump.
- `1` (2026-10-18): `said.held` gained `acted` (PLAN-fael-row-retire r2): per `shown` source (`held`, `done`, later `add`/`start`) the stamped retire events in the window, split `close` / `supersede` / `bump`, one entry per event — the fix for 01M57RES on the new count; the old `earned` rule is unchanged; no bump.
- `1` (2026-10-03): `cross_agent` gained `other_client`, `by_client`, `writer_unknown`, and now joins a Claude session's usage path to the row's UUID (before, a row pushed back to its own Claude writer counted as another session and `other_worktree` missed Claude writers); no bump.
- `1` (2026-10-03): added `value.cross_agent` (rows that crossed sessions, worktrees, live); no bump.
- `1` (2026-10-01): added `value` (the value line) and `in-context` usage rows; no bump.
- `1` (2026-10-01): `--since` cuts the input to a window and the temp filter also skips `/tmp`; same shape, no bump.
- `1` (2026-10-01): added `retired` (pushed rows closed or superseded within a day of a push); no bump.
- `1` (2026-09-30): usage rows may carry `agent`; readers ignore it today — no bump.
- `1` (2026-09-30): added `capture` (reply capture, manual adds, silent sessions); no bump.
- `1` (2026-09-29): first frozen shape. `schema` key added; everything else
  byte-identical to the pre-core output.
- `DayView: 1` (2026-09-29): first frozen day shape (`fael stats --day --json`).
