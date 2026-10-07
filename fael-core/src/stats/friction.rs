//! Friction and first-call success (PLAN-fael-agent-ergonomics chunk 1): how
//! often an agent's call to fael costs it another round. Read from the
//! `event:"call"` usage lines the CLI (inside an agent session) and MCP write —
//! pure: the parsed lines in, one struct out.
//!
//! A call is `ok`, a `reject` (with its reason) or a `help` read. Friction =
//! rejects + helps + `find_repeat` (a `find` right after an empty `find` whose
//! arguments differ). A call is a first-call success when it is `ok`, is no
//! repeat itself, and none of the next `FIRST_CALL_WINDOW` calls of its stream
//! is friction. A stream is one session, or one (repo, client) when the line
//! carries none (MCP). Per call, not per task — fael cannot see the task.

use super::parse::Parsed;
use crate::ts_ms;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// How many calls after one a reject, help or repeat still counts against it.
pub const FIRST_CALL_WINDOW: usize = 2;

/// The counts for all calls, or for one command's.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Tally {
    pub calls: usize,
    pub rejects: usize,
    pub help: usize,
    pub find_repeat: usize,
    pub first_call_ok: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Friction {
    #[serde(flatten)]
    pub total: Tally,
    /// Rejects by `reason` (unknown_flag, flag_not_taken, bad_id, bad_value,
    /// unknown_command).
    pub reasons: BTreeMap<String, usize>,
    pub by_command: BTreeMap<String, Tally>,
}

#[derive(PartialEq)]
enum Outcome {
    Ok,
    Reject,
    Help,
}

struct Call<'a> {
    ms: i64,
    cmd: &'a str,
    outcome: Outcome,
    empty: bool,
    sig: &'a str,
}

/// A clean `find` right after an empty `find` (within the window) that asked something else —
/// a reject or help there is already counted as itself.
fn repeat(calls: &[Call], i: usize) -> bool {
    calls[i].cmd == "find"
        && calls[i].outcome == Outcome::Ok
        && calls[i.saturating_sub(FIRST_CALL_WINDOW)..i]
            .iter()
            .any(|p| {
                p.cmd == "find" && p.outcome == Outcome::Ok && p.empty && p.sig != calls[i].sig
            })
}

pub(super) fn friction(parsed: &Parsed) -> Friction {
    let mut streams: HashMap<String, Vec<Call>> = HashMap::new();
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    for v in parsed.kept.iter().filter(|v| v["event"] == "call") {
        let outcome = match v["outcome"].as_str() {
            Some("ok") => Outcome::Ok,
            Some("reject") => Outcome::Reject,
            Some("help") => Outcome::Help,
            _ => continue,
        };
        let Some(ms) = v["ts"].as_str().and_then(ts_ms) else {
            continue;
        };
        if outcome == Outcome::Reject {
            let r = v["reason"].as_str().unwrap_or("unknown");
            *reasons.entry(r.to_string()).or_default() += 1;
        }
        let key = match v["session"].as_str().filter(|s| !s.is_empty()) {
            Some(s) => s.to_string(),
            None => format!(
                "{}|{}",
                v["repo"].as_str().unwrap_or(""),
                v["client"].as_str().unwrap_or("")
            ),
        };
        streams.entry(key).or_default().push(Call {
            ms,
            cmd: v["cmd"].as_str().unwrap_or("?"),
            outcome,
            empty: v["empty"].as_bool().unwrap_or(false),
            sig: v["sig"].as_str().unwrap_or(""),
        });
    }
    let mut out = Friction {
        reasons,
        ..Friction::default()
    };
    for calls in streams.values_mut() {
        calls.sort_by_key(|c| c.ms);
        let rep: Vec<bool> = (0..calls.len()).map(|i| repeat(calls, i)).collect();
        let bad = |i: usize| calls[i].outcome != Outcome::Ok || rep[i];
        for (i, c) in calls.iter().enumerate() {
            let first_ok = c.outcome == Outcome::Ok
                && (i..calls.len().min(i + 1 + FIRST_CALL_WINDOW)).all(|j| !bad(j));
            let by = out.by_command.entry(c.cmd.to_string()).or_default();
            for t in [&mut out.total, by] {
                t.calls += 1;
                t.rejects += (c.outcome == Outcome::Reject) as usize;
                t.help += (c.outcome == Outcome::Help) as usize;
                t.find_repeat += rep[i] as usize;
                t.first_call_ok += first_ok as usize;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::parse::parse;
    use std::path::{Path, PathBuf};

    /// `(minute, cmd, outcome, empty, sig)` calls in one session → friction.
    fn run(calls: &[(u32, &str, &str, bool, &str)]) -> super::Friction {
        let text: String = calls
            .iter()
            .map(|(m, cmd, outcome, empty, sig)| {
                format!(
                    "{{\"ts\":\"2026-10-07T00:{m:02}:00.000Z\",\"repo\":\"/work/r\",\"client\":\"cli\",\"event\":\"call\",\"session\":\"s\",\"cmd\":\"{cmd}\",\"outcome\":\"{outcome}\",\"reason\":\"unknown_flag\",\"empty\":{empty},\"sig\":\"{sig}\"}}\n"
                )
            })
            .collect();
        let p = parse(
            &text,
            Path::new("/work/state/usage.jsonl"),
            &[PathBuf::from("/tmp")],
        );
        assert_eq!(p.n, 0, "call lines are no injections");
        super::friction(&p)
    }

    /// One call line; `session` empty = the line carries none.
    #[allow(clippy::too_many_arguments)]
    fn line(
        m: u32,
        session: &str,
        client: &str,
        cmd: &str,
        outcome: &str,
        empty: bool,
        sig: &str,
    ) -> String {
        let session = if session.is_empty() {
            String::new()
        } else {
            format!("\"session\":\"{session}\",")
        };
        format!(
            "{{\"ts\":\"2026-10-07T00:{m:02}:00.000Z\",\"repo\":\"/work/r\",\"client\":\"{client}\",\"event\":\"call\",{session}\"cmd\":\"{cmd}\",\"outcome\":\"{outcome}\",\"empty\":{empty},\"sig\":\"{sig}\"}}\n"
        )
    }

    fn of(lines: &[String]) -> super::Friction {
        let p = parse(
            &lines.concat(),
            Path::new("/work/state/usage.jsonl"),
            &[PathBuf::from("/tmp")],
        );
        super::friction(&p)
    }

    #[test]
    fn sessions_do_not_mix_their_calls() {
        // s1's empty find, then s2's find, interleaved: not a repeat of s1's
        let f = of(&[
            line(0, "s1", "cli", "find", "ok", true, "a"),
            line(1, "s2", "cli", "find", "ok", false, "b"),
            line(2, "s1", "cli", "add", "ok", false, "c"),
        ]);
        assert_eq!(
            (f.total.find_repeat, f.total.first_call_ok),
            (0, 3),
            "{f:?}"
        );
    }

    #[test]
    fn a_line_without_a_session_streams_by_repo_and_client() {
        let f = of(&[
            line(0, "", "mcp", "find", "ok", true, "a"),
            line(1, "", "mcp", "find", "ok", false, "b"),
        ]);
        assert_eq!(f.total.find_repeat, 1, "{f:?}");
        // another client's find is another agent
        let f = of(&[
            line(0, "", "mcp", "find", "ok", true, "a"),
            line(1, "", "cli", "find", "ok", false, "b"),
        ]);
        assert_eq!(f.total.find_repeat, 0, "{f:?}");
    }

    #[test]
    fn calls_are_read_in_time_order_not_file_order() {
        // written out of order: the empty find is the earlier call
        let f = of(&[
            line(1, "s", "cli", "find", "ok", false, "b"),
            line(0, "s", "cli", "find", "ok", true, "a"),
        ]);
        assert_eq!(f.total.find_repeat, 1, "{f:?}");
    }

    #[test]
    fn calls_at_the_tail_of_a_stream_are_judged_on_the_calls_that_exist() {
        // one call after, then the stream ends: both clean, both succeed
        let f = of(&[
            line(0, "s", "cli", "add", "ok", false, "a"),
            line(1, "s", "cli", "add", "ok", false, "b"),
        ]);
        assert_eq!(f.total.first_call_ok, 2, "{f:?}");
    }

    #[test]
    fn a_lone_clean_call_is_a_first_call_success() {
        let f = run(&[(0, "add", "ok", false, "a")]);
        assert_eq!((f.total.calls, f.total.first_call_ok), (1, 1));
        assert_eq!(f.total.rejects + f.total.help + f.total.find_repeat, 0);
    }

    #[test]
    fn a_reject_is_friction_and_spoils_the_call_before_it() {
        let f = run(&[
            (0, "add", "ok", false, "a"),
            (1, "find", "reject", false, "b"),
        ]);
        assert_eq!((f.total.rejects, f.total.first_call_ok), (1, 0));
        assert_eq!(f.reasons["unknown_flag"], 1);
        assert_eq!(f.by_command["find"].rejects, 1);
        assert_eq!(f.by_command["add"].first_call_ok, 0);
    }

    #[test]
    fn a_reject_beyond_the_window_does_not_spoil() {
        let f = run(&[
            (0, "add", "ok", false, "a"),
            (1, "add", "ok", false, "b"),
            (2, "add", "ok", false, "c"),
            (3, "add", "ok", false, "d"),
            (4, "find", "help", false, "e"),
        ]);
        assert_eq!((f.total.help, f.total.first_call_ok), (1, 2));
    }

    #[test]
    fn find_after_empty_find_is_a_repeat_but_refining_a_hit_is_not() {
        let f = run(&[(0, "find", "ok", true, "a"), (1, "find", "ok", false, "b")]);
        assert_eq!((f.total.find_repeat, f.total.first_call_ok), (1, 0));
        let f = run(&[(0, "find", "ok", false, "a"), (1, "find", "ok", false, "b")]);
        assert_eq!((f.total.find_repeat, f.total.first_call_ok), (0, 2));
        // a reject or help after an empty find is counted once, as itself
        let f = run(&[
            (0, "find", "ok", true, "a"),
            (1, "find", "help", false, "b"),
        ]);
        assert_eq!((f.total.find_repeat, f.total.help), (0, 1));
        // the same empty call again is a re-poll, not a fix
        let f = run(&[(0, "find", "ok", true, "a"), (1, "find", "ok", true, "a")]);
        assert_eq!(f.total.find_repeat, 0);
    }
}
