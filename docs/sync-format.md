# fael sync format v1

One page, public. Any tool that follows it can carry a fael journal between
clones without fael. Row bytes are defined by [`format.md`](format.md) §Row;
this page only says how those same bytes travel.
Reference implementation: `fael-core::sync` (pure) + `fael sync` (Git transport).

## Contract

Four principles. They are locked from chunk 1 and never redefined by a transport:

1. *Local `.fael/log` is the canonical journal representation.*
2. *A writer owns one journal.*
3. *A transport may store/transfer the journal, but must not introduce a second row format.*
4. *Git refs are a transport namespace, not the Fael data model.*

"Writer" is [`format.md`](format.md) §Layout `<writer>` (a logical author, not a
machine). "Row format" is [`format.md`](format.md) §Row: a transport carries
every row's content, unknown fields included, and writes it in the one
canonical rendering (`Row::to_line`) — the bytes `fael add` writes. A line an
older fael or a hand edit wrote with another key order or spacing travels in
the canonical form; the row is the same row. A future cloud transport stores
the same rows behind HTTP instead of Git — its database is an index over this
format, never a second format.

## Git mapping (transport #1)

Local journal path → ref tree path, per writer:

```
.fael/log/<writer>/<yyyy-mm>.jsonl         →  refs/fael/<repo-id>/<writer>/<yyyy-mm>.jsonl
.fael/log/<writer>/<yyyy-mm>.close.jsonl   →  refs/fael/<repo-id>/<writer>/<yyyy-mm>.close.jsonl
```

- The tree is **flat**: `meta.json` plus `<yyyy-mm>.jsonl` / `<yyyy-mm>.close.jsonl`
  at the ref root. There is no `<writer>/` level inside the tree — the ref
  already is the writer namespace.
- Every row the reader sees travels, and the tree is the exact inverse of the
  reader's split: add rows go to `<yyyy-mm>.jsonl`, close rows to
  `<yyyy-mm>.close.jsonl`, month from `ts` (UTC). Bytes are the row's
  `format.md` §Row rendering, one JSON object per line, LF. This holds for rows
  stored in `compact.*` or `_import/*` locally too — they are re-split by
  their own `ts` month, and those file *names* never appear in the tree.
  `quarantine/` (lines doctor removed) and `cache/` (not `.jsonl`) are not
  rows and are never read.
- Readers apply [`format.md`](format.md) §Readers to every fetched file:
  torn tail ignored, bad lines skipped with `file:line`, unknown fields kept,
  duplicate `id`s dropped (first in path order wins).

Example tree at `refs/fael/<repo-id>/alice-3f9a`:

```
refs/fael/abc123…/alice-3f9a
├── meta.json
├── purged.txt          (only once the writer purged a row)
├── 2026-09.jsonl
└── 2026-09.close.jsonl
```

`purged.txt` is the tombstone list: one row id per line, sorted. A push cannot
delete a row from a remote (ff-only, no force), so `fael purge <id>` records
the id and the next sync carries it here. Every reader skips a tombstoned id
on ingest — and the close events that name it — and drops it from its own ref
on its next push. History still holds the row, and a copy already in a
teammate's journal stays there until they purge it. The file is optional; a
reader that does not know it just keeps reading rows.

### Ref scheme

```
refs/fael/<repo-id>/<writer>
```

- `<repo-id>` — the workspace/repository identity, identical for every clone
  and every branch of the same repo (see below). Two different repos syncing
  to one remote never share a ref prefix.
- `<writer>` — the writer id, same string as the local `<writer>` directory.
  One writer owns exactly one ref per repo-id; one ref is written by exactly
  one writer.

Ref-name safety: a writer id that is not valid in a Git ref is rejected before
any network call (`fael sync` errors, nothing is pushed). The mapping
writer-id → ref path component is identity — no escaping scheme, no second
naming.

### repo-id

- Computed from `git rev-list --max-parents=0 --branches --remotes=origin`, taking the minimum SHA.
  Only branches and origin's branches count — what every clone shares. A
  `stash -u`, `git notes`, another remote or sync's own parentless commits
  never shift the id. Every clone of the same repo derives the same id,
  including repos with two root commits. A local-only orphan branch or a
  single-branch clone can still differ; pin with `git config fael.repoid`.
- Cached at the first sync in `git config fael.repoid` so the value never
  moves afterwards.
- A shallow clone (history truncated, root set incomplete) errors clearly
  instead of deriving a wrong id.

`repo-id` is a workspace identity, not a Git identity in the long term: the
cloud transport keys journals by the same string without Git.

## meta.json

One per ref, at the tree root:

```json
{"format_version": 1, "repo_id": "abc123…", "origin": "https://github.com/acme/my-project", "name": "my-project"}
```

| field | required | rule |
|---|---|---|
| `format_version` | yes | `1`. Bump only when a field is removed, renamed, retyped, or changes meaning. Adding an optional field is **not** a bump (same rule as row schema) — the addition is recorded here in this doc. |
| `repo_id` | yes | must equal the `<repo-id>` in the ref name. A mismatch is rejected on ingest. |
| `origin` | yes | the source repo's origin URL at the time of the first push (provenance label, never used for auth or routing), with any `user:password@` stripped. Empty string when unknown. |
| `name` | yes | human short name of the repo (display label). Empty string when unknown. |

`meta.json` is written by the **first** push of a ref only. A later push, from
another clone or worktree of the same writer, keeps the label the ref already
has — otherwise checkouts named differently would rewrite it on every
alternating sync.

There is deliberately **no `writer` field**: the ref already is the writer
identity; duplicating it invites divergence.

## Push semantics

- A writer pushes **only its own ref** (`refs/fael/<repo-id>/<writer>`).
  Each ref is therefore append-only by construction; two writers never
  contend on one ref, so concurrent pushes do not conflict.
- What a ref carries: the rows this writer filed, the rows an import put in
  the clone, and rows filed under an earlier writer id of this clone (a
  changed `user.email`/`user.name`) when that id owns no ref and no other ref
  already carries them. A sync ingests every other ref first, so a clone that
  merely received such rows never pushes them a second time.
- Fast-forward only, never force. If the remote ref moved during a sync
  (same writer pushing from two machines), the pusher re-fetches, re-unions
  ingests what that brought and retries once; a second failure is an error
  telling the user to run `fael sync` again. `synced: pushed N` counts the
  rows this push added to the ref, not the size of the journal.
- The working tree and the PR diff never change: a sync touches only refs
  under `refs/fael/`, never a checked-out branch, never a working-tree file.
- Ingest is union by `id`: fetched rows missing locally are appended to the
  local journal through the normal write path (lock/seal rules of
  [`format.md`](format.md) §Writers hold) — the journal only, never a
  `.fael/log/` working-tree file, whatever `store` says; local rows missing
  remotely are pushed. Dedupe by `id` is the only de-duplication mechanism — there is no
  merge and no conflict resolution on the remote side.
- One writer's bad ref never stops the rest: another writer's ref whose
  `meta.json` does not parse or validate (a newer `format_version`) or whose
  fetch fails is skipped with one `fael: skipped <ref> — …` line, and a row
  the local write refuses (`by`/`ts` not usable as a path) is skipped the
  same way; every other row still lands.
- One sync at a time per clone: the journal is shared by every worktree, so a
  second `fael sync` while one runs exits with `another sync is running`
  instead of appending the same ingested rows twice.
- Empty journal + no remote ref is `nothing to sync`: no ref is created.
- Remote resolution: `--remote <url>` flag wins, else `git config fael.remote`.
  Neither is set → exit 1 with
  `fael: no fael.remote — set it with: git config fael.remote <url>`.
  `fael.remote` lives in `.git/config` (per machine, never committed), and
  auth is the user's own Git credential. A remote is any Git URL (private
  repo, Gitea/Forgejo, bare path on a NAS) — fael does not care who hosts it.
- `store = local` + destination is `origin` pushes no less, but prints one
  warning line (the ref is fetchable by anyone with read access even though
  no UI shows it):
  `fael: fael ref is publicly fetchable from origin — point fael.remote at a private remote if this repo is public`.
  Any other remote, or any other store value, prints nothing.

## Security

fael has no permission system of its own — a Git server's read/write rights
*are* the permission system. Repos whose memory must not leave the team never
set `fael.remote` to a shared remote. `store = local` keeps rows out of the
source repo's tree, but a `fael.remote` pointing at the public origin is
still public — hence the one-line warning above.

## What this page does not cover

- No daemon, no timer, no auto-sync per `add` — the only batch boundary is
  the session.
- No cross-repo reads (`fael find --remote …`).
- No cloud server or UI — that is separate work reusing this exact
  contract (`Meta` / month split / union / validate) over HTTP.
- No plan files — fael never reads `PLAN-*.md` or `.fapony/`; `plan:<name>`
  keys are an opaque user convention (see [`format.md`](format.md) §Row), and
  sync carries rows, never plan documents.
- No file contents — sync carries rows only, never the files they name. A path
  in `files[]` may dangle where the file is absent (a personal plan,
  uncommitted work, a non-dev document); the row text must stand alone anyway.
  A team that needs shared documents resolvable by anchor needs a document
  store on the server — that is cloud scope, not this contract.
