//! Top-level usage: core commands shown, everything else one `--all` away.
//!
//! `COMMANDS` in `commands.rs` stays the one source — this module only
//! partitions it. New strings live here, not in `commands.rs` (336 lines
//! of the 400 cap).

use super::commands::COMMANDS;

/// Commands a new team meets first (decision `product:cli-tiers-table`,
/// key `product:cli-tiers-table`, owner-fixed).
/// `core_names_are_commands` keeps every name here a real COMMANDS row.
pub(super) const CORE: &[&str] = &["add", "find", "close", "bump", "claim", "doctor", "stats"];

/// Workflow commands no tier-test source names yet (tier GAP: the four
/// commands no skill, brief/rules, AGENTS.md, app contract or README names).
/// Owner 2026-10-19: a W item named by none of the five sources stays shown
/// in default help — never hidden. `core_names_are_commands` covers these too.
pub(super) const GAP_SHOWN: &[&str] = &["next", "keys", "compact", "intake"];

/// `fael <cmd> [options]` for one row of the COMMANDS table: the positionals
/// as written, every flag folded into `[options]`.
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

fn rows() -> Vec<(&'static str, String, &'static str)> {
    COMMANDS
        .iter()
        .map(|(name, summary, section)| (*name, list_args(name, section), *summary))
        .collect()
}

fn width_of(all: &[(&str, String, &str)]) -> usize {
    all.iter().map(|(_, a, _)| a.len()).max().unwrap_or(0)
}

fn list(select: &[&(&str, String, &str)], width: usize) -> String {
    select
        .iter()
        .map(|(_, a, s)| format!("  {a:width$}   {s}\n"))
        .collect()
}

fn footer() -> &'static str {
    "global options:
  --json             one JSON row per line, uncut, for programs
  -h, --help         this help — or one command's: fael <command> --help
  -v, -V, --version  print the version

examples:
  fael add issue \"login loops after a token refresh\" --title \"login loop\" --files src/a.rs
  fael find --files src/
  fael stats --since 2026-10-01

run 'fael <command> --help' for the flags of one command."
}

/// Core plus the unguided GAP (owner: never hidden), then the line to the full list.
pub(super) fn usage() -> String {
    let all = rows();
    let width = width_of(&all);
    let core: Vec<_> = all.iter().filter(|(n, ..)| CORE.contains(n)).collect();
    let gap: Vec<_> = all.iter().filter(|(n, ..)| GAP_SHOWN.contains(n)).collect();
    let (core, gap) = (list(&core, width), list(&gap, width));
    let footer = footer();
    format!(
        "fael — the repo's work ledger, handed to each agent at the file or chunk it works on

usage: fael <command> [options]

commands:
{core}also shown (no guide names them yet — never hidden):
{gap}{footer}
run 'fael --help --all' for every command."
    )
}

/// The old full list: core, then everything else.
pub(super) fn usage_all() -> String {
    let all = rows();
    let width = width_of(&all);
    let (core, rest): (Vec<_>, Vec<_>) = all.iter().partition(|(n, ..)| CORE.contains(n));
    let (core, rest) = (list(&core, width), list(&rest, width));
    let footer = footer();
    format!(
        "fael — the repo's work ledger, handed to each agent at the file or chunk it works on

usage: fael <command> [options]

commands:
{core}more commands:
{rest}{footer}
run 'fael --help --all' for every command."
    )
}
