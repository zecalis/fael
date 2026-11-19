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

/// Step-1 retire batch (cli-tiers chunk retire, 2026-11-19): tier W/W GAP
/// flags, never app-contract, with 0 ok/help/reject rows in `usage.by_flag`
/// since 2026-11-01 — CLI and MCP together (`fael stats --json`: 36,019 agent
/// call lines on flag-null rows in the window, so the bar is met). Generated
/// from that window; recount before the Step-2 removal. Already-gone flags
/// (`find --groups`, `chunk done --verdict`) are not listed — they reject.
pub const RETIRED_FLAGS: &[(&str, &str)] = &[
    ("add", "urgent-before"),
    ("bump", "to"),
    ("bump", "revisit"),
    ("bump", "urgent"),
    ("bump", "urgent-before"),
    ("bump", "not-urgent"),
    ("find", "by"),
    ("find", "to"),
    ("find", "revisit"),
    ("find", "recall"),
    ("doctor", "fat"),
    ("restore", "edge"),
    ("report", "out"),
    ("report", "open"),
    ("report", "since"),
    ("compact", "writer"),
    ("compact", "before"),
    ("compact", "prune"),
    ("import", "map"),
    ("mcp", "pin"),
    ("kickoff", "full"),
    ("kickoff", "limit"),
    ("kickoff", "offset"),
    ("install", "client"),
    ("install", "dry-run"),
    ("install", "yes"),
    ("upgrade", "client"),
    ("upgrade", "dry-run"),
    ("upgrade", "yes"),
    ("board", "watch"),
];

/// Verb-level retirements from the same window: no `by_flag` row at all for
/// the verb (`run end` stays — it is app-contract, never removed by count).
pub const RETIRED_VERBS: &[(&str, &str)] = &[("run", "beat"), ("chunk", "unapprove")];

/// One `warn:`-prefixed line per retired flag/verb this argv uses — Step 1
/// says the use still works, so this never rejects. A help request stays
/// quiet; text after `--` is a row's text, never a flag.
pub fn retired_warns(argv: &[String]) -> Vec<String> {
    if is_request(argv) {
        return Vec::new();
    }
    let mut pos: Vec<&str> = Vec::new();
    let mut flags: Vec<&str> = Vec::new();
    for a in argv {
        if a == "--" {
            break;
        }
        if let Some(f) = a.strip_prefix("--") {
            let f = f.split('=').next().unwrap_or("");
            if !f.is_empty() {
                flags.push(f);
            }
        } else if !a.starts_with('-') {
            pos.push(a.as_str());
        }
    }
    let cmd = pos.first().copied().unwrap_or("");
    // `update` is `upgrade` under another name; the window counts it there.
    let cmd = if cmd == "update" { "upgrade" } else { cmd };
    let mut out = Vec::new();
    for f in flags {
        if RETIRED_FLAGS.contains(&(cmd, f)) {
            out.push(format!(
                "warn: --{f} is deprecated — 0 calls since 2026-11-01, removed next release"
            ));
        }
    }
    if let Some(verb) = pos.get(1).copied()
        && RETIRED_VERBS.contains(&(cmd, verb))
    {
        out.push(format!(
            "warn: {cmd} {verb} is deprecated — 0 calls since 2026-11-01, removed next release"
        ));
    }
    out
}

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
one verb: fael chunk <verb> --help · all verbs: fael chunk --help
uids: fael board --open lists uids (`fael plan export <plan>` lists them per plan)",
        ),
        "plan" => Some(
            "fael plan — sections plus chunks in crew.db
  list                      every plan
  export <plan>             read it (--section <key>; --json is the bill)
  apply <file|-> --dry-run  change it through the bill
  new <name> \"<title>\"      a new plan
  park|unpark|done|undone|drop <plan>
one verb: fael plan <verb> --help · all verbs: fael plan --help
names: fael plan list lists plans",
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
    let named = argv
        .iter()
        .find(|a| !a.starts_with('-') && a.as_str() != "help");
    let section = named.and_then(|c| match all {
        true => for_command_full(c),
        false => for_command(c),
    });
    match section {
        Some(text) => {
            // a core section that hides flags says where they are, or an agent
            // guesses that only `fael --help --all` (commands) exists
            let more = match named.and_then(|c| for_command_full(c)) {
                Some(full) if !all && full != text => format!(
                    "\nrun 'fael {} --help --all' for its hidden flags.",
                    named.map_or("", |c| c.as_str())
                ),
                _ => String::new(),
            };
            format!("{text}\n\nrun 'fael --help' for all commands and global options.{more}")
        }
        None => match all {
            true => usage_all(),
            false => usage(),
        },
    }
}
