# Fael — agent instructions

## North star and product invariant

North star: the engine behind plan work — every chunk's brief, area (dev|marketing), size,
model hint and state in `.git/fael/fael.db`, started, waited on and finished through
`fael chunk start|wait|done` — plus the ledger: the hand-off where a chunk stopped, the decision
and why, the open issue, the note the next session needs, what the user asked for. Git owns what
changed; fael keeps what git and the code cannot say. The agent discovers, judges and records;
fael captures, carries and surfaces — never infers, never judges, never fixes for it.

The mechanism is the team's shared work ledger — context and hand-offs (decisions, requirements,
assigned issues, claims, where a plan stopped) that pass between sessions and agents — not the
agent's memory, not a personal notebook, not a generic AI memory store, and not a process or git
guard. Value is work context (`task (key) → decision → evidence → outcome → closure`) in front
of the right agent, once — never more rows. Rounds and tokens are a cost to keep low, never a promise.

Decision `product:mission` (plan `PLAN-fael-board`): a change ships when it fixes a bug, cuts
noise, removes code, or serves the plan → chunk → agent → ship loop. The owner's SwiftUI app
reads `fael board --json` and starts an agent only on the owner's click or from a queue
the owner approved; dev chunks are started by the owner or by a plan's lead agent, after
the owner approves its wave (decision 01M4QVH4; the lead lives in zecalis/fael-crew, 01M4SQ9B), and the app only shows them;
the owner merges every PR; fael itself
never starts an agent turn or re-prompts on idle,
adds no daemon, never guesses, never polices the developer's process (worktree, branch, git flow).
Assigning and claiming inform. A claim is race-safe (of two agents, one wins) but only gates the claim — `--force` takes it over, no edit is ever blocked.
Claims about value (README, docs, PRs) say only what `fael stats` shows — what fael handed over
and how often it was in front of the agent at an edit (`value.by_event`, `value.cross_agent`).
Never "saves tokens/rounds/time": fael cannot see the counterfactual.
Full statement: `docs/architecture.md` §0.

## Push lines — said once, silent when nothing is earned

Every line a hook puts in front of the agent goes through the `Outbox` in `fael/src/hook/say.rs`: its `Kind` and the one `policy()` match decide how it is said once per session and whether it must offer a command.
A new kind gets a fixture in `fael/src/hook/say_contract.rs` (silent when empty, never twice in a session), and a change to what the agent sees re-blesses `fael/tests/hook/replay.rs` (`FAEL_BLESS=1`) with the diff in the PR.

## Memory — `.fael/log/`

`.fael/log/` is fael's local, append-only memory, excluded from git.

**Log as you work. Do not wait to be asked.** Use Fael MCP when available, else the CLI:

```text
fael kickoff [file]
fael add decision|issue|note "..." --title "<≤15-word headline>" --files <files>
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

A plan is sections plus chunks in fael.db — no plan file. Read it with
`fael plan export <plan>` (`--section <key>` for one); change a section through the bill
(`fael plan export <plan> --json`, edit `sections`, `fael plan apply --dry-run`, then apply).
Draft or re-cut plans with the `fael-plan` skill.

`fael chunk start <uid>` prints the brief and the chunk rules (how to close, wait, checkpoint)
— don't restate them here. The `fael chunk done <uid> "<handoff>"` text is the handoff: the
board shows it and the brief of every chunk `after` it prints it, so it needs no ledger row.
A word for a chunk another agent will start goes in `fael chunk note <uid> "<text>"`, printed
in that chunk's next brief; never `chunk edit` (it clears the owner's approval).

A chunk that waits on events (a draft titled with its gate, or `wait --on data`): `scripts/checkpoints.py` prints every such
gate's count against its locked bar. Run the chunk only when its line reads `ready`; its
result goes in the decision that chunk names. Each run that moves a number appends to
`~/fael-onoff-results/checkpoints.jsonl` (the trend, gold included); don't file a ledger
row for a count that isn't ready.

## Git

Follow the repo-wide Git workflow. Validate before `push pr`:

```text
cargo fmt --all --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets --locked
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc --no-default-features
scripts/file-size.sh
tests
```

CI lint runs clippy with `-D warnings`; bare `cargo clippy` exits 0 on warnings, so a
local pass without the flag proves nothing. Check each command by its own exit code.

Windows CI runs on `main` and on wave PRs (head `plan/**`), not on other PRs. A PR that touches paths, golden files,
process spawning, env vars or time-dependent tests runs it and reports its result before
review — `gh workflow run ci.yml --ref <branch>` after `push pr`, then
`gh run watch <id> --exit-status` — and says pass, or fail with the failing test, in the
PR body and the chunk's done text.

Only English in commits, PR titles/bodies, and review replies. Never merge PRs — the
user reviews and merges them.

PR titles follow Conventional Commits — `type(scope): summary`, type one of
feat|fix|docs|refactor|perf|test|ci|chore|build|revert — the squash merge makes the title
the commit subject on `main`. CI's `pr-title` check fails any other title.

## Multi-agent Safety

* Run `git status` first.
* Don't touch uncommitted changes you didn't make; ask first.
* No `git reset --hard`, `git checkout -- .`, `git clean -fd`, or bare `git stash`
  without asking.
* Each agent uses its own permanent worktree; never switch branches in another agent's.

## File Size

CI enforces `.rs` ≤400 lines and functions ≤100. Splitting guidance:
`CONTRIBUTING.md` → **File size: no god files**.
