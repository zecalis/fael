# Fael — agent instructions

## Product invariant

Fael maintains the team's shared memory of work — not a personal notebook, not a
generic AI memory store, and not a process or git guard. Value is work context
(`task (key) → decision → evidence → outcome → closure`) made reusable with **fewer
agent rounds**, never more rows.

Ship a feature only if it preserves/repairs context, retrieves the right context, cuts
repeated agent work, or improves team reuse — and never starts an agent turn or
re-prompts on idle (the opt-in `[capture] block = true` mode aside), adds no daemon,
never guesses, never polices the developer's process (worktree, branch, git flow).
Full statement: `docs/architecture.md` §0.

## Memory — `.fael/log/`

`.fael/log/` is fael's local, append-only memory, excluded from git. The old
`.fapony/.memory` log was imported 2026-09-25; never write new rows there.

**Log as you work. Do not wait to be asked.** Use Fael MCP when available, else the CLI:

```text
fael kickoff <file|PLAN>
fael add decision|issue|note "..." --files <files>
fael find [text] [--files ...] [--key ...]
fael close <id> "..."
```

What to record:

* **issue** — broken, inconsistent, or likely to break. Add it immediately, even
  mid-task; close it when fixed.
* **decision** — an agreed/locked choice not already in code or the plan, including why.
* **note** — what the next session needs, especially unfinished work or handoff state.

Rules: every row needs `--files` · write so it stands without this chat · verify ids with
`fael find`, never from memory · before ending a session that changed files, add a row ·
read `docs/architecture.md` §1 before deciding where code belongs.

## Plan Workflow

How many chunks per session/PR, when to stop and how to close a chunk: `fapony plan <PLAN>`
prints the rules (single source, fapony's `chunkRules()`) — don't restate them here.

Fael's part: each chunk's handoff note goes on the plan, never a code path (every read of
that file would re-push it): `--files plan:<name>,<path/to/PLAN-<name>.md> --key
plan:<name>:handoff`, `<name>` lowercased. One key per plan, so each chunk's note
supersedes the last. Start the next session with `fael kickoff <path/to/PLAN-x.md>`;
don't carry the old transcript forward.

## Git

Follow the repo-wide Git workflow. Validate before `push pr`:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc
scripts/file-size.sh
tests
```

Only English in commits, PR titles/bodies, and review replies. Never merge PRs — the
user reviews and merges them.

## Multi-agent Safety

* Run `git status` first.
* Don't touch uncommitted changes you didn't make; ask first.
* No `git reset --hard`, `git checkout -- .`, `git clean -fd`, or bare `git stash`
  without asking.
* Each agent uses its own permanent worktree; never switch branches in another agent's.

## File Size

CI enforces `.rs` ≤400 lines and functions ≤100. Splitting guidance:
`CONTRIBUTING.md` → **File size: no god files**.
