# fael stats JSON contract

> **Status:** `schema: 1` — `fael stats --json` prints one `fael-core::stats::Stats`
> struct as-is, so the CLI, the desktop app and any outside reader share the
> shape by construction. The human text (`fael stats`) is unstable by design;
> only `--json` is a contract.

## Source

Every number comes from two inputs, joined as pure functions in
`fael-core::stats` (no spawn, no clock, no filesystem):

- `usage.jsonl` under the state dir (`FAEL_STATE_DIR`, else
  `~/.local/state/fael`) — one row per injection into context, with `ask`
  (`reject` · `stop-block` · `warning`), `session` and `real_tokens` on some
  rows. Torn lines are skipped; temp-dir repos are skipped unless the state
  dir itself is scratch.
- the repos' logs (tree + journal union) — for stop-block outcomes, row
  statuses and language share. A repo that no longer resolves reads empty
  (its rows resolve `unknown`).

## Schema rule

`schema` bumps **only** when a field is removed, renamed, retyped or
redefined. Adding a field never bumps — readers skip unknown keys — it just
gets a changelog line below. A bumped number without a reader for the old
shape is a breaking change: ship the reader first.

## Fields (`fael stats --json`)

| Field | Type | Meaning |
|---|---|---|
| `schema` | u32 | contract version, currently `1` |
| `events` | u32 | usage rows counted |
| `bytes` | u32 | summed `bytes` |
| `est_tokens` | u32 | summed `est_tokens` (estimate, never `token`) |
| `skipped_temp` | u32 | rows skipped as temp-dir repos |
| `by_event` | map name → `{events, est_tokens}` | per `event` (`read`, `edit`, `stop-work`, …) |
| `by_client` | map name → `{events, est_tokens}` | per `client` (`claude`, `codex`, …) |
| `top_rows` | `[{id, pushes}]` × ≤10 | most-pushed row ids, pushes desc then id asc |
| `stop_blocks` | map event → `{blocks, followed_by_row}` | per `stop-*` event; `stop-bug` counts a following `issue` row, others any following row |
| `asks` | `{reject, stop-block, warning}` → `{events, bytes}` | rounds fael cost the agent; plain pushes never count |
| `repeat_blocks` | u32 | a block following a block in one session with no row between |
| `constants` | `{skill_bytes, skill_est, mcp_schema_bytes, mcp_schema_est}` | bytes the agent pays every session before saying anything |
| `rounds` | `{after_block, rows_added, since}` | `since` is the first-usage day (`YYYY-MM-DD`), empty when no rows |
| `non_english_rows` | `{rows, non_english}` | rows outside the running repo's accepted `[lang] rows` scripts (deduped by id) |
| `real_tokens` | object, else absent | mean cost of the round after a stop-block: `post_block_rounds`, `avg_input`, `avg_cache_create`, `avg_cache_read`, `avg_output` |
| `rows` | array, only with `--rows` | `[{id, pushes, status, noise}]` × ≤20; `status` is `open` · `closed` · `superseded` · `unknown`; `noise` = pushed ≥ 10 times |

`null` never appears: an absent `real_tokens` means no post-block round had
transcript `usage` (not zero tokens), and an absent `rows` means `--rows`
was not passed (not zero pushes). A zero `share`-like value is always a
measured zero, never "unknown" — chunk 3's `DayView` keeps the same rule
(`null` = unknown, see its section when it lands).

## Changelog

- `1` (2026-09-29): first frozen shape. `schema` key added; everything else
  byte-identical to the pre-core output.
