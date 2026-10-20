# fael

[![npm](https://img.shields.io/npm/v/@zecalis/fael.svg)](https://www.npmjs.com/package/@zecalis/fael)
[![release](https://img.shields.io/github/v/release/zecalis/fael.svg)](https://github.com/zecalis/fael/releases)
[![CI](https://github.com/zecalis/fael/actions/workflows/ci.yml/badge.svg)](https://github.com/zecalis/fael/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**The continuity ledger for agent work — with proof.** Every session starts from zero: no
session shares another's chat. fael keeps what passes between them — where the work stopped,
what was decided and why, the issue still open, what the user asked for — filed where the code
is touched, handed back where it is touched, and retired once the code absorbs it. `fael stats`
shows what was handed over and what got acted on; fael never claims what it can't measure.

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
  agent and, for an agent, the command that starts it on the row (`opencode run "…"`) — fael
  prints it, you or your agent runs it, in the worktree you chose. fael never wakes an agent:
  it reads the row when it next starts or touches the file.
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
  re-prompted.
- **Memory finds them.** When an agent edits a file, the decisions and open bugs about *that file*
  come attached — nobody has to remember to search. Reading or searching brings nothing: the
  rows wait for the edit. Sub-agents too: one starts with an empty context and only a short
  brief, and the file it edits brings the memory the brief left out.
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
  shared at once by every worktree and every agent on the machine, so PRs never carry log lines to
  conflict on. No server, no account. Across machines, `fael import` moves a log; git sync for
  dev teams (`refs/fael/*`, never on code branches) is planned, not shipped.

Works with **Claude Code, Codex and OpenCode**, and any MCP host. One small binary; a hook call takes about 10 ms on a repo with a thousand rows (a session start, once, about 100 ms).

## What you get

| Without fael | With fael |
|---|---|
| Each session starts from zero | Session opens with what was handed to it: the hand-off and the work assigned to you |
| "Why is it like this?" — ask again, guess again | The reason sits next to the file, from the agent that made the call |
| Agent notices a bug mid-task, then forgets it | It's filed on the spot, and shown to whoever touches that file next |
| Two agents in parallel worktrees hit the same problem | The first files it; the second gets it when it opens the file — the same hour, before any commit (`fael stats` → `across agents` counts how often) |
| A sub-agent finds something and its summary drops it | It files the row with `fael add` itself, and the parent gets it on that file |
| Work is handed over in chat and lost between sessions | A hand-off note, an assigned issue or a claim sits in the ledger — the assignee sees it at session start |
| Two agents pick the same issue | `fael claim <id>`: the second sees `(held @<branch>)` and picks another |
| Knowledge stays in one chat's history | It's in the clone — every agent and worktree on the machine gets it |
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

## fael crew, the owner's layer

Everything above is core and needs no plan; core never depends on crew. fael crew (`fael plan|chunk|board`, hidden from default help) is the owner's layer for several agents sharing the work. When one person runs five agents toward one goal, the second
door opens: `fael plan new` cuts the work into chunks, `fael chunk start` hands one to an agent
with its brief and the open rows on its files, `fael chunk done` takes the hand-off back. Chunks
read in the terminal (`fael board --plan <plan>`, `fael plan export <plan>`) and in the board
app. Plans stay out of the way until you make one.

fael crew is a separate binary, `fael-crew`, that `fael` runs for those commands. The installs below
do not include it; from a clone of this repo, `cargo install --path fael-crew`. Without it `add`,
`find`, `close` and the hooks work as before, and `fael plan` says how to install it.

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
fael upgrade              # update fael by the channel it came from, then its hooks and skills; asks first (alias: update)
fael install --dry-run    # show what would change, write nothing
```

fael also checks for a new release at session start, at most once a day, and installs it by the same channel after the tag has been out a day. On Windows it only says a new release is out — run `fael upgrade` yourself. Turn that off with `FAEL_NO_AUTO_UPDATE=1` or `auto_update = false` in `~/.config/fael/config.toml`; `fael upgrade` by hand always works.

`fael` must be on your `PATH` — the hooks call it by name, so upgrades never leave them pointing at
an old path. That's also why `npx @zecalis/fael install` is refused: npx keeps the binary in a
throwaway cache. Install it globally first.

**3. Work as usual.** Rows land in `.git/fael/` — nothing to commit. To bring in another clone's
rows: `fael import <path>` (a log file, a log dir or a repo root — its journal plus any `.fael/log`; importing twice is safe).

A `.fael/log/` already committed in a repo stays read as frozen history; new rows never go there.

## How it works

```
agent reads src/pay.rs   →  fael attaches: "[bug] refund rounds down on JPY"
agent fixes it           →  fael add decision "refunds round half-up, per finance" --files src/pay.rs
fael                     →  files it as a row, in the same message as the next tool call — no extra turn
the next agent           →  in any worktree, sees it when it opens src/pay.rs
```

Agents use the `fael` MCP server (`find`, `add`, `close`). You can use the same log from the shell:

```bash
fael kickoff                                             # what this session should know
fael find --files src/pay.rs                             # everything about one file
fael add bug "refund rounds down on JPY" --files src/pay.rs
fael add issue "count from order date or ship date?" --to finance --files src/pay.rs
fael close <id> "fixed in 4f2a91c"
fael mv src/pay.rs src/pay/refund.rs                     # a split git can't see — rows follow
fael doctor                                              # check the log and the setup
```

`fael` with no arguments lists every command.

## Links

- **Homebrew tap:** [zecalis/homebrew-tap](https://github.com/zecalis/homebrew-tap)
- **npm:** [@zecalis/fael](https://www.npmjs.com/package/@zecalis/fael)
- **Releases & changelog:** [GitHub Releases](https://github.com/zecalis/fael/releases)
- **Bugs & ideas:** [Issues](https://github.com/zecalis/fael/issues)
- **Docs:** [architecture](docs/architecture.md) · [log format](docs/format.md) (read/write it without fael) · [integrate another agent](docs/integrate.md)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [Code of Conduct](CODE_OF_CONDUCT.md)

## License

[MIT](LICENSE) © zecalis
