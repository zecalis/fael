# fael multi-agent preset

One setup for teams of agents, then no hand-tuning:

```sh
git clone <repo> && cd <repo>
fael install --preset multi-agent
```

What it writes is a configuration bundle of c1-c6 primitives only — the
repo's `.fael/config.toml` with every value pinned to the default
(`fael-core/src/config.rs`, written by `fael/src/install/preset.rs`). No
hidden multi-agent code path, no new policy: default semantics identical.
An existing config is left alone; deleting lines you never changed is safe.

## The 5-workflow smoke

A fresh clone with the preset passes these with zero config edits (locked by
`fael/tests/top/preset.rs` for the preset itself and `fael/tests/top/preset_matrix.rs`
for the matrix — final ledger and emitted pushes asserted, not just exit 0):

1. **fresh start** — `fael kickoff` prints the session brief on an empty repo.
2. **add + close** — `fael add issue … --files src/a.rs`, then
   `fael close <id> "<cause> → <fix>; guard \`<test>\`"` retires it.
3. **duplicate / retry** — the same `add` twice returns the same id and
   writes zero new rows; a `close` retry names the existing close id.
4. **supersede** — `fael add issue … --supersedes <id>` files the successor
   and retires the old row once; a retry names the survivor.
5. **doctor** — `fael doctor` stays green; `--fix` closes only what it can
   prove (`fael/src/maintain.rs`, `fael-core/src/doctor.rs`).

Two wider guarantees ride on the same pinned budgets, proved elsewhere:

- **resume** — `chunk start <uid>` twice returns the same run, the second
  print short (`fael-crew/src/start.rs`).
- **concurrent handoff + cross-session** — the push dedupe key is
  `(session_id, row_id)` only: three edits in one session push a row once, a
  restart inside the session stays silent, and a second session still gets
  the row (`fael/tests/hook/push_once.rs`, `fael/src/hook/say.rs`).
- **wait → resume** — a chunk gated on data or the owner resumes on its own
  bar, never on a guess.
