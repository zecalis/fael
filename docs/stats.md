# fael stats JSON contract

> **Status:** `schema: 2` — `fael stats --json` prints one `fael-core::stats::Stats`
> struct as-is, so the CLI, the desktop app and any outside reader share the
> shape by construction. The human text (`fael stats`) is unstable by design;
> only `--json` is a contract.

## Source

Every number comes from two inputs, joined as pure functions in
`fael-core::stats` (no spawn, no clock, no filesystem):

- `usage.jsonl` under the state dir (`FAEL_STATE_DIR`, else
  `~/.local/state/fael`) — one row per injection into context, with `ask`
  (`reject` · `warning`; old rows may carry `stop-block`, which no field
  reads since schema 2), `session`, `agent` (the sub-agent
  whose context the push landed in — absent on the session's own thread),
  `real_tokens` on some rows, and `row` on an add's `warning` (the id just
  filed — not in `ids`, which count pushes; `session` there is its writer).
  An edit writes one 0-byte `event: "in-context"` row when decisions or issues
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
  `glob`), `policy` (`baseline@1`), `feat` (per row said, and the first 20 cut: `tier`, `hub`,
  `kind`, `age_d`, and `touch` = how many of the row's files this session's working set (the files its earlier pushes were on) already held — absent with no session) and, when rows were cut, `cut` (`[{id, r}]`, at most 20,
  `r` = `cap` · `hub_peek` · `budget`) with `cut_n` (all of them). A cut row is
  a system limit, not a policy's verdict. `would_drop` (`{policy, ids}`, chunk 3) names the said rows the shadow policy `touch@1` would have cut — a row with `touch` 0 unless it is an issue or a handoff; recorded only, the agent saw them all. A hook reply also carries `said` — one
  `{kind, key?}` per line it said: `row` (row id), `ask` (row id; `*` in
  older lines, the generic clause before every ask named its row), `pointer` (key), `count` (one per count line:
  `<files>|file`, `<files>|dir:<dirs>`, `<files>|key:<key>` or `<files>|keys`),
  `brief` (no key — its rows are the line's `ids`),
  `bodies` and `notice` (no key). A pull that showed rows (`find`,
  `mcp-find`, `kickoff`) writes a 0-byte row with the shown ids under `found`
  (not `ids`) and its query's `key`/`files`/`id` under `q` — never free text;
  like `in-context` it is no injection. So is an `outcome` row (PLAN-fael-learn-loop
  chunk 2): `cited` lists the ids from the session's seen list that a tool input or
  the closing reply typed, once per id per session. Torn lines are
  skipped; temp-dir repos (the OS temp dir and `/tmp`, where agent
  scratchpads live) are skipped unless the state dir itself is scratch.
  `--since <YYYY-MM-DD | RFC 3339>` keeps only the lines stamped at or after
  it, before anything is counted — first use per repo, rows added and
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
| `schema` | u32 | contract version, currently `2` |
| `events` | u32 | usage rows counted |
| `bytes` | u32 | summed `bytes` |
| `est_tokens` | u32 | summed `est_tokens` (estimate, never `token`) |
| `skipped_temp` | u32 | rows skipped as temp-dir repos |
| `by_event` | map name → `{events, est_tokens}` | per `event` (`read`, `edit`, `stop-work`, `capture`, …) |
| `by_client` | map name → `{events, est_tokens}` | per `client` (`claude`, `codex`, …) |
| `top_rows` | `[{id, pushes}]` × ≤10 | most-pushed row ids, pushes desc then id asc |
| `asks` | `{reject, warning}` → `{events, bytes}` | asks fael put to the agent; plain pushes never count |
| `constants` | `{skill_bytes, skill_est, mcp_schema_bytes, mcp_schema_est}` | bytes the agent pays every session before saying anything; SKILL.md is LF-normalised before measuring so the count is checkout-independent |
| `rounds` | `{rows_added, since}` | `rows_added` = rows filed at or after each repo's first usage · `since` = the first-usage day (`YYYY-MM-DD`), empty when no rows |
| `non_english_rows` | `{rows, non_english}` | rows outside the running repo's accepted `[lang] rows` scripts (deduped by id) |
| `capture` | object | reply capture, manual adds and silent sessions (fields below) |
| `retired` | object | `pushed` = distinct rows a `read`/`edit` push handed over · `at_touch` = of those, closed, superseded or bumped within a day after one of those pushes — how many rows the edit-push ask retires where they went stale |
| `value` | object | the line `fael stats` prints first: `in_context_at_edit` = distinct (session, row) pairs from `in-context` rows whose row an earlier push of that session handed over (a row the agent filed or found itself does not count) · `issues_closed` = issues closed at or after the repo's first usage, a close `fael compact` folded into its row included (deduped by id) · `handoffs_picked_up` = distinct `*:handoff`-keyed rows a push handed over · `by_event` = map push event → `{pushed, in_context_at_edit}`: each (session, row) pair counts once, for the event of the push that first handed it over — hit rate per event, over the usage lines stamped at or after each client's first `in-context` line (before it no hook could write one, so those pushes could never score and are left out of both numbers; a client with no such line has no entry, and the sums can be lower than the plain totals). Three limits: a lower bound (an `in-context` line exists only when the agent edits the row's file, so a read-only session scores every row a miss); the first push claims the pair and a push never repeats a row the session already holds, so a later event carries only what is new, and an `edit` push can only hit on a later edit. The cost per event is the top-level `by_event`. · `cross_agent` = `{other_session, other_worktree, written_during_session, other_client, by_client, writer_unknown}`, each of the first four and each `by_client` value `{pushed, in_context_at_edit}`: pushed (session, row) pairs whose row names a writer session (`fael add` inside a hook session tags it; older rows and rows filed outside a session do not count), over the same window as `by_event` — `other_session` = the writer is not the session handed the row · `other_worktree` = and the writer session's usage lines name a worktree the receiver is not in (a writer with no usage lines is unknown, not counted) · `written_during_session` = and the row was written after the receiver's first usage line (two agents at once, one's row reaching the other); the three are not exclusive; `other_client` = and both sessions' usage lines name a client (the agent's, not `cli`/`mcp`) and they differ — `by_client` splits it `"<writer>→<receiver>"`, e.g. `claude→opencode`; `writer_unknown` = pushed pairs whose row names no writer session, which none of the above can place (a client that gives `fael add` no session id, or a row older than the tag) — read it first: a row it hides is never counted as crossing; `in_context_at_edit` = of those pairs, rows still in context when the agent edited their file · `same_file` = `{sessions_seen, files, session_pairs}`, two sessions at one file, read from the `files` on edit usage lines (`edit`, `shell-edit` and the `in-context` row an edit writes; independent of the writer-session tag above and of the `by_event` window): two distinct sessions are **concurrent** on a file when both have such a line for the same repo path and file and the two lines are at most **10 minutes** apart (`OVERLAP_WINDOW_MS`; nearest pair decides, a longer idle is not shown to be at the file). `sessions_seen` = distinct sessions with at least one such line (the base: 0 = nothing recorded yet) · `files` = distinct (repo path, file) pairs with two concurrent sessions · `session_pairs` = distinct unordered session pairs concurrent on at least one file. A lower bound, never a collision count: only edits that wrote a line are seen (an edit where fael said nothing and the row was not in context writes none), lines from before `files` was recorded name nothing, a session without an id is skipped, and worktrees have their own repo path so one file edited in two worktrees is never matched — nothing is guessed beyond what the rows show. A count of what crossed agents, never of what it saved. The text line adds `retired.at_touch` and `capture.reply_stored` |
| `said` | map kind → `{said, earned}` | yield per line kind a hook says, every kind listed (zeros included): `row`, `note` (a `row` entry whose row is a note), `brief`, `ask`, `pointer`, `count`, `bodies`, `notice`. `said` = `said` entries of that kind; `earned` = of those, acted on later in the same session — `row`/`note`/`brief`: the id in an `in-context` row (`in_context` or `in_context_notes`) or closed/superseded within a day · `ask`: the id closed, superseded or bumped within a day (`*` is not counted) · `pointer`: a pull whose `q.key` is that key · `count`: a pull by the call the line printed — for `file`/`dir:` a `q.files` path naming one of its files or a directory over one (at a `/` boundary: `src` covers `src/a.rs`), for `key:<key>` that `q.key`, for `keys` any `q.key` · `bodies`: a pull by id · `notice`: a row the session filed after it. An upper bound (the agent may have done it anyway): read it only to cut a kind, never as proof one works |
| `unused_rows` | `[{id, kind, pushes}]` × ≤20 | open decisions and issues handed over ≥ 20 times (counted over the usage lines stamped at or after each client's first `in-context` line) with no `in-context` line ever naming them, pushes desc then id asc — a row to close, bump or `--supersede`. A note never lists (`in-context` does not measure notes). Never-seen-used, not proven useless: a read-only session writes no `in-context` line. The text line prints the first 10 with the commands; nothing to list = no line |
| `incidents` | map week → map kind → u32 | incidents a human filed: rows keyed `incident:<kind>` or `incident:<kind>:<slug>` (e.g. `incident:duplicate-work:parser`, `incident:contradicted-decision:auth`), any row kind, counted by filing week — the week's Monday, `YYYY-MM-DD`, UTC — and by `<kind>`. Rows filed at or after the repo's first usage (so `--since` cuts them too), deduped by id; a superseded row is a rewrite of the same incident and drops out, a closed one still counts. Give each incident its own `<slug>`: a second row on one key supersedes the first. Never inferred — no row, no incident; no week listed = none filed. The text line prints the newest 4 weeks |
| `file_verdict` | `{changed, unchanged, no_verdict, no_fh, retire}` | what a push could say about the files of the rows it showed (file-hash verdict), counted over the usage lines that carry the shadow keys — a read push writes `changed` and `unchanged` (id lists, possibly empty) beside `ids`; an edit push, the session-start push and every line from before the keys existed carry none and are **not measured**, never counted as `no_verdict`. Unit = distinct (repo, session, row id) pairs, the key `value.by_event` uses; each pair counts once, for the first measured line that showed it, so a duplicate id or a repeat push is one pair, and a measured line with no `session` is left out. `changed` = the id is in the line's `changed` list (a file changed since the row was written) · `unchanged` = in its `unchanged` list (every file still matches) · `no_verdict` = shown (in `ids`) and in neither list. The three sum to the measured pairs. `no_verdict` has three reasons the usage line cannot tell apart: the row has no `fh` stamp, a stamped file is over the 1 MiB push cap (`hook/changed.rs` `PUSH_MAX_BYTES`, though the stamp covers up to 16 MiB), or the file is gone. `no_fh` splits off the first by joining the pair to the repo's log: of `no_verdict`, the rows whose log entry carries no `fh` (as the log folds it now — a no-stamp row restamped by a later bump reads as stamped); `no_verdict - no_fh` is the stamped rows with no verdict plus any row the log cannot find. `retire` is `{changed, unchanged}`, each `{pairs, retired}`: the retire rate that gates the `(changed)` label. A pair counts only once an edit push in the same repo touched one of the row's files twice after the read push (first edit = where the ask is said, second = the deadline); it is `retired` when the row was closed, superseded or bumped after the read push and before that second edit. Counted in events, never days; files are compared as the hooks wrote them (no alias resolution), so a row on a since-renamed file drops out. The text line reads `retired by the 2nd edit: changed a/b · unchanged c/d` with the gate: waits until each side holds 30 pairs, then passes when the changed rate is at least twice the unchanged one and `changed` is at most a third of every shown pair A count of what push could not say, never of what it saved. The text line (`file verdict at push: …`) prints only when at least one pair was measured |
| `outcomes_v` | u32 | version of the outcome definitions behind `rows[].outcomes`, currently `1` — a changed definition bumps it |
| `rows` | array, only with `--rows` | `[{id, pushes, status, noise, outcomes}]` × ≤20; `status` is `open` · `closed` · `superseded` · `unknown`; `noise` = pushed ≥ 10 times; rows only ever cut (`pushes` 0) rank after pushed ones. `outcomes` = what happened to the row after fael said or cut it, each a count of **sessions** (one session, one row = one observation, however many pushes repeated it), observations only — no weights, no causal claim: `shown` = sessions fael said it in · `cut` = map reason → sessions it was cut in (`cap`, `hub_peek`, `budget` are system limits; `gate` and `would_drop` are a policy's decision, none exists yet) · `cited` = said, then its id (≥ 8 chars) typed into a later tool input or the closing reply (`outcome` usage lines; a `fael …` shell command, a tool response and an id fael never said do not count) · `pulled` = `{agent_initiated, fael_induced}`: said, then a later `find`/`kickoff` showed it; `fael_induced` = the query is one a line fael said earlier in the session printed (a pointer's key, a count line's call, an edit hint's id), `agent_initiated` = anything else · `acted` = said, then closed, superseded or bumped within a day · `retrieved_after_cut` = cut (any reason), then the agent pulled it itself (never a `fael_induced` pull) while it was still unsaid — a `would_drop` row was said, so its pull counts whenever it comes · `missed_push` = a `gate`/`would_drop` row that was `retrieved_after_cut` and then cited or acted on (the rate is over gate-cut rows, not every cut). A pull that does not note the row in the session's seen list (`kickoff`) cannot be followed by a cite, so `missed_push` is a lower bound. `dup` (SPEC §B) is not measured yet |

`capture` (PLAN-fael-dev-adoption): `reply_lines` = `fael <kind>:` lines seen in
replies = `reply_stored` + `reply_rejected` · `manual_adds` = rows added since the repo's first usage that
did not come from a reply line, by any writer · `sessions_with_edits` = sessions that edited a file ·
`sessions_with_edits_no_row` = of those, sessions with no row filed during them (+10 min) — a signal to
look at, not a verdict. Worktrees share one journal, so a row counts for a session only when it is that session's: its writer
`session` (transcript stem) when the row has one, else its `branch` against the session's edit events
(`branch` on `edit` usage rows; either side absent = it counts) ·
`no_row_sessions` = the newest ≤10 of those as `[{repo, session, from, to}]`, for a human to judge. Replies are recorded as `event: "capture"` usage rows (`capture: stored|rejected`,
`row` = the filed id; never `ids`, a capture is no push).

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

## `fael tune [--json] [--since d]`

Read-only replay (PLAN-fael-learn-loop chunk 4, SPEC §C). `core::stats::tune`
prints `Tune`: candidate rules that only *cut* are replayed over the search
pushes already in `usage.jsonl` and set beside `baseline@1`. It names no
winner, picks no threshold and writes nothing — a human reads the table. `Stats`
is untouched, so no `STATS_SCHEMA` bump; `Tune` carries its own `outcomes_v`.

- **Unit.** One said search row in one session (the same observation
  `rows[].outcomes` counts). A row is *replayable* when its push line recorded
  `feat.touch` and the row is still in the log; the rest are counted in
  `sizes.rows_said` but not replayed (older lines, no session: unknown, not 0).
- **Candidates.** `touch@1` calls the pinned `touch_drops` on each row, and its
  `dropped` must equal `sizes.recorded_would_drop` (what the shadow wrote).
  `touch-yield@1` drops what `touch@1` would, unless the row was engaged
  (cited, pulled by the agent itself, or acted on) in ≥10% of earlier sessions,
  read from the first of (row,file,trigger), (row,file), (row), (class) that has
  ≥5; with none it keeps the row. History is sessions that had *ended* before
  the row was said. `decay` = how many earlier sessions per key count
  (`none`, 20, 50, 100, 200).
- **Rates** are `{x, n, lo, hi}` — `x/n` with its Wilson 95% interval, `null`
  bounds when `n` is 0. `exposure` = rows still said / rows said; `retained`
  = an outcome on rows the candidate keeps / that outcome on all of them;
  `retrieved_after_cut.rows` = dropped rows the agent pulled itself,
  `.sessions` = sessions that did (a pull fael induced is never evidence);
  `missed_push` = dropped rows the agent pulled itself and then cited or acted on.
- **Coverage** (provisional thresholds from decision `01M45DKB4`: ≥3 days, busiest
  day ≤50%, busiest session ≤10% of search pushes) and **strata** (repo × client;
  eligible at ≥10 sessions and ≥100 search pushes) are reported, never tuned.
- **Validation** (PLAN-fael-learn-loop chunk 5, SPEC §E) appears once push lines
  carry an `arm` — `push_policy = "touch@1"` in `.fael/config.toml`. Candidate
  sessions are cut for real (`gate`); holdout sessions run `baseline@1`. The
  replay tables above leave candidate-arm lines out (their rows are what the gate
  left). The block prints arm sizes per stratum with the trigger-mix guard (> 10
  points = `unbalanced`, not compared), exposure (rows said per session), what the
  gate forfeits (the candidate replayed on holdout rows), `missed_push` over gate
  cuts, and the sessions that went back for a cut row — then one verdict:
  `validated`, `not_validated` or `insufficient_data`, with reasons. `dup` is not
  measured, so the session gate rests on `retrieved_after_cut`. A human files the
  decision row; `tune` writes nothing.
- **An upper bound.** The replay holds the session fixed: a row a candidate had
  dropped would have stayed unseen and could have been said at a later push,
  which the log cannot show. Read every cost as at most what the data says.
- **Association** is a rate per feature value (and phi for a yes/no feature):
  observed, not a cause. The causal test is the holdout (SPEC §E).

## Changelog

Newest first.

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
