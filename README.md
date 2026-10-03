# fael

[![npm](https://img.shields.io/npm/v/@zecalis/fael.svg)](https://www.npmjs.com/package/@zecalis/fael)
[![release](https://img.shields.io/github/v/release/zecalis/fael.svg)](https://github.com/zecalis/fael/releases)
[![CI](https://github.com/zecalis/fael/actions/workflows/ci.yml/badge.svg)](https://github.com/zecalis/fael/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**A shared work ledger for every agent on your repo.** Hand work from one session, agent or person to
the next — a decision, a requirement, an issue assigned to someone, a claim, where a plan stopped —
and it shows up where the next agent is working: on the file it opens, and at session start for what
is routed to it.

One person runs five agents; a team runs fifty — and each of those hands work to sub-agents. They
don't share a chat, so every one starts from zero: it finds the same flaky test, re-asks why that
function looks weird, picks up the issue another agent is already on, and repeats the mistake the
last agent already fixed. Tools that stuff a summary of everything into context before the agent has
said what it's about to do average the one row that mattered away, and a summary of stale notes is
still stale.

fael works the other way round. The agent writes down what it decided or found **in the same
message as its next tool call, at no extra turn**, and when it opens a file it gets **only what was
written about that file**. The file it touches is the question. Work routed to an agent or a person
lists in full at their session start.

What moves through it:

- **Hand-offs.** `fael add note "pricing page: copy approved, layout half-done" --files
  src/pricing.tsx --key pricing:page` — the next session or agent gets it when it opens that file or
  its prompt names the key.
- **Assignments.** `fael add issue "…" --to ploy` — lists in full at the session start of whoever's
  git `user.name` is `ploy` (lowercased). `--to opencode` (or `codex`, `claude`) reaches every
  session of that agent in the repo — Claude hands a review to OpenCode, and what it answers comes
  back as the close line (`fael find --all`). The receipt prints the line to paste to the other
  agent. fael never wakes an agent: it reads the row when it next starts or touches the file.
- **Claims.** `fael claim <id>` — `fael find --kind issue` then shows `(held @<branch>)`, so another
  agent picks something else. Never a lock.
- **Requirements and decisions.** A PM, QA, EM or tech lead writes one once — through their own
  agent or the CLI — and every dev agent that touches that code later gets it, so the next feature
  doesn't forget what the last one agreed.
- **Come-backs.** `--revisit <date|text>` brings a row back when it is due.

How it behaves:

- **Writing costs no extra turn.** The agent runs `fael add` in the same message as its next tool
  call. When a turn ends with no tool call left, it can close its reply with `fael
  decision|issue|note: … [files: …]` lines and fael files them — no turn is stopped or
  re-prompted. Want the old enforcement (no row, no end of turn)? Opt in with `[capture] block = true`.
- **Memory finds them.** When an agent reads a file, the decisions and open bugs about *that file*
  come attached — nobody has to remember to search. Sub-agents too: one starts with an empty
  context and only a short brief, and the file it opens brings the memory the brief left out.
- **No spam in context.** Rows are pushed per file, once per context window (a sub-agent, or a
  session after compaction, is told again — it no longer has them), and cut to a token budget
  (800 by default) — not a notes dump. Anything else the agent asks for itself, through MCP.
  `fael stats` shows exactly what fael has put into context; `fael report` puts it on one
  offline page you can hand to your lead.
- **It retires what the code outgrew.** Git owns what changed; fael keeps only what git can't say.
  When an agent edits a file, it is asked to close a row the code now says, or re-file one the
  code contradicts — in the same message. `fael doctor` lists the rest: open rows whose files took
  10+ commits since they were written, to check against the code.
- **It follows the code.** Rename a file and its rows follow it (`git log -M`). Split one into
  several and `fael mv old new` points the rows at the new files.
- **It lives in your git, out of your branches.** Rows are plain JSONL in the clone's `.git/fael/`,
  shared at once by every worktree, so PRs never carry log lines to conflict on. `fael sync` carries
  them to teammates through your own remote (`refs/fael/*`). No server, no account.

Works with **Claude Code, Codex and OpenCode**, and any MCP host. One small binary; a hook call takes about 10 ms on a repo with a thousand rows (a session start, once, about 100 ms).

## What you get

| Without fael | With fael |
|---|---|
| Each session starts from zero | Session opens with what the last one left: open bugs, recent decisions |
| "Why is it like this?" — ask again, guess again | The reason sits next to the file, from the agent that made the call |
| Agent notices a bug mid-task, then forgets it | It's filed on the spot, and shown to whoever touches that file next |
| Two agents in parallel worktrees hit the same problem | The first files it; the second gets it when it opens the file — the same hour, before any commit (`fael stats` → `across agents` counts how often) |
| A sub-agent finds something and its summary drops it | Its `fael issue: …` line is filed when it stops (Claude Code), and the parent gets it on that file |
| Work is handed over in chat and lost between sessions | A hand-off note, an assigned issue or a claim sits in the ledger — the assignee sees it at session start |
| Two agents pick the same issue | `fael claim <id>`: the second sees `(held @<branch>)` and picks another |
| Knowledge stays in one person's chat history | It's in the clone — teammates and their agents get it on `fael sync` |
| A PM's requirement lives in a ticket the agent never opens | `fael add decision … --files src/pay.rs` — it's in front of the agent the moment it opens the file |

## Does it help? Check your own numbers

fael does not claim to save tokens, turns or time: it cannot see what an agent would have done
without it. It reports what it handed over, so you can judge on your own repo:

```bash
fael stats
```

- `hit N/M rows` per event — rows still in front of the agent when it edited that file. A lower
  bound: a read-only session scores every row a miss.
- `across agents` — rows one agent wrote that another was handed, and how many were written while
  the receiver was working. If you run agents in parallel worktrees, watch this one.
- `retired at touch`, `issues closed`, `handoffs picked up` — rows closed or picked up.

Most useful when several agents (or people) work in one repo and return to the same files over
days. Little to gain from one short session in a repo you will not touch again.
Definitions: [docs/stats.md](docs/stats.md).

## Install

**1. Get the binary** — prebuilt for macOS, Linux and Windows. No Rust needed.

```bash
brew install zecalis/tap/fael    # Homebrew (macOS / Linux)
curl -LsSf https://github.com/zecalis/fael/releases/latest/download/fael-installer.sh | sh
```

Windows (PowerShell):

```powershell
irm https://github.com/zecalis/fael/releases/latest/download/fael-installer.ps1 | iex
```

Or through npm: `npm i -g @zecalis/fael`. It works, but every `fael` call — including each hook —
starts Node first, so the options above are faster.

**2. Connect your agents** — once per machine.

```bash
fael install              # finds Claude Code, Codex and OpenCode; adds hooks, MCP server and a skill
fael upgrade              # show what is out of date, ask, then update (alias: update)
fael install --dry-run    # show what would change, write nothing
```

`fael` must be on your `PATH` — the hooks call it by name, so upgrades never leave them pointing at
an old path. That's also why `npx @zecalis/fael install` is refused: npx keeps the binary in a
throwaway cache. Install it globally first.

**3. Work as usual.** Rows land in `.git/fael/` — nothing to commit. To share them or back them up:

```bash
git config fael.remote <url>   # any git remote you can push to — origin works, a private one if the repo is public
fael sync                      # push your rows, pull everyone else's (the Stop hook also runs it once per session)
```

Rather review memory in PRs? `store = "tracked"` in `.fael/config.toml` also writes the rows to
`.fael/log/` in the tree, to commit like code. Repos that already have a `.fael/log/` keep that mode;
`fael migrate local` moves one over ([docs/integrate.md](docs/integrate.md#moving-a-tracked-repo-to-local)).

## How it works

```
agent reads src/pay.rs   →  fael attaches: "[bug] refund rounds down on JPY → src/pay.rs"
agent fixes it, replies  →  "…done. fael decision: refunds round half-up, per finance [files: src/pay.rs]"
fael                     →  files that line as a row — no extra turn, nothing blocked
fael sync                →  the next agent, on any machine, sees it when it opens src/pay.rs
```

Agents use the `fael` MCP server (`find`, `add`, `close`). You can use the same log from the shell:

```bash
fael kickoff                                             # what this session should know
fael find --files src/pay.rs                             # everything about one file
fael add bug "refund rounds down on JPY" --files src/pay.rs
fael add issue "count from order date or ship date?" --to finance --files src/pay.rs
fael close <id> "fixed in 4f2a91c"
fael mv src/pay.rs src/pay/refund.rs                     # a split git can't see — rows follow
fael doctor                                              # check the setup (e.g. no fael.remote to back rows up)
```

`fael` with no arguments lists every command.

**Coming from fapony?** `fael install --replace-fapony` switches the hooks over, and
`fael import .fapony/.memory` brings the old log with it — no row lost.

## Links

- **Homebrew tap:** [zecalis/homebrew-tap](https://github.com/zecalis/homebrew-tap)
- **npm:** [@zecalis/fael](https://www.npmjs.com/package/@zecalis/fael)
- **Releases & changelog:** [GitHub Releases](https://github.com/zecalis/fael/releases)
- **Bugs & ideas:** [Issues](https://github.com/zecalis/fael/issues)
- **Docs:** [architecture](docs/architecture.md) · [log format](docs/format.md) (read/write it without fael) · [integrate another agent](docs/integrate.md)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [Code of Conduct](CODE_OF_CONDUCT.md)

## License

[MIT](LICENSE) © zecalis
