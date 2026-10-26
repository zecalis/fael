//! Stats self-check: one (command, flags, session) repeated `LEAK_CALLS`
//! times inside `LEAK_WINDOW_MS` is no agent — it is a test run that forgot
//! `FAEL_STATE_DIR` and wrote into the real `usage.jsonl` (about 83k identical
//! `plan --help` lines once, found only by chance). Counted from `event:"call"`
//! lines only; says that it looks like a leak, never removes anything. Pure.

use super::parse::Parsed;
use crate::ts_ms;
use serde::Serialize;
use std::collections::HashMap;

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
}

pub(super) fn leaks(parsed: &Parsed) -> Vec<Leak> {
    type Key = (String, Vec<String>, String);
    let mut by: HashMap<Key, Vec<i64>> = HashMap::new();
    for v in parsed.kept.iter().filter(|v| v["event"] == "call") {
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
        by.entry((by_cmd, flags, stream.to_string()))
            .or_default()
            .push(ms);
    }
    let mut out: Vec<Leak> = by
        .into_iter()
        .filter_map(|((cmd, flags, stream), mut ts)| {
            ts.sort_unstable();
            let (mut lo, mut best) = (0, 0);
            for hi in 0..ts.len() {
                while ts[hi] - ts[lo] > LEAK_WINDOW_MS {
                    lo += 1;
                }
                best = best.max(hi - lo + 1);
            }
            (best >= LEAK_CALLS).then_some(Leak {
                cmd,
                flags,
                stream,
                calls: best,
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
        format!(
            r#"{{"ts":"2026-10-26T10:{:02}:{:02}.000Z","repo":"/r","client":"cli","event":"call","cmd":"{cmd}","sub":"x","flags":["help"],"session":"{session}","outcome":"help"}}"#,
            sec / 60,
            sec % 60
        )
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
