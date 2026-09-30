# fael

[![npm](https://img.shields.io/npm/v/@zecalis/fael.svg)](https://www.npmjs.com/package/@zecalis/fael)
[![release](https://img.shields.io/github/v/release/zecalis/fael.svg)](https://github.com/zecalis/fael/releases)
[![CI](https://github.com/zecalis/fael/actions/workflows/ci.yml/badge.svg)](https://github.com/zecalis/fael/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**The secretary for every agent on your team.** Every agent on your repo knows what was decided and
what's still open — without re-asking, and without a context dump.

One person runs five agents; a team runs fifty — and each of those hands work to sub-agents. Every
one of them starts from zero: it finds the same flaky test, re-asks why that function looks weird,
and repeats the mistake the last agent already fixed.
Memory tools that try to help stuff a summary of everything into context before the agent has said
what it's about to do — you pay tokens for noise, and the one row that mattered gets averaged away.

fael works the other way round: like a good secretary, it takes the notes nobody else will and hands
each agent **only what matters for the file in front of it**. The agent writes down what it decided
or found **in the same reply, at no extra turn**, and when it opens a file it gets **only what was
written about that file**. The file it touches is the question.

The team isn't only the people who write code. A PM, QA, EM or tech lead writes a requirement, a
decision or an assignment once — through their own agent or the CLI — and every dev agent that
touches that code later gets it, so the next feature doesn't forget what the last one agreed.

fael gives the repo a memory that agents can't skip:

- **Writing costs no extra turn.** The agent ends its reply with `fael decision|issue|note: …
  [files: …]` lines and fael files them — no turn is stopped or re-prompted. Want the old
  enforcement (no row, no end of turn)? Opt in with `[capture] block = true`.
- **Memory finds them.** When an agent reads a file, the decisions and open bugs about *that file*
  come attached — nobody has to remember to search. Sub-agents too: one starts with an empty
  context and only a short brief, and the file it opens brings the memory the brief left out.
- **No spam in context.** Rows are pushed per file, once per context window (a sub-agent, or a
  session after compaction, is told again — it no longer has them), and cut to a token budget
  (800 by default) — not a notes dump. Anything else the agent asks for itself, through MCP.
  `fael stats` shows exactly what fael has put into context.
- **It follows the code.** Rename a file and its rows follow it (`git log -M`). Split one into
  several and `fael mv old new` points the rows at the new files.
- **It lives in your git, out of your branches.** Rows are plain JSONL in the clone's `.git/fael/`,
  shared at once by every worktree, so PRs never carry log lines to conflict on. `fael sync` carries
  them to teammates through your own remote (`refs/fael/*`). No server, no account.

Works with **Claude Code, Codex and OpenCode**, and any MCP host. One small binary; hooks run in 2–3 ms.

## What you get

| Without fael | With fael |
|---|---|
| Each session starts from zero | Session opens with what the last one left: open bugs, recent decisions |
| "Why is it like this?" — ask again, guess again | The reason sits next to the file, from the agent that made the call |
| Agent notices a bug mid-task, then forgets it | It's filed on the spot, and shown to whoever touches that file next |
| Two agents in parallel worktrees hit the same problem | The first files it; the second gets it when it opens the file — the same hour, before any commit |
| A sub-agent finds something and its summary drops it | Its `fael issue: …` line is filed when it stops (Claude Code), and the parent gets it on that file |
| Knowledge stays in one person's chat history | It's in the clone — teammates and their agents get it on `fael sync` |
| A PM's requirement lives in a ticket the agent never opens | `fael add decision … --files src/pay.rs` — it's in front of the agent the moment it opens the file |

## Install

**1. Get the binary** — prebuilt for macOS, Linux and Windows. No Rust needed.

```bash
npm i -g @zecalis/fael               # Node
brew install zecalis/tap/fael    # Homebrew (macOS / Linux)
curl -LsSf https://github.com/zecalis/fael/releases/latest/download/fael-installer.sh | sh
```

Windows (PowerShell):

```powershell
irm https://github.com/zecalis/fael/releases/latest/download/fael-installer.ps1 | iex
```

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

- **npm:** [@zecalis/fael](https://www.npmjs.com/package/@zecalis/fael)
- **Homebrew tap:** [zecalis/homebrew-tap](https://github.com/zecalis/homebrew-tap)
- **Releases & changelog:** [GitHub Releases](https://github.com/zecalis/fael/releases)
- **Bugs & ideas:** [Issues](https://github.com/zecalis/fael/issues)
- **Docs:** [architecture](docs/architecture.md) · [log format](docs/format.md) (read/write it without fael) · [integrate another agent](docs/integrate.md)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [Code of Conduct](CODE_OF_CONDUCT.md)

## License

[MIT](LICENSE) © zecalis
