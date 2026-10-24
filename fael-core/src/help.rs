//! Help text — the full usage plus one section per command.
//! `main.rs` owns the parser; this module only owns the strings, so the
//! 400-line file cap never forces help content to shrink.
//!
//! Split at the chunk-3 ratchet: `core.rs` holds the trimmed sections
//! `fael <cmd> --help` shows, `tests.rs` the unit tests.

mod commands;
mod core;
pub mod reject;
mod top;
mod verb;

#[cfg(test)]
mod tests;

use commands::COMMANDS;

/// Core-only usage; the full list is `usage_all()`.
pub fn usage() -> String {
    top::usage()
}

/// Every command: core, then the rest under "more commands:".
pub fn usage_all() -> String {
    top::usage_all()
}

/// A bare `fael chunk` / `fael plan`: the few verbs met first, not the whole synopsis.
pub fn bare(cmd: &str) -> Option<&'static str> {
    match cmd {
        "chunk" => Some(
            "fael chunk — the units of work in a plan
  start <uid>               claim it and print its brief
  done <uid> \"<handoff>\"    finish it (--pr N when there is a PR)
  wait <uid> \"<question>\" --on owner|data
  show <uid|label>          one chunk as the board has it
  add \"<title>\"             a new draft (--plan inbox|<plan>)
one verb: fael chunk <verb> --help · all verbs: fael chunk --help",
        ),
        "plan" => Some(
            "fael plan — sections plus chunks in crew.db
  list                      every plan
  export <plan>             read it (--section <key>; --json is the bill)
  apply <file|-> --dry-run  change it through the bill
  new <name> \"<title>\"      a new plan
  park|unpark|done|undone|drop <plan>
one verb: fael plan <verb> --help · all verbs: fael plan --help",
        ),
        _ => None,
    }
}

/// The reject for a known verb called with the wrong arguments: its own synopsis
/// on line two (line one keeps the code the usage log classifies by).
pub fn wrong_args(cmd: &str, verb: Option<&str>, fallback: &str) -> String {
    match verb.and_then(|v| verb::synopsis(cmd, v)) {
        Some(syn) => format!(
            "rejected: wrong arguments for {cmd} {} — try 'fael {cmd} --help'\n  {syn}",
            verb.unwrap_or_default()
        ),
        None => format!("rejected: {fallback}"),
    }
}

/// Every command name — what an unknown one is measured against.
pub fn names() -> impl Iterator<Item = &'static str> {
    COMMANDS.iter().map(|(n, ..)| *n)
}

/// The one-line summary `fael --help --all` lists for `cmd`.
pub fn summary(cmd: &str) -> Option<&'static str> {
    COMMANDS
        .iter()
        .find(|(n, ..)| *n == cmd)
        .map(|(_, summary, _)| *summary)
}

/// `fael <cmd> --help` — only that command's core section (chunk 3: the
/// flags an agent meets first), or None when `cmd` names no command (the
/// caller falls back to the full usage). `--help --all` gets the full
/// section from `for_command_full`.
pub fn for_command(cmd: &str) -> Option<&'static str> {
    if let Some(s) = core::section(cmd) {
        return Some(s);
    }
    for_command_full(cmd)
}

/// The full section, hidden flags included — what `--help --all`, the docs
/// table and did-you-mean read, so trimming the core never loses a flag.
pub fn for_command_full(cmd: &str) -> Option<&'static str> {
    COMMANDS
        .iter()
        .find(|(n, ..)| *n == cmd)
        .map(|(.., section)| *section)
}

/// Is this argv a help request — `help` first, or `--help`/`-h` before any `--`
/// (after it, `-h` is text: `fael add note --files a.rs -- -h`).
pub fn is_request(argv: &[String]) -> bool {
    argv.first().is_some_and(|c| c == "help")
        || argv
            .iter()
            .take_while(|x| *x != "--")
            .any(|x| x == "--help" || x == "-h")
}

/// `fael help` / `fael --help` / `fael <cmd> --help` / `fael help <cmd>`:
/// the core usage, unless a non-flag word names a command — then only that
/// command's core section (`help` itself is skipped, so `help find` finds
/// `find`). With `--all` anywhere before `--`, the full section instead —
/// and with no command named, the full usage (`usage_all`).
pub fn for_argv(argv: &[String]) -> String {
    if let Some(text) = verb::for_argv(argv) {
        return text;
    }
    let all = argv.iter().take_while(|x| *x != "--").any(|x| x == "--all");
    let section = argv
        .iter()
        .find(|a| !a.starts_with('-') && a.as_str() != "help")
        .and_then(|c| match all {
            true => for_command_full(c),
            false => for_command(c),
        });
    match section {
        Some(text) => {
            format!("{text}\n\nrun 'fael --help' for all commands and global options.")
        }
        None => match all {
            true => usage_all(),
            false => usage(),
        },
    }
}
