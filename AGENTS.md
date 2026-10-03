# Fael — agent instructions

## Product invariant

Fael is the team's shared work ledger — context and hand-offs (decisions, requirements,
assigned issues, claims, where a plan stopped) that pass between sessions and agents — not a
personal notebook, not a generic AI memory store, and not a process or git guard. Value is work context
(`task (key) → decision → evidence → outcome → closure`) in front of the right agent, once,
at the file it touches — never more rows. Rounds and tokens are a cost to keep low, never a promise.

Ship a feature only if it preserves/repairs context, retrieves the right context, cuts
repeated agent work, or improves team reuse — and never starts an agent turn or
re-prompts on idle, adds no daemon,
never guesses, never polices the developer's process (worktree, branch, git flow).
Assigning and claiming inform. A claim is race-safe (of two agents, one wins) but only gates the claim — `--force` takes it over, no edit is ever blocked.
Claims about value (README, docs, PRs) say only what `fael stats` shows — what fael handed over
and how often it was in front of the agent at an edit (`value.by_event`, `value.cross_agent`).
Never "saves tokens/rounds/time": fael cannot see the counterfactual.
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
supersedes the last — except a chunk run in parallel with another open chunk (another
worktree): its note goes under `--key plan:<name>:chunk-<n>` so it can't wipe that
chunk's handoff. Start the next session with `fael kickoff <path/to/PLAN-x.md>`;
don't carry the old transcript forward.

## Git

Follow the repo-wide Git workflow. Validate before `push pr`:

```text
cargo fmt --all --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets --locked
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc
scripts/file-size.sh
tests
```

CI lint runs clippy with `-D warnings`; bare `cargo clippy` exits 0 on warnings, so a
local pass without the flag proves nothing. Check each command by its own exit code.

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
