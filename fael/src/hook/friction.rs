//! Friction accounting (PLAN-fael-agent-ergonomics chunk 1): one `call` usage
//! line per agent call — clean, rejected or a help read — so `fael stats` can
//! say how often a call cost another round. Write side and text; the numbers
//! are `fael-core::stats::friction`. Fails open like every usage write.
//!
//! CLI calls count only inside an agent session (a human's typo is no agent
//! friction); MCP calls always do. `sig` is a hash of the call's arguments —
//! the free text itself is never stored.

use super::asks::{UsageMeta, append_row};
use super::usage::usage_row;
use crate::core;
use serde_json::Value;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by a find that matched nothing; the call line reads and clears it.
static EMPTY: AtomicBool = AtomicBool::new(false);

pub(crate) fn note_empty() {
    EMPTY.store(true, Ordering::Relaxed);
}

/// Read and clear the flag — first thing in each entry, so a call that writes
/// no line (an early return) can never leave its `empty` for the next one.
fn take_empty() -> bool {
    EMPTY.swap(false, Ordering::Relaxed)
}

/// The commands an agent calls for an answer — a clean call to anything else
/// (`stats`, `doctor`, `install`) is a human at the keyboard and is not counted.
const AGENT_CMDS: [&str; 9] = [
    "add", "find", "close", "kickoff", "bump", "claim", "next", "keys", "mv",
];

/// Why a reject cost a round, from its message — the vocabulary the plan fixed.
fn reason(e: &str) -> &'static str {
    if e.starts_with("rejected: nothing written") {
        core::stats::SHAPE_GATE
    } else if e.starts_with("rejected: unknown flag") {
        "unknown_flag"
    } else if e.contains(" takes no --") {
        "flag_not_taken"
    } else if e.contains("is not an id") || e.contains("ids not shown") {
        // `find a b` prints each bad id, then rejects with the count — only the count is the Err
        "bad_id"
    } else if e.starts_with("rejected: unknown command") || e.starts_with("unknown tool") {
        "unknown_command"
    } else {
        "bad_value"
    }
}

/// The command a call was about: the first word that is a real command (so
/// `fael help find` and `fael find --stale` both read `find`), else `?`.
fn command(argv: &[String]) -> &str {
    argv.iter()
        .find(|a| !a.starts_with('-') && a.as_str() != "help")
        .map(String::as_str)
        .filter(|c| crate::help::for_command(c).is_some())
        .unwrap_or("?")
}

fn sig<T: Hash>(x: T) -> String {
    let mut h = DefaultHasher::new();
    x.hash(&mut h);
    format!("{:x}", h.finish())
}

enum Outcome {
    Ok,
    Help,
    Reject(&'static str),
}

fn record(
    client: &str,
    cmd: &str,
    root: Option<&Path>,
    session: &str,
    (o, empty): (Outcome, bool),
    sig: String,
) {
    let meta = UsageMeta {
        session: (!session.is_empty()).then_some(session),
        ..UsageMeta::default()
    };
    let mut row = usage_row(
        client,
        "call",
        root.unwrap_or(Path::new("")),
        "",
        &[],
        &meta,
    );
    row["cmd"] = cmd.into();
    row["sig"] = sig.into();
    match o {
        Outcome::Ok => {
            row["outcome"] = "ok".into();
            if empty {
                row["empty"] = true.into();
            }
        }
        Outcome::Help => row["outcome"] = "help".into(),
        Outcome::Reject(r) => {
            row["outcome"] = "reject".into();
            row["reason"] = r.into();
        }
    }
    append_row(row);
}

/// The CLI's one choke point, next to `record_cli_reject`: what `run` returned
/// for this argv. `hook` runs on every tool call and `mcp` is the server — neither
/// is a call an agent made for an answer.
pub(crate) fn record_cli(argv: &[String], res: &Result<ExitCode, String>) {
    let empty = take_empty();
    if matches!(argv.first().map(String::as_str), Some("hook" | "mcp")) {
        return;
    }
    let cmd = command(argv);
    let outcome = match res {
        Ok(_) if crate::help::is_request(argv) => Outcome::Help,
        Ok(_) if AGENT_CMDS.contains(&cmd) => Outcome::Ok,
        Err(e) if e.starts_with("rejected:") => Outcome::Reject(reason(e)),
        _ => return,
    };
    let Some(root) = std::env::current_dir()
        .ok()
        .and_then(|d| crate::repo_at(&d).ok())
        .map(|r| r.root)
    else {
        return;
    };
    let session = crate::session::hook_session(&root);
    if session.is_empty() {
        return;
    }
    record(
        "cli",
        cmd,
        Some(&root),
        &session,
        (outcome, empty),
        sig(argv),
    );
}

/// One MCP tool call: `res` is what the tool returned. No session — an MCP
/// server's env can name another session's (`stats:no-row-attribution`), so its
/// calls stream by repo and client.
pub(crate) fn record_mcp_call(
    tool: &str,
    args: &Value,
    root: Option<&Path>,
    res: &Result<String, String>,
) {
    let empty = take_empty();
    let outcome = match res {
        Ok(_) => Outcome::Ok,
        Err(e) if e.starts_with("rejected:") || e.starts_with("unknown tool") => {
            Outcome::Reject(reason(e))
        }
        Err(_) => return,
    };
    let cmd = if AGENT_CMDS.contains(&tool) {
        tool
    } else {
        "?"
    };
    record(
        "mcp",
        cmd,
        root,
        "",
        (outcome, empty),
        sig(args.to_string()),
    );
}

/// `per 100 calls`, one decimal.
fn per100(n: usize, calls: usize) -> String {
    format!("{:.1}", n as f64 * 100.0 / calls as f64)
}

fn tally_text(t: &core::stats::Tally) -> String {
    let (c, bad) = (t.calls, t.rejects + t.help + t.find_repeat);
    format!(
        "{} — reject {} · help {} · find repeat {} · first-call success {}%",
        per100(bad, c),
        per100(t.rejects, c),
        per100(t.help, c),
        per100(t.find_repeat, c),
        t.first_call_ok * 100 / c
    )
}

/// The `fael stats` friction lines: the total per 100 calls, the reject
/// reasons, then each command's own — nothing called and nothing gated = no
/// lines; gate rejects alone get the line with no rate (none of 0 calls).
pub(super) fn lines(f: &core::stats::Friction) -> Vec<String> {
    let gate = if f.shape_gate == 0 {
        String::new()
    } else {
        format!(" · shape gate ×{} (not friction)", f.shape_gate)
    };
    if f.total.calls == 0 {
        return match f.shape_gate {
            0 => vec![],
            _ => vec![format!("  friction per 100 calls (0 calls): n/a{gate}")],
        };
    }
    let reasons: Vec<String> = f.reasons.iter().map(|(r, n)| format!("{r} ×{n}")).collect();
    let mut out = vec![format!(
        "  friction per 100 calls ({} calls): {}{}{gate}",
        f.total.calls,
        tally_text(&f.total),
        if reasons.is_empty() {
            String::new()
        } else {
            format!(" · rejects: {}", reasons.join(", "))
        }
    )];
    let mut by: Vec<_> = f.by_command.iter().collect();
    by.sort_by_key(|(k, t)| (std::cmp::Reverse(t.calls), *k));
    out.extend(
        by.iter()
            .map(|(k, t)| format!("    {k} ({} calls): {}", t.calls, tally_text(t))),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::{command, lines, reason};
    use crate::core::stats::{Friction, Tally};

    fn argv(s: &str) -> Vec<String> {
        s.split(' ').map(String::from).collect()
    }

    #[test]
    fn rejects_get_the_plans_reasons() {
        for (msg, want) in [
            (
                "rejected: unknown flag --stale — try 'fael --help'",
                "unknown_flag",
            ),
            (
                "rejected: bump takes no --title — try 'fael --help'",
                "flag_not_taken",
            ),
            (
                "rejected: \"-k\" is not an id — copy it from fael find",
                "bad_id",
            ),
            (
                "rejected: 2 of 2 ids not shown — the rest printed",
                "bad_id",
            ),
            (
                "rejected: unknown command \"decision\" — try 'fael --help'",
                "unknown_command",
            ),
            ("rejected: --limit 0 shows nothing", "bad_value"),
            (
                "rejected: nothing written — text is 131 words with no title (or add --force to file it as is)",
                "shape_gate",
            ),
        ] {
            assert_eq!(reason(msg), want, "{msg}");
        }
    }

    #[test]
    fn the_command_is_the_first_real_command_word() {
        assert_eq!(command(&argv("find --stale")), "find");
        assert_eq!(command(&argv("help add")), "add");
        assert_eq!(command(&argv("decision x")), "?");
        assert_eq!(command(&argv("--help")), "?");
    }

    #[test]
    fn text_is_silent_with_no_calls_and_per_100_otherwise() {
        assert!(lines(&Friction::default()).is_empty());
        // gate rejects alone still print, with no rate to divide by zero
        let gated = Friction {
            shape_gate: 3,
            ..Friction::default()
        };
        assert_eq!(
            lines(&gated),
            ["  friction per 100 calls (0 calls): n/a · shape gate ×3 (not friction)"]
        );
        let t = Tally {
            calls: 8,
            rejects: 1,
            help: 1,
            find_repeat: 0,
            first_call_ok: 6,
        };
        let f = Friction {
            total: t.clone(),
            reasons: [("unknown_flag".to_string(), 1)].into(),
            by_command: [("find".to_string(), t)].into(),
            shape_gate: 2,
        };
        let l = lines(&f);
        assert_eq!(
            l[0],
            "  friction per 100 calls (8 calls): 25.0 — reject 12.5 · help 12.5 · find repeat 0.0 · first-call success 75% · rejects: unknown_flag ×1 · shape gate ×2 (not friction)"
        );
        assert!(l[1].starts_with("    find (8 calls): 25.0"), "{l:?}");
    }
}
