# fael log format v1

One page, public. Any tool that follows it can read and write a fael log without fael.
Reference implementation: [`fael-core`](../fael-core/src).

## Layout

```
.fael/
  config.toml                  optional
  log/
    <writer>/
      2026-09.jsonl            add rows written in that month (UTC, from ts) — append-only
      2026-09.close.jsonl      close rows written in that month — append-only
      compact.<ULID>.jsonl     immutable (+ a `.close.jsonl` companion when closes name no row here)
    _import/<ULID>.jsonl       immutable (+ a `.close.jsonl` companion, same rule)
  quarantine/<file>.<ULID>.jsonl  lines `doctor --fix` removed — never re-read, never deleted
  cache/aliases.json             rename cache from `git log -M`, gitignored, rebuildable — never a source of truth
  .lock                        not in git
```

- `.gitattributes`: `.fael/log/**/*.jsonl merge=union`
- `<writer>` = `<slug of git user.name>-<first 4 hex of sha256(lowercase git user.email)>`, e.g. `delamind-3f9a`.
  Slug = lowercase ASCII letters and digits, anything else collapses to one `-`; empty → `anon`. No email → hash the hostname.
  The email is never written — only the hash. A writer id never starts with `_` or `.`.
- Files are UTF-8, LF, one JSON object per line.

## Rows

**Add row** — in `<writer>/<yyyy-mm>.jsonl`:

```json
{"v":1,"id":"01J8ZQ3K4M7N2P5R8T1V4X6Y9A","ts":"2026-09-25T10:00:00Z","by":"delamind-3f9a","kind":"decision","text":"…","files":["src/a.rs"],"key":"auth:session"}
```

| field | required | rule |
|---|---|---|
| `v` | yes | `1` |
| `id` | yes | writers emit a [ULID](https://github.com/ulid/spec); readers accept any unique string |
| `ts` | yes | RFC 3339 UTC, always with millis (`2026-09-25T10:00:00.123Z`) — recency compares run at ms precision, so a row filed just before a session start never reads as newer. Readers accept with or without millis. For humans — order comes from `id`, never `ts` |
| `by` | yes | writer id |
| `kind` | yes | `decision` · `issue` · `note`, or a kind listed in `config.toml` `kinds = [...]` |
| `text` | yes | non-empty, written to stand alone |
| `files` | yes, ≥ 1 | stable references: repo-relative paths (`src/a.rs` — never `./`, `..`, absolute or `\`), or `scheme:ref` anchors (`issue:#12`, `doc:pricing`; scheme ≥ 2 chars `[a-z0-9+.-]` starting with a letter, ref non-empty and opaque) |
| `key` | no | `:`-separated segments of `[a-z0-9._-]+`, ≤ 64 chars, e.g. `auth:session:timeout` |
| `to` | no | who has to answer, e.g. `ploy` — stored lowercase; an `issue --to <who>` lists in full at the session start of that reader — or of every session of the agent client named `<who>` (`opencode`, `codex`, `claude`) — everyone else only counts it |
| `title` | no | ≤ ~15-word headline lists show; the body stays in `text` and is pulled by id (`find <id>`, `--full`) — set it when `text` tops ~60 words; rows without one list their first sentence (~20 words + `…`) |
| `urgent` | no | orderable number on issues, lower = more urgent — absent = not urgent; `--urgent` files at the back, `--urgent-before <id>` just above that row, `fael bump` moves it later |
| `revisit` | no | a date `YYYY-MM`/`YYYY-MM-DD` or free text (`mdl lands`) — a date ≤ today lists the row first at kickoff whatever its files; free text only counts (`fael find --revisit` lists it); `find --kind issue` lists a row whose revisit is free text or a date ahead after the ready ones, shown `(waiting: …)` |
| `supersedes` | no | id of an older row this one replaces |
| `bumps` | no | on a bump event only (§Bump): the id of the row it moves |
| `held` | no | the branch working on an issue, set by `fael claim` and carried through later bumps — a claim is race-safe (one winner per clone) but gates only the claim, never an edit; `find` shows `(held @<branch>)` |
| `fh` | no | file hashes at write time: an object mapping each real file in `files` to the first 12 hex of its git blob id (`sha1("blob <len>\0" + bytes)`), with CRLF read as LF in a text file so the same file hashes the same on a CRLF checkout and an LF one; a binary file (a NUL in its first 8000 bytes) is hashed raw. The file is streamed in 64 KiB chunks, never read whole. It equals `git hash-object` when the repo stores LF and the file has no lone `\r` and no NUL past byte 8000 (git's own CRLF rule reads either as binary and leaves the file as is; fael normalises it — both sides of a comparison use the same rule, so it never reads as a change). Files are capped at 8 per row and 16 MiB each, in `files` order; anchors, globs, directories and missing files get no key, silently; a real file left out for size, an unreadable one or one past the 8th is named in one info line in the `add`/`bump` receipt (`fael: not stamped (no file-hash verdict at push): <path> (over 16 MiB), …`) — never a reject, and the row is still filed. `fael add` and a bare `fael bump` restamp it; `fael claim` and a bump that only moves the row (`--to`, `--urgent`, `--revisit`) carry the old map forward — neither is a check. It lets fael tell "this file changed since the row was written" without a git spawn (`docs/architecture.md`) |
| `client` `model` `session` `branch` `sha` | no | filled in by tools, never by the agent · `session` is the hook session id that filed the row (the transcript UUID, never a path), absent outside a hook session |

**Close row** — in `<writer>/<yyyy-mm>.close.jsonl`. Any kind can be closed; a close never edits the row.

```json
{"v":1,"id":"01J9…","ts":"…","by":"delamind-3f9a","ref":"01J8ZQ…","text":"fixed in 4139925"}
```

**Compact files** carry the close inside the row instead: `"closed":{"id","ts","by","text"}`.

**Alias row** — in `<writer>/<yyyy-mm>.jsonl` (what `fael mv <old> <new>` appends when git
can't see a move: anchors, uncommitted rewrites, repos without git):

```json
{"v":1,"id":"01J8…","ts":"…","by":"delamind-3f9a","text":"doc:pricing → doc:pricing-2027","moved":{"from":"doc:pricing","to":"doc:pricing-2027"}}
```

Carries no `kind` and no `files` — it only says "what was `from` is now `to`". Readers that
don't know `moved` must skip the row entirely (never show it, never count it as a legacy
row without `files`); readers that do expand queries through it like a git rename. Never
edited or deleted — a wrong alias is fixed by moving back, not by rewriting.

### Plan keys

Planning rides the existing `key` and `files` fields — no new field, no format bump:

- `plan:<name>` — the anchor a `PLAN-<name>.md` path widens a kickoff filter to (plan name
  lowercased; see `plan_anchor`). A decision or issue also lists the real code files it is
  about; a handoff note lists only the anchor and the PLAN path, so reading code never
  re-pushes it.
- `plan:<name>:handoff` — the plan's handoff note. One key per plan: each chunk's note
  supersedes the last by self-heal identity (same kind, key and writer). A chunk run in
  parallel with another open chunk of the same plan (another worktree) writes its note under
  `plan:<name>:chunk-<n>` instead — on the shared key it would supersede the other's handoff.
- `plan:<name>:chunk-<n>` — a row about chunk `n`, the chunk it was filed in. The `n` is a
  plain number.

These are a **fapony convention**, not fael semantics: fael stores and matches them like any
other anchor or key, and never infers from them which plan a session is inside — a session's
intent is not a fact the shared log can answer. The workflow tool that knows the plan asks for
its rows itself (`fael find --key 'plan:<name>:*'`, `fael kickoff PLAN-<name>.md`).

## Writers

**Write contract ≠ read contract.** Writers v1 must follow every rule below; readers must accept anything
that parses (§Readers) — including legacy rows without `files` and rows from newer versions.

A writer must reject a row before writing it when: `files` is empty or not repo-relative · `kind` is not allowed ·
`key` breaks the pattern · the serialised line is over 10 KiB (bytes, not counting `fh`) · it looks like a secret.

To append:
1. take an exclusive lock on `.fael/.lock`
2. refuse if the target month file is ≥ 50 MiB (compact first)
3. if the file does not end in `\n`, prepend `\n` (seals a torn line off)
4. write the whole line plus `\n` in one write
5. release the lock

A line counts as written only once its `\n` is. There is no fsync — git is the durability layer.
Only files nobody appends to any more (past months, compact, `_import`) are ever rewritten, and only with tmp-then-rename.

## Readers

Reading never fails. Take no lock; for every `*.jsonl` under `log/`:

- strip a BOM, accept CRLF, replace invalid UTF-8
- ignore the text after the last `\n` (a write in progress or a torn write)
- skip blank lines and merge-conflict markers (`<<<<<<<` `=======` `|||||||` `>>>>>>>`) — keep the rows on both sides
- skip a line that isn't a JSON object, or whose known fields have the wrong type, and report `file:line`
- **keep every field and kind you don't know**, and write them back unchanged — readers are forward-compatible and lossless
- skip a row with a `moved` object you don't understand — it's an alias carrier, not a result
- a row with no `kind` and no `files` is a carrier, never a result — it moves
  no finding and holds no topic, so lists show nothing for it and counts skip
  it. A legacy row without `files` still carries a `kind` and stays a result;
  close rows ride the `.close.jsonl` stream, not the row stream, and still
  hide the rows they name
- drop duplicate `id`s, keeping the first in path order (a union merge duplicates lines)
- a row without `files` (legacy) is valid to read
- compare `files` after turning `\` into `/` and dropping a leading `./` — legacy rows were not normalised

A row is hidden by default when a close row's `ref` names it, it has a `closed` field, or another row `supersedes` it — minus the edges a restore row reverted (see below).

Closing a row that supersedes others also closes the rows it names — one close row per version. Each is a plain close row, so an older reader hides the whole chain exactly as this rule says; the marker itself is never erased.

## Restore

`fael restore` reverts one supersede edge without rewriting anything: it appends a restore event row carrying `restores` (the superseding row's id — one row supersedes at most one row, so the superseder names the edge), no `kind`, no `files`, and a text naming both ends in full (`<B> restored — supersede by <A> reverted`). Readers hide `superseded()` = all supersede edges minus the reverted ones, so the restored row opens again unless another still-active edge names it. Restoring an already-open row, or an already-reverted edge, writes nothing. After a restore both ends are open at once — the next `add` re-runs the self-heal policy over both, so a same-kind add on their key holds until an agent picks one with `--supersedes`; it never silently re-hides the restored row. The restore is also the rule's label: `doctor` reports per-`decision_source` precision over labeled edges only — edges never restored are not counted, re-adds the healer files alone never label, and pre-verdict edges (no `decision_source`) are skipped. A re-opened row lists with a `(restored)` mark until another edge hides it or it closes.

No `v` bump — the degrade is intended, in two layers. Readers at or after the carrier rule above never list the restore row at all (a carrier, never a result). Readers older than that do list it, but its text reads as information, not garbage; and they keep hiding the restored row (over-hide) until upgraded — a bump could not teach them the subtraction anyway.

## Bump

`fael bump` and `fael claim` move an open row without a new version: they append a bump event row carrying `bumps` (the row's id, which stays its id), no `kind`, no `files`, a text naming the row in full (`<id> bumped`), and a snapshot of the moved fields — `to`, `urgent`, `revisit`, `held`, `fh` (absent = cleared) — plus the writer's `sha`/`branch` stamp. Readers fold events onto their rows in id order, the newest winning: those fields, `ts`, and `sha`/`branch` when the event has them, come from the event; `id`, `by`, `session`, text and files stay the row's. A bump never changes text or files — new content is a new row with `supersedes`. An event naming no known row is ignored. Raw readers that copy rows elsewhere (sync, compact) carry the events as rows and must not fold, so a row never travels with another writer's fields under its id. Bumps written before this rule are supersede versions and keep reading that way.

No `v` bump, the same degrade as restore: readers at or after the carrier rule skip the event and show the row as last written (old routing, old `fh`); older readers list the event's text as information.

## Versioning

`v` is per row, not per repo — one log holds rows of every version, written by old and new CLIs side by side.

- **Log rows are never migrated.** No tool rewrites a written row to a newer `v`: the log is append-only,
  in git, union-merged across branches, and teammates on an older CLI keep writing their version.
  A reader that knows `v2` reads a `v1` row by upcasting it in memory.
- **Adding an optional field is not a bump.** Old readers keep it (§Readers) — `to` and `urgent` came this way.
- **Bump `v` only when an old reader would misread a row**: a field changes meaning or type, or a required field
  is renamed or removed. A new `v` must say how to upcast every older one.
- **Cache files carry their own `v`** and are rebuilt, never migrated: a reader that sees a cache `v` it doesn't
  know deletes the cache and rebuilds it from the log and git.
