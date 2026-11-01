//! Friction accounting (PLAN-fael-agent-ergonomics chunk 1): one `call` usage
//! line per agent call — clean, rejected or a help read — so `fael stats` can
//! say how often a call cost another round. Write side and text; the numbers
//! are `fael-core::stats::friction`. Fails open like every usage write.
//!
//! CLI calls count as friction only inside an agent session (a human's typo is
//! no agent friction; such a call is a row tagged `who: human`, read only by
//! `stats::by_flag`); MCP calls always do. `sig` is a hash of the call's arguments —
//! the free text itself is never stored. A reject carries its `reason` from
//! `codes` (plus the gate's `rules`, or the command an unknown one was `near`),
//! and a `chunk`/`plan` call its `sub` verb — each from a closed vocabulary.
//! Every CLI row carries the call's `shape` (`verb`, flag names, positional
//! count — never a value); an MCP row carries its argument names as `flags`.

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
const AGENT_CMDS: [&str; 12] = [
    "add", "find", "close", "kickoff", "bump", "claim", "next", "keys", "mv", "chunk", "plan",
    "board",
];

/// The `chunk`/`plan` verb a call used, when it is a real one: `chunk start`
/// and `chunk add` fail for different reasons, so stats keep them apart.
/// `shape` reuses it for the call row's `verb` — the one closed vocabulary.
pub(crate) fn sub<'a>(cmd: &str, argv: &'a [String]) -> Option<&'a str> {
    let verbs: &[&str] = match cmd {
        "chunk" => crate::core::work::CHUNK_VERBS,
        "plan" => crate::core::work::PLAN_VERBS,
        _ => return None,
    };
    argv.iter()
        .skip_while(|a| *a != cmd)
        .nth(1)
        .map(String::as_str)
        .filter(|v| verbs.contains(v))
}

/// An MCP `chunk` call names its verb in `verb`, not in an argv: without it every MCP chunk
/// reject lands in one bucket no verb's fix can be read from. Same closed vocabulary as `sub`.
fn mcp_sub<'a>(cmd: &str, args: &'a Value) -> Option<&'a str> {
    args["verb"]
        .as_str()
        .filter(|v| cmd == "chunk" && crate::core::work::CHUNK_VERBS.contains(v))
}

/// The real command an unknown one was a near miss of (`did you mean X?`) —
/// a name from the command table, never what was typed.
fn near(e: &str) -> Option<&'static str> {
    let n = e.split_once("did you mean ")?.1.trim_end_matches('?');
    crate::help::names().find(|c| *c == n)
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

enum Outcome<'a> {
    Ok,
    Help,
    Reject(&'a str),
}

fn record(
    client: &str,
    cmd: &str,
    root: Option<&Path>,
    session: &str,
    (o, empty, human): (Outcome, bool, bool),
    sig: String,
    (argv, sub): (Option<&[String]>, Option<&str>),
) -> Value {
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
    if human {
        row["who"] = "human".into();
    }
    if let Some(s) = sub {
        row["sub"] = s.into();
    }
    match o {
        Outcome::Ok => {
            row["outcome"] = "ok".into();
            if let Some(a) = argv {
                super::shape::apply(cmd, a, &mut row);
            }
            if empty {
                row["empty"] = true.into();
            }
        }
        Outcome::Help => {
            row["outcome"] = "help".into();
            if let Some(a) = argv {
                super::shape::apply(cmd, a, &mut row);
            }
        }
        Outcome::Reject(e) => {
            let r = super::codes::reject(e);
            row["outcome"] = "reject".into();
            row["reason"] = r.into();
            if let Some(a) = argv {
                super::shape::apply(cmd, a, &mut row);
            }
            if r == core::stats::SHAPE_GATE {
                row["rules"] = super::codes::gate_rules(e).into();
            }
            if let Some(n) = near(e) {
                row["near"] = n.into();
            }
        }
    }
    row
}

/// The CLI's one choke point — the only place a CLI reject is counted: what `run` returned
/// for this argv. `hook` runs on every tool call and `mcp` is the server — neither
/// is a call an agent made for an answer.
pub(crate) fn record_cli(argv: &[String], res: &Result<ExitCode, String>) {
    let empty = take_empty();
    // `FAEL_FROM_CREW`: fael-crew's own call back into this binary — the verb the agent
    // typed is the one that counts, and `fael` counted it when it handed over
    if matches!(
        argv.first().map(String::as_str),
        Some("hook" | "mcp" | "held")
    ) || std::env::var_os("FAEL_FROM_CREW").is_some()
    {
        return;
    }
    let cmd = command(argv);
    let outcome = match res {
        Ok(_) if crate::help::is_request(argv) => Outcome::Help,
        Ok(_) => Outcome::Ok,
        Err(e) if e.starts_with("rejected:") => Outcome::Reject(e),
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
    // a call outside an agent session, or any call to a command no agent asks
    // an answer from, is a human's: counted as usage, never as friction — its
    // help and rejects too, or a `stats` read would show 100 per 100 with no
    // clean call to dilute it; `?` stays the agent's (an unknown command is
    // friction). Only in a real repo (`repo_at` falls back to the cwd), so the
    // app's `board` run from `/` stays unrecorded
    let human = session.is_empty() || (cmd != "?" && !AGENT_CMDS.contains(&cmd));
    if human && !(root.join(".git").exists() || root.join(".fael").exists()) {
        return;
    }
    append_row(record(
        crate::session::env_client(),
        cmd,
        Some(&root),
        &session,
        (outcome, empty, human),
        sig(argv),
        (Some(argv), sub(cmd, argv)),
    ));
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
        Err(e) if e.starts_with("rejected:") || e.starts_with("unknown tool") => Outcome::Reject(e),
        Err(_) => return,
    };
    let cmd = if AGENT_CMDS.contains(&tool) {
        tool
    } else {
        "?"
    };
    let verb = mcp_sub(cmd, args);
    let mut row = record(
        "mcp",
        cmd,
        root,
        "",
        (outcome, empty, false),
        sig(args.to_string()),
        (None, verb),
    );
    row["flags"] = super::shape::mcp_flags(args).into();
    append_row(row);
}

/// `per 100 calls`, one decimal.
fn per100(n: usize, calls: usize) -> String {
    format!("{:.1}", n as f64 * 100.0 / calls as f64)
}

/// `a ×3, b ×1` — most first, so the cause to fix leads.
pub(super) fn counts(m: &std::collections::BTreeMap<String, usize>) -> String {
    let mut v: Vec<_> = m.iter().collect();
    v.sort_by_key(|(k, n)| (std::cmp::Reverse(**n), *k));
    let v: Vec<String> = v.iter().map(|(r, n)| format!("{r} ×{n}")).collect();
    v.join(", ")
}

fn tally_text(t: &core::stats::Tally) -> String {
    let (c, bad) = (t.calls, t.rejects + t.help + t.find_repeat);
    let why = match t.reasons.is_empty() {
        true => String::new(),
        false => format!(" · rejects: {}", counts(&t.reasons)),
    };
    format!(
        "{} — reject {} · help {} · find repeat {} · first-call success {}%{why}",
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
    let gate = match (f.shape_gate, f.shape_gate_rules.is_empty()) {
        (0, _) => String::new(),
        (n, true) => format!(" · shape gate ×{n} (not friction)"),
        (n, false) => format!(
            " · shape gate ×{n}: {} (not friction)",
            counts(&f.shape_gate_rules)
        ),
    };
    if f.total.calls == 0 {
        return match f.shape_gate {
            0 => vec![],
            _ => vec![format!("  friction per 100 calls (0 calls): n/a{gate}")],
        };
    }
    let mut out = vec![format!(
        "  friction per 100 calls ({} calls): {}{gate}",
        f.total.calls,
        tally_text(&f.total),
    )];
    let mut by: Vec<_> = f.by_command.iter().collect();
    by.sort_by_key(|(k, t)| (std::cmp::Reverse(t.calls), *k));
    out.extend(
        by.iter()
            .map(|(k, t)| format!("    {k} ({} calls): {}", t.calls, tally_text(t))),
    );
    // the client is only known for calls from a shell with a session env; MCP and plain CLI read `mcp` / `cli`
    let mut cl: Vec<_> = f.by_client.iter().collect();
    cl.sort_by_key(|(k, t)| (std::cmp::Reverse(t.calls), *k));
    if cl.len() > 1 {
        out.push("  friction by client:".into());
        out.extend(
            cl.iter()
                .map(|(k, t)| format!("    {k} ({} calls): {}", t.calls, tally_text(t))),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{command, lines, mcp_sub, near, sub};
    use crate::core::stats::{Friction, Tally};

    fn argv(s: &str) -> Vec<String> {
        s.split(' ').map(String::from).collect()
    }

    #[test]
    fn the_command_is_the_first_real_command_word() {
        assert_eq!(command(&argv("find --stale")), "find");
        assert_eq!(command(&argv("help add")), "add");
        assert_eq!(command(&argv("decision x")), "?");
        assert_eq!(command(&argv("--help")), "?");
    }

    #[test]
    fn sub_and_near_keep_to_real_names() {
        assert_eq!(sub("chunk", &argv("chunk start 01M4AAAA")), Some("start"));
        assert_eq!(sub("plan", &argv("plan show p")), Some("show"));
        assert_eq!(sub("chunk", &argv("chunk 01M4AAAA")), None);
        assert_eq!(sub("find", &argv("find x")), None);
        let e = "rejected: unknown command \"fnd\" — did you mean find?";
        assert_eq!(near(e), Some("find"));
        let usage = crate::core::work::CHUNK_VERBS.join("|");
        assert!(usage.contains("start") && usage.contains("done"));
        assert_eq!(
            near("rejected: unknown command \"zz\" — try 'fael --help'"),
            None
        );
    }

    #[test]
    fn an_mcp_chunk_call_reads_its_verb_from_the_args() {
        let a = serde_json::json!({"verb": "note", "uid": "u"});
        assert_eq!(mcp_sub("chunk", &a), Some("note"));
        assert_eq!(mcp_sub("find", &a), None);
        let bad = serde_json::json!({"verb": "zzz"});
        assert_eq!(mcp_sub("chunk", &bad), None);
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
            reasons: [("unknown_flag".to_string(), 1)].into(),
        };
        let f = Friction {
            total: t.clone(),
            by_command: [("find".to_string(), t)].into(),
            shape_gate: 2,
            shape_gate_rules: [("untitled".to_string(), 2)].into(),
            ..Friction::default()
        };
        let l = lines(&f);
        assert_eq!(
            l[0],
            "  friction per 100 calls (8 calls): 25.0 — reject 12.5 · help 12.5 · find repeat 0.0 · first-call success 75% · rejects: unknown_flag ×1 · shape gate ×2: untitled ×2 (not friction)"
        );
        assert!(l[1].starts_with("    find (8 calls): 25.0"), "{l:?}");
    }
}
