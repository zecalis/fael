//! Help text — the full usage plus one section per command.
//! `main.rs` owns the parser; this module only owns the strings, so the
//! 400-line file cap never forces help content to shrink.

/// One row per command: name, the one-line summary the full usage lists,
/// and the section `fael <cmd> --help` prints (synopsis, then notes). The
/// only copy — the command list and each section both come from here, and
/// `docs_match_flags` holds docs/architecture.md to the same flags.
const COMMANDS: &[(&str, &str, &str)] = &[
    (
        "add",
        "write a row",
        "fael add <kind> \"<text>\" [--files a,b] [--key k] [--title t] [--to who] [--revisit date|text] [--urgent|--urgent-before id] [--supersedes id] [--force] [--dry-run] [--json]
    (write rows in English; file each in the same message as your next tool call, never alone;
     no --files = the files this session edited, as the edit hook recorded;
     --title = the ≤15-word headline lists show, the body is pulled by id;
     --force files a path that looks like a typo of an existing one;
     --dry-run prints the verdict the real add would act on without writing;
     a repeat on these files or key, \"Supersedes <id>\" in the text, and the
     only key on these files are filled in for you;
     batch: fael add --json - < rows.json (a JSON array; a bad row reports alone, the rest save);
     text that starts with - goes after --: fael add note --files a.rs -- \"-h\")",
    ),
    (
        "close",
        "close a row",
        "fael close <id> \"<why>\"
    (an exact id or unique prefix names the row; closing twice is rejected)",
    ),
    (
        "bump",
        "same text/files, new to/urgent/revisit",
        "fael bump <id> [--to who] [--revisit date|text] [--urgent|--urgent-before id|--not-urgent]
    (same text/files, new version — text and files never change through bump)",
    ),
    (
        "find",
        "search rows",
        "fael find [text|id] [--files a,b] [--key glob] [--kind k] [--since yyyy-mm[-dd]] [--by writer] [--to who] [--revisit[=text]] [--all] [--branches] [--full] [--limit N] [--offset M] [--text query]
     (an exact id or unique prefix pulls that row's body — and, if it is closed,
      why: `closed: <text> (<sha>)`; an id-shaped query is
      never a text search — it rejects when no row owns it, naming the rows
      that only mention it; --text forces a literal text search;
      an id counts as verified only after fael find printed it as - [<id>] —
      never type one from memory;
      --full shows every body;
     --branches also reads branches not yet merged into HEAD, tagging their rows @<branch>;
     it only sees rows committed to .fael/log on those branches — a repo that
     gitignores .fael/log gets nothing from it;
     a cut list prints the exact next call — rerun it with the new --offset)",
    ),
    (
        "keys",
        "list keys with row counts",
        "fael keys [glob]
    (lists each key with its row count; a glob narrows, e.g. fael keys \"auth:*\")",
    ),
    (
        "kickoff",
        "the session brief",
        "fael kickoff [file|anchor] [--branches] [--full] [--limit N] [--offset M]
    (the session brief: open rows ranked for a file, anchor or the whole repo;
    an explicit --limit N is not cut by budget.kickoff_tokens)",
    ),
    (
        "mv",
        "record a move git can't see",
        "fael mv <old> <new>
    (record a move git can't see — anchors, uncommitted rewrites, repos without
     git; appends an alias row, the log stays append-only, nothing is rewritten)",
    ),
    (
        "restore",
        "revert a supersede edge",
        "fael restore [<id>] [--edge id]
    (revert a supersede edge with an event row — the row keeps its id and opens
     again; <id> reopens that row when one edge still hides it, --edge names
     the superseding row when several do; an already-open row or an
     already-reverted edge is info, never an error)",
    ),
    (
        "purge",
        "delete a row for good",
        "fael purge <id>
    (permanently remove a leaked test row or a mistake: the row and its close
     events go from every month file, tree and journal; refused when another
     row supersedes or restores it, when the id names a close event (use
     fael restore for those), when it lives in an immutable compact file, or
     when the file has lines `read` would skip — run `fael doctor --fix` first;
     the next sync keeps it from coming back; copies already in a teammate's
     journal stay until purged there too)",
    ),
    (
        "migrate",
        "move a tracked repo to store = local",
        "fael migrate local
    (fold the tree log into this clone's journal — the tree copy wins on every
     id it holds, so rows edited in the tree never fall back to a stale journal
     copy — then set store = \"local\" in .fael/config.toml; safe to rerun;
     every clone runs it before the tree log is removed)",
    ),
    (
        "hook",
        "stdin in, stdout out; always exits 0",
        "fael hook <event> [--client c]
    (event: stop, session-start, read or edit; stdin in, stdout out; always exits 0)",
    ),
    (
        "stats",
        "tokens fael has put into context",
        "fael stats [--json] [--rows] [--day]
    (tokens fael has put into context, per machine;
     --rows = per-row pushes against open/closed/superseded, flagging noise?;
     --day = today's panels per repo and summed (local day: FAEL_TZ_OFFSET
     like +07:00 wins, else the machine zone); --day --json prints DayView)",
    ),
    (
        "doctor",
        "check the log; --fix repairs what it can",
        "fael doctor [--fix] [--fat]
     (check the log and the repo for problems; --fix repairs what it can, including
      closing the confirmed [Shipped] notes; [Phantom] flags citations of ids
      with no row behind them — in rows, in close reasons and in the prose of
      every *.md in the repo (fenced code skipped); [Secret] flags a row that
      holds a token — rotate it, then `fael purge <id>` (never --fix); --json prints each
      problem's full row ids for a cleanup pass; --fat lists every fat row,
      including pre-self-heal legacy rows that stay collapsed to one line by
      default)",
    ),
    (
        "compact",
        "fold old rows per writer",
        "fael compact [--writer id] [--before yyyy-mm] [--prune]
    (fold old rows into per-writer summaries; --prune deletes)",
    ),
    (
        "import",
        "import a fapony log",
        "fael import <path> [--map old/=new/]
    (import rows from a fapony log; --map rewrites a path prefix, repeatable)",
    ),
    (
        "sync",
        "push and ingest writer journals",
        "fael sync [--remote url]
    (push this writer's journal to refs/fael/<repo-id>/<writer> on the remote
     and ingest every writer's ref back; --remote wins, else git config
     fael.remote; set it with: git config fael.remote <url>)",
    ),
    (
        "mcp",
        "MCP server on stdio",
        "fael mcp
    (serve find/add/close over stdio for MCP clients)",
    ),
    (
        "install",
        "install hooks and skills for a client",
        "fael install [--client claude|codex|opencode] [--dry-run] [--replace-fapony]
    (install hooks and skills for an agent client; --dry-run prints without writing)",
    ),
    (
        "upgrade",
        "bring hooks and skills up to date (alias: update)",
        "fael upgrade [--client claude|codex|opencode] [--dry-run] [--yes] [--replace-fapony]
    (show what is out of date, then ask before writing; --yes skips the question, --dry-run only shows; `fael update` is the same)",
    ),
];

/// Commands a new user meets in week one; the rest list under "more".
/// `core_names_are_commands` keeps every name here a real COMMANDS row.
const CORE: &[&str] = &["add", "find", "close", "kickoff", "install"];

/// Full usage: core commands, the rest, global options, examples.
pub(crate) fn usage() -> String {
    let rows: Vec<(&str, String, &str)> = COMMANDS
        .iter()
        .map(|(name, summary, section)| (*name, list_args(name, section), *summary))
        .collect();
    let width = rows.iter().map(|(_, a, _)| a.len()).max().unwrap_or(0);
    let (core, rest): (Vec<_>, Vec<_>) = rows.iter().partition(|(n, ..)| CORE.contains(n));
    let list = |rows: Vec<&(&str, String, &str)>| -> String {
        rows.iter()
            .map(|(_, a, s)| format!("  {a:width$}   {s}\n"))
            .collect()
    };
    let (core, rest) = (list(core), list(rest));
    format!(
        "fael — a repo's memory that agents can't skip writing

usage: fael <command> [options]

commands:
{core}
more commands:
{rest}
global options:
  --json             one JSON row per line, uncut, for programs
  -h, --help         this help — or one command's: fael <command> --help
  -v, -V, --version  print the version

examples:
  fael add issue \"login loops\" --files src/a.rs
  fael find --files src/
  fael kickoff src/a.rs

run 'fael <command> --help' for the flags of one command."
    )
}

/// `find [text|id] [options]` from the section's synopsis line: the
/// positionals as written, every flag folded into `[options]`.
fn list_args(name: &str, section: &str) -> String {
    let synopsis = section.lines().next().unwrap_or_default();
    let words: Vec<&str> = synopsis.split(' ').skip(2).collect();
    let pos = words.iter().take_while(|w| !w.starts_with("[--")).count();
    let mut s = std::iter::once(name)
        .chain(words[..pos].iter().copied())
        .collect::<Vec<_>>()
        .join(" ");
    if pos < words.len() {
        s.push_str(" [options]");
    }
    s
}

/// `fael <cmd> --help` — only that command's section, or None when `cmd`
/// names no command (the caller falls back to the full usage).
pub(crate) fn for_command(cmd: &str) -> Option<&'static str> {
    COMMANDS
        .iter()
        .find(|(n, ..)| *n == cmd)
        .map(|(.., section)| *section)
}

/// `fael help` / `fael --help` / `fael <cmd> --help` / `fael help <cmd>`:
/// the full usage, unless a non-flag word names a command — then only that
/// command's section (`help` itself is skipped, so `help find` finds `find`).
pub(crate) fn for_argv(argv: &[String]) -> String {
    let section = argv
        .iter()
        .find(|a| !a.starts_with('-') && a.as_str() != "help")
        .and_then(|c| for_command(c));
    match section {
        Some(text) => {
            format!("{text}\n\nrun 'fael --help' for all commands and global options.")
        }
        None => usage(),
    }
}

#[cfg(test)]
mod tests {
    /// `--files`, `--revisit` … named anywhere in `s`.
    fn flags(s: &str) -> std::collections::BTreeSet<&str> {
        s.split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
            .filter(|w| w.starts_with("--") && w.len() > 2)
            .collect()
    }

    /// A misspelled or renamed CORE entry would silently drop that command
    /// from the "commands:" list.
    #[test]
    fn core_names_are_commands() {
        for c in super::CORE {
            assert!(
                super::COMMANDS.iter().any(|(n, ..)| n == c),
                "CORE names {c}, which is not in COMMANDS"
            );
        }
    }

    /// docs/architecture.md's CLI table is prose around the same synopses —
    /// each command's row must name exactly the flags its `--help` names.
    #[test]
    fn docs_match_flags() {
        let docs = include_str!("../../docs/architecture.md");
        for (name, _, section) in super::COMMANDS {
            let row = docs
                .lines()
                .find(|l| l.starts_with(&format!("| `fael {name}")))
                .unwrap_or_else(|| panic!("docs/architecture.md has no row for fael {name}"));
            let synopsis = row.split("` |").next().unwrap_or_default();
            let help = section.lines().next().unwrap_or_default();
            assert_eq!(flags(synopsis), flags(help), "fael {name}: docs vs --help");
        }
    }
}
