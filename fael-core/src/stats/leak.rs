//! Stats self-check: one (command, flags, session) repeated `LEAK_CALLS`
//! times inside `LEAK_WINDOW_MS` is no steady agent work — it is either a
//! test run that forgot `FAEL_STATE_DIR` and wrote into the real `usage.jsonl`
//! (about 83k identical `plan --help` lines once, found only by chance) or a
//! runaway poll/retry loop in a live agent session (a session id repeating
//! `plan export`/`find` hundreds of times a minute). Counted from
//! `event:"call"` lines only; says what it looks like, never removes anything. Pure.

use super::parse::Parsed;
use crate::ts_ms;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// Calls of one shape inside the window that make it look like a leak.
pub const LEAK_CALLS: usize = 20;
/// One minute: no agent session repeats one call that fast.
pub const LEAK_WINDOW_MS: i64 = 60_000;

/// The busiest minute of one repeated call shape.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Leak {
    /// `cmd`, or `cmd sub` for a verb.
    pub cmd: String,
    pub flags: Vec<String>,
    /// The session the lines name, else the repo path.
    pub stream: String,
    /// Calls in the busiest window.
    pub calls: usize,
    /// Different `sig`s among them: many = one shell loop over different
    /// arguments (a fan-out), one = the same call retried.
    pub distinct: usize,
    /// Of `calls`, how many were rejected.
    pub rejects: usize,
}

pub(super) fn leaks(parsed: &Parsed) -> Vec<Leak> {
    type Key = (String, Vec<String>, String);
    let mut by: HashMap<Key, Vec<(i64, String, bool)>> = HashMap::new();
    for v in &parsed.calls {
        let (Some(cmd), Some(ms)) = (v["cmd"].as_str(), v["ts"].as_str().and_then(ts_ms)) else {
            continue;
        };
        let by_cmd = match v["sub"].as_str() {
            Some(s) => format!("{cmd} {s}"),
            None => cmd.to_string(),
        };
        let flags = v["flags"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|f| f.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let stream = v["session"].as_str().or(v["repo"].as_str()).unwrap_or("");
        let sig = v["sig"].as_str().unwrap_or("").to_string();
        by.entry((by_cmd, flags, stream.to_string()))
            .or_default()
            .push((ms, sig, v["outcome"] == "reject"));
    }
    let mut out: Vec<Leak> = by
        .into_iter()
        .filter_map(|((cmd, flags, stream), mut ts)| {
            ts.sort_unstable();
            let (mut lo, mut best, mut at) = (0, 0, (0, 0));
            for hi in 0..ts.len() {
                while ts[hi].0 - ts[lo].0 > LEAK_WINDOW_MS {
                    lo += 1;
                }
                if hi - lo + 1 > best {
                    (best, at) = (hi - lo + 1, (lo, hi));
                }
            }
            let win = &ts[at.0..=at.1];
            let distinct = win.iter().map(|w| &w.1).collect::<HashSet<_>>().len();
            (best >= LEAK_CALLS).then_some(Leak {
                cmd,
                flags,
                stream,
                calls: best,
                distinct,
                rejects: win.iter().filter(|w| w.2).count(),
            })
        })
        .collect();
    out.sort_by(|a, b| b.calls.cmp(&a.calls).then_with(|| a.cmd.cmp(&b.cmd)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn parsed(lines: &[String]) -> Parsed {
        super::super::parse(&lines.join("\n"), Path::new("/s/usage.jsonl"), &[])
    }

    fn call(sec: u32, cmd: &str, session: &str) -> String {
        call_as(sec, cmd, session, "s", "help")
    }

    fn call_as(sec: u32, cmd: &str, session: &str, sig: &str, outcome: &str) -> String {
        format!(
            r#"{{"ts":"2026-10-26T10:{:02}:{:02}.000Z","repo":"/r","client":"cli","event":"call","cmd":"{cmd}","sub":"x","flags":["help"],"session":"{session}","outcome":"{outcome}","sig":"{sig}"}}"#,
            sec / 60,
            sec % 60
        )
    }

    #[test]
    fn the_burst_says_how_many_calls_differ_and_how_many_were_rejected() {
        // a shell loop over 30 different ids, 25 of them rejected
        let lines: Vec<String> = (0..30)
            .map(|i| {
                let outcome = if i < 25 { "reject" } else { "ok" };
                call_as(i, "chunk", "s1", &format!("sig{i}"), outcome)
            })
            .collect();
        let l = leaks(&parsed(&lines));
        assert_eq!((l[0].calls, l[0].distinct, l[0].rejects), (30, 30, 25));
        // the same call 25 times: one sig
        let same: Vec<String> = (0..25)
            .map(|i| call_as(i, "chunk", "s1", "one", "ok"))
            .collect();
        let l = leaks(&parsed(&same));
        assert_eq!((l[0].calls, l[0].distinct, l[0].rejects), (25, 1, 0));
    }

    #[test]
    fn a_burst_of_one_call_shape_is_a_leak() {
        let lines: Vec<String> = (0..25).map(|i| call(i, "plan", "s1")).collect();
        let l = leaks(&parsed(&lines));
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].cmd.as_str(), l[0].calls), ("plan x", 25));
    }

    #[test]
    fn spread_out_or_mixed_calls_are_not() {
        // 25 calls, 10 s apart: at most 7 in any minute
        let slow: Vec<String> = (0..25).map(|i| call(i * 10, "plan", "s1")).collect();
        assert!(leaks(&parsed(&slow)).is_empty());
        // 25 in a minute but split over two sessions: 13 and 12
        let split: Vec<String> = (0..25)
            .map(|i| call(i, "plan", if i % 2 == 0 { "a" } else { "b" }))
            .collect();
        assert!(leaks(&parsed(&split)).is_empty());
    }
}
