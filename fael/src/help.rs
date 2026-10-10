//! Help text — the full usage plus one section per command.
//! `main.rs` owns the parser; this module only owns the strings, so the
//! 400-line file cap never forces help content to shrink.
//!
//! Split at the chunk-3 ratchet: `core.rs` holds the trimmed sections
//! `fael <cmd> --help` shows, `tests.rs` the unit tests.

mod commands;
mod core;

#[cfg(test)]
mod tests;

use commands::COMMANDS;

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

/// Every command name — what an unknown one is measured against.
pub(crate) fn names() -> impl Iterator<Item = &'static str> {
    COMMANDS.iter().map(|(n, ..)| *n)
}

/// `fael <cmd> --help` — only that command's core section (chunk 3: the
/// flags an agent meets first), or None when `cmd` names no command (the
/// caller falls back to the full usage). `--help --all` gets the full
/// section from `for_command_full`.
pub(crate) fn for_command(cmd: &str) -> Option<&'static str> {
    if let Some(s) = core::section(cmd) {
        return Some(s);
    }
    for_command_full(cmd)
}

/// The full section, hidden flags included — what `--help --all`, the docs
/// table and did-you-mean read, so trimming the core never loses a flag.
pub(crate) fn for_command_full(cmd: &str) -> Option<&'static str> {
    COMMANDS
        .iter()
        .find(|(n, ..)| *n == cmd)
        .map(|(.., section)| *section)
}

/// Is this argv a help request — `help` first, or `--help`/`-h` before any `--`
/// (after it, `-h` is text: `fael add note --files a.rs -- -h`).
pub(crate) fn is_request(argv: &[String]) -> bool {
    argv.first().is_some_and(|c| c == "help")
        || argv
            .iter()
            .take_while(|x| *x != "--")
            .any(|x| x == "--help" || x == "-h")
}

/// `fael help` / `fael --help` / `fael <cmd> --help` / `fael help <cmd>`:
/// the full usage, unless a non-flag word names a command — then only that
/// command's core section (`help` itself is skipped, so `help find` finds
/// `find`). With `--all` anywhere before `--`, the full section instead.
pub(crate) fn for_argv(argv: &[String]) -> String {
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
        None => usage(),
    }
}
