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
  session. It is no injection: no count above includes it. Torn lines are skipped; temp-dir repos (the OS temp dir and `/tmp`, where
  agent scratchpads live) are skipped unless the state dir itself is scratch.
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
| `retired` | object | `pushed` = distinct rows a `read`/`edit` push handed over · `at_touch` = of those, closed or superseded (a bump is a supersede) within a day after one of those pushes — how many rows the edit-push ask retires where they went stale |
| `value` | object | the line `fael stats` prints first: `in_context_at_edit` = distinct (session, row) pairs from `in-context` rows whose row an earlier push of that session handed over (a row the agent filed or found itself does not count) · `issues_closed` = issues closed at or after the repo's first usage, a close `fael compact` folded into its row included (deduped by id) · `handoffs_picked_up` = distinct `*:handoff`-keyed rows a push handed over · `by_event` = map push event → `{pushed, in_context_at_edit}`: each (session, row) pair counts once, for the event of the push that first handed it over — hit rate per event, over the usage lines stamped at or after each client's first `in-context` line (before it no hook could write one, so those pushes could never score and are left out of both numbers; a client with no such line has no entry, and the sums can be lower than the plain totals). Three limits: a lower bound (an `in-context` line exists only when the agent edits the row's file, so a read-only session scores every row a miss); the first push claims the pair and a push never repeats a row the session already holds, so a later event carries only what is new, and an `edit` push can only hit on a later edit. The cost per event is the top-level `by_event`. · `cross_agent` = `{other_session, other_worktree, written_during_session, other_client, by_client, writer_unknown}`, each of the first four and each `by_client` value `{pushed, in_context_at_edit}`: pushed (session, row) pairs whose row names a writer session (`fael add` inside a hook session tags it; older rows and rows filed outside a session do not count), over the same window as `by_event` — `other_session` = the writer is not the session handed the row · `other_worktree` = and the writer session's usage lines name a worktree the receiver is not in (a writer with no usage lines is unknown, not counted) · `written_during_session` = and the row was written after the receiver's first usage line (two agents at once, one's row reaching the other); the three are not exclusive; `other_client` = and both sessions' usage lines name a client (the agent's, not `cli`/`mcp`) and they differ — `by_client` splits it `"<writer>→<receiver>"`, e.g. `claude→opencode`; `writer_unknown` = pushed pairs whose row names no writer session, which none of the above can place (a client that gives `fael add` no session id, or a row older than the tag) — read it first: a row it hides is never counted as crossing; `in_context_at_edit` = of those pairs, rows still in context when the agent edited their file. A count of what crossed agents, never of what it saved. The text line adds `retired.at_touch` and `capture.reply_stored` |
| `rows` | array, only with `--rows` | `[{id, pushes, status, noise}]` × ≤20; `status` is `open` · `closed` · `superseded` · `unknown`; `noise` = pushed ≥ 10 times |

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
| `schema` | u32 | contract version of **this** shape, currently `1` (`DAY_SCHEMA`, versioned apart from `Stats`) |
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

- `2` (2026-10-03): the Stop-block mode is gone (fael never blocks a turn), so
  every field that read its usage rows went with it: `stop_blocks`,
  `repeat_blocks`, `real_tokens` (post-block cost), `asks.stop-block`,
  `rounds.after_block`, `capture.post_stop_rounds`. Old `stop-*` usage rows
  still count as events in `events`/`by_event`. `DayView: 2` (same day):
  `health.ignored_blocks` removed.
- `1` (2026-10-01): added `value` (the value line) and `in-context` usage rows; no bump.
- `1` (2026-10-03): added `value.cross_agent` (rows that crossed sessions, worktrees, live); no bump.
- `1` (2026-10-03): `cross_agent` gained `other_client`, `by_client`, `writer_unknown`, and now joins a Claude session's usage path to the row's UUID (before, a row pushed back to its own Claude writer counted as another session and `other_worktree` missed Claude writers); no bump.
- `1` (2026-10-03): added `value.by_event` (hit rate per push event, windowed to when each client could write `in-context` lines); no bump. The text `fael stats` prints it beside each event's cost.

- `1` (2026-10-01): `--since` cuts the input to a window and the temp filter also skips `/tmp`; same shape, no bump.
- `1` (2026-10-01): added `retired` (pushed rows closed or superseded within a day of a push); no bump.
- `1` (2026-09-30): usage rows may carry `agent`; readers ignore it today — no bump.
- `1` (2026-09-30): added `capture` (reply capture, manual adds, silent sessions); no bump.
- `1` (2026-09-29): first frozen shape. `schema` key added; everything else
  byte-identical to the pre-core output.
- `DayView: 1` (2026-09-29): first frozen day shape (`fael stats --day --json`).
