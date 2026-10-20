# Fael — agent instructions

## North star and product invariant

North star: fael is the lifecycle ledger for agent work — the hand-off where work stopped, the
decision and why, the open issue, the claim, the note the next session needs, what the user asked
for. Its primary user is the agent; the goal is wide adoption by other teams. Git owns what
changed; fael keeps what git and the code cannot say. The agent discovers, judges and records;
fael captures, carries and surfaces — never infers, never judges, never fixes for it.
Memory remembers; fael closes (decision 01M5ABTD, key product:mission).

The mechanism is the team's shared work ledger — context and hand-offs (decisions, requirements,
assigned issues, claims, where work stopped) that pass between sessions and agents — not the
agent's memory, not a personal notebook, not a generic AI memory store, and not a process or git
guard. Value is work context (`task (key) → decision → evidence → outcome → closure`) in front
of the right agent, once — never more rows. Rounds and tokens are a cost to keep low, never a promise.
Today the log is per machine: shared by every worktree and agent on one machine, moved across
machines by `fael import`. Team sync over the git remote (`refs/fael/*`, no server) is planned,
not shipped.

Core (public): add, find, close, bump, claim, doctor, stats, hooks, MCP. fael crew is the owner's
layer: plan, chunk, board (`fael plan|chunk|board`, hidden from default help; plan `cli-tiers`).
Core never depends on crew; crew reads core through `fael board --json`. Say "fael crew", never
bare "crew". The crew repo is private: never link it from public docs.

Decision `product:mission` (01M5ABTD): a core change ships when it fixes a bug, cuts noise,
removes code, strengthens lifecycle or stats, or brings work in from the team's other systems
(intake, 01M4TJTW). A crew change ships when it serves the owner's plan → chunk → agent → ship
loop and adds nothing to core default help. The owner's SwiftUI app
reads `fael board --json` and starts an agent only on the owner's click or from a queue
the owner approved; dev chunks are started by the owner, by the project's lead after the
owner approves its set, a lone one as a set of 1 (decision 01M4Z88Y; the lead is part of fael crew, 01M4SQ9B);
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

A plan is sections plus chunks in crew.db — no plan file. Read it with
`fael plan export <plan>` (`--section <key>` for one); change a section through the bill
(`fael plan export <plan> --json`, edit `sections`, `fael plan apply --dry-run`, then apply).
Draft or re-cut plans with the `fael-plan` skill (fael crew).

`fael chunk start <uid>` prints the brief and the chunk rules (how to close, wait, checkpoint)
— don't restate them here. The `fael chunk done <uid> "<handoff>"` text is the handoff: the
board shows it and the brief of every chunk `after` it prints it, so it needs no ledger row.
A word for a chunk another agent will start goes in `fael chunk note <uid> "<text>"`, printed
in that chunk's next brief; never `chunk edit` (it clears the owner's approval).

A chunk that waits on events (a draft titled with its gate, or `wait --on data`) runs only
once the count its brief names meets its locked bar; its result goes in the decision that
chunk names. Don't file a ledger row for a count that isn't ready.

## New flags

`fael --help` is core-only; workflow stays parseable but is reached via the `fael-plan` skill (fael crew), `fael chunk start` briefs, Plan Workflow above and the board app.
A new flag/command ships with (1) the issue id it fixes, (2) the friction it cuts, (3) the `fael stats` by-flag line showing its use (decision 01M59HQJ).
A new workflow flag is born hidden (outside default help) and named by the skill/brief that sends the agent to it.
Tier it under `product:cli-tiers-table` (`fael find --key product:cli-tiers-table`); removal follows `fael plan export cli-tiers --section 4` — link, never copy, either list.

## Git

Follow the repo-wide Git workflow. `push pr` is `git fetch && git rebase origin/main` →
`/fael-review` → `git ship` → report the PR number. `git ship` runs the clean, merged-PR and
rebase checks, then `scripts/gate.sh` (fmt, clippy with `-D warnings` on host and Windows
target, `file-size.sh`, tests — one ✓/✗ line each, output only for a failing step), pushes
and opens or edits the PR. Repo rules for it live in `.fael/worktree.json` (`gate`,
`ship.regen`). Run `scripts/gate.sh` alone to validate without shipping.
While iterating, run `cargo check -p <crate> --message-format=short` and
`cargo nextest run -p <crate> <filter> --profile gate` (profile in `.config/nextest.toml`)
instead of the full gate; run `scripts/gate.sh` once before ship.

Windows CI runs on `main` and on wave PRs (head `plan/**`), not on other PRs. A PR that touches paths, golden files,
process spawning, env vars or time-dependent tests runs it and reports its result before
review — `gh workflow run ci.yml --ref <branch>` after `git ship`, then
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
