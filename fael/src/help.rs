//! Help text — the full usage plus one section per command.
//! `main.rs` owns the parser; this module only owns the strings, so the
//! 400-line file cap never forces help content to shrink.

/// Full usage: command list, global options, examples.
pub(crate) const USAGE: &str = "fael — a repo's memory that agents can't skip writing

usage: fael <command> [options]

commands:
  add <kind> \"<text>\" [--files a,b] ...   write a row
  close <id> \"<why>\"                       close a row
  bump <id> [options]                      same text/files, new to/urgent/revisit
  find [text|id] [options]                 search rows
  keys [glob]                              list keys with row counts
  kickoff [file|anchor] [options]          the session brief
  mv <old> <new>                           record a move git can't see
  hook <event> [options]                   stdin in, stdout out; always exits 0
  stats [options]                          tokens fael has put into context
  doctor [--fix]                           check the log; --fix repairs what it can
  compact [options]                        fold old rows per writer
  import <path> [options]                  import a fapony log
  mcp                                      MCP server on stdio
  install [options]                        install hooks and skills for a client

global options:
  --json         one JSON row per line, uncut, for programs
  -h, --help     this help — or one command's: fael <command> --help
  -v, -V, --version  print the version

examples:
  fael add issue \"login loops\" --files src/a.rs
  fael find --files src/
  fael kickoff src/a.rs

run 'fael <command> --help' for the flags of one command.";

/// `fael <cmd> --help` — only that command's section, or None when `cmd`
/// names no command (the caller falls back to the full usage).
pub(crate) fn for_command(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "add" => "fael add <kind> \"<text>\" [--files a,b] [--key k] [--title t] [--to who] [--revisit date|text] [--urgent|--urgent-before id] [--supersedes id] [--force]
    (no --files = the files this session edited, as the edit hook recorded;
     --title = the ≤15-word headline lists show, the body is pulled by id;
     --force files a path that looks like a typo of an existing one)",
        "close" => "fael close <id> \"<why>\"
    (an exact id or unique prefix names the row; closing twice is rejected)",
        "bump" => "fael bump <id> [--to who] [--revisit date|text] [--urgent|--urgent-before id|--not-urgent]
    (same text/files, new version — text and files never change through bump)",
        "find" => "fael find [text|id] [--files a,b] [--key glob] [--kind k] [--since yyyy-mm[-dd]] [--by writer] [--to who] [--revisit[=text]] [--all] [--branches] [--full] [--limit N] [--offset M]
    (an exact id or unique prefix pulls that row's body; --full shows every body;
     --branches also reads branches not yet merged into HEAD, tagging their rows @<branch>;
     it only sees rows committed to .fael/log on those branches — a repo that
     gitignores .fael/log gets nothing from it;
     a cut list prints the exact next call — rerun it with the new --offset)",
        "keys" => "fael keys [glob]
    (lists each key with its row count; a glob narrows, e.g. fael keys \"auth:*\")",
        "kickoff" => "fael kickoff [file|anchor] [--branches] [--full] [--limit N] [--offset M]
    (the session brief: open rows ranked for a file, anchor or the whole repo)",
        "mv" => "fael mv <old> <new>
    (record a move git can't see — anchors, uncommitted rewrites, repos without
     git; appends an alias row, the log stays append-only, nothing is rewritten)",
        "hook" => "fael hook <stop|session-start|read|edit> [--client c]
    (stdin in, stdout out; always exits 0)",
        "stats" => "fael stats [--json] [--rows]
    (tokens fael has put into context, per machine;
     --rows = per-row pushes against open/closed/superseded, flagging noise?)",
        "doctor" => "fael doctor [--fix]
    (check the log and the repo for problems; --fix repairs what it can)",
        "compact" => "fael compact [--writer id] [--before yyyy-mm] [--prune]
    (fold old rows into per-writer summaries; --prune deletes)",
        "import" => "fael import <path> [--map old/=new/]
    (import rows from a fapony log; --map rewrites a path prefix, repeatable)",
        "mcp" => "fael mcp
    (serve add/close/find/bump over stdio for MCP clients)",
        "install" => "fael install [--client claude|codex|opencode] [--dry-run] [--replace-fapony]
    (install hooks and skills for an agent client; --dry-run prints without writing)",
        _ => return None,
    })
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
        None => USAGE.to_string(),
    }
}
