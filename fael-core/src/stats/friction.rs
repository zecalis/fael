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
//!
//! A `SHAPE_GATE` reject (fael refused to write a row it could not read back,
//! 01M4FNQ1) is a round asked for on purpose, not friction: it is counted
//! apart (by the `rules` it named) and left out of the stream, so it neither
//! counts as a call nor spoils the calls around it.
//!
//! Rejects split by `reason` per command, and `chunk`/`plan` per verb
//! (`chunk start`): what a fix should aim at, read without transcripts.
//! `bad_value` and a gate with no `rules` are lines written before the codes
//! (2026-10-13); a new line never carries them.

use super::parse::Parsed;
use crate::ts_ms;
use serde::Serialize;
use std::collections::BTreeMap;

/// How many calls after one a reject, help or repeat still counts against it.
pub const FIRST_CALL_WINDOW: usize = 2;

/// The reject reason of a pre-write gate ("rejected: nothing written —"),
/// shared with the side that records it.
pub const SHAPE_GATE: &str = "shape_gate";

/// The counts for all calls, or for one command's.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Tally {
    pub calls: usize,
    pub rejects: usize,
    pub help: usize,
    pub find_repeat: usize,
    pub first_call_ok: usize,
    /// Rejects by `reason` (`hook/codes.rs`: unknown_flag, no_chunk, …).
    pub reasons: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Friction {
    #[serde(flatten)]
    pub total: Tally,
    /// Per command, `chunk`/`plan` per verb (`chunk start`).
    pub by_command: BTreeMap<String, Tally>,
    /// Per `client` the call row carries (`opencode`, `claude`, `codex`, `cli`, `mcp`).
    pub by_client: BTreeMap<String, Tally>,
    /// `SHAPE_GATE` rejects — in no count above.
    pub shape_gate: usize,
    /// The faults those gates named (untitled, topic_list, …; one gate can name several).
    pub shape_gate_rules: BTreeMap<String, usize>,
    /// The same totals for calls of the last 7 days, whatever the window —
    /// rejects fixed weeks ago do not hide today's rate.
    pub last_7d: Tally,
    /// The one stream (session) with the most friction, and its counts: one
    /// looping session can carry the headline (issue 01M88QVV), so the reader
    /// sees its share and the rate without it. Empty when no call was friction.
    pub top_stream: Tally,
    /// The same for the last 7 days.
    pub last_7d_top_stream: Tally,
}

impl Tally {
    /// Rejects + help + find repeats: the calls that cost another round.
    pub fn bad(&self) -> usize {
        self.rejects + self.help + self.find_repeat
    }

    /// These counts less `part`'s — the rate with one stream left out.
    pub fn without(&self, part: &Tally) -> Tally {
        let mut reasons = self.reasons.clone();
        for (k, n) in &part.reasons {
            if let Some(v) = reasons.get_mut(k) {
                *v = v.saturating_sub(*n);
            }
        }
        reasons.retain(|_, n| *n > 0);
        Tally {
            calls: self.calls.saturating_sub(part.calls),
            rejects: self.rejects.saturating_sub(part.rejects),
            help: self.help.saturating_sub(part.help),
            find_repeat: self.find_repeat.saturating_sub(part.find_repeat),
            first_call_ok: self.first_call_ok.saturating_sub(part.first_call_ok),
            reasons,
        }
    }

    fn add(&mut self, c: &Call, rep: bool, first_ok: bool) {
        self.calls += 1;
        if c.outcome == Outcome::Reject {
            self.rejects += 1;
            *self.reasons.entry(c.reason.to_string()).or_default() += 1;
        }
        self.help += (c.outcome == Outcome::Help) as usize;
        self.find_repeat += rep as usize;
        self.first_call_ok += first_ok as usize;
    }
}

/// `own` replaces `top` when it cost more rounds.
fn keep_top(top: &mut Tally, own: Tally) {
    if own.bad() > top.bad() {
        *top = own;
    }
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
    /// `cmd`, or `cmd sub` for a verb — the `by_command` key
    by: String,
    client: &'a str,
    reason: &'a str,
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

/// A call line from an agent session: `who: "human"` and `"auto"` (pollers)
/// are usage, never friction.
pub(super) fn is_agent_call(v: &serde_json::Value) -> bool {
    v["who"] != "human" && v["who"] != "auto"
}

/// A reject of the pre-write gate: a round asked for on purpose, in no count.
pub(super) fn is_gate_reject(v: &serde_json::Value) -> bool {
    v["outcome"] == "reject" && v["reason"].as_str().unwrap_or("unknown") == SHAPE_GATE
}

pub(super) fn friction(parsed: &Parsed) -> Friction {
    friction_at(parsed, crate::now_ms() as i64)
}

fn friction_at(parsed: &Parsed, now: i64) -> Friction {
    let cutoff = now - 7 * 86_400_000;
    let mut streams: BTreeMap<String, Vec<Call>> = BTreeMap::new();
    let mut out = Friction::default();
    // `who: "human"` is a call outside an agent session — usage, never friction
    for v in parsed.calls.iter().filter(|v| is_agent_call(v)) {
        let outcome = match v["outcome"].as_str() {
            Some("ok") => Outcome::Ok,
            Some("reject") => Outcome::Reject,
            Some("help") => Outcome::Help,
            _ => continue,
        };
        let Some(ms) = v["ts"].as_str().and_then(ts_ms) else {
            continue;
        };
        let reason = v["reason"].as_str().unwrap_or("unknown");
        if is_gate_reject(v) {
            out.shape_gate += 1;
            for r in v["rules"].as_array().into_iter().flatten() {
                *out.shape_gate_rules
                    .entry(r.as_str().unwrap_or("?").into())
                    .or_default() += 1;
            }
            continue;
        }
        let key = match v["session"].as_str().filter(|s| !s.is_empty()) {
            Some(s) => s.to_string(),
            None => format!(
                "{}|{}",
                v["repo"].as_str().unwrap_or(""),
                v["client"].as_str().unwrap_or("")
            ),
        };
        let cmd = v["cmd"].as_str().unwrap_or("?");
        streams.entry(key).or_default().push(Call {
            ms,
            cmd,
            by: match v["sub"].as_str() {
                Some(s) => format!("{cmd} {s}"),
                None => cmd.to_string(),
            },
            client: v["client"].as_str().unwrap_or("?"),
            reason,
            outcome,
            empty: v["empty"].as_bool().unwrap_or(false),
            sig: v["sig"].as_str().unwrap_or(""),
        });
    }
    for calls in streams.values_mut() {
        calls.sort_by_key(|c| c.ms);
        let (mut own, mut own_7d) = (Tally::default(), Tally::default());
        let rep: Vec<bool> = (0..calls.len()).map(|i| repeat(calls, i)).collect();
        let bad = |i: usize| calls[i].outcome != Outcome::Ok || rep[i];
        for (i, c) in calls.iter().enumerate() {
            let first_ok = c.outcome == Outcome::Ok
                && (i..calls.len().min(i + 1 + FIRST_CALL_WINDOW)).all(|j| !bad(j));
            let by = out.by_command.entry(c.by.clone()).or_default();
            let cl = out.by_client.entry(c.client.to_string()).or_default();
            let recent = c.ms >= cutoff;
            for t in [&mut out.total, by, cl, &mut own] {
                t.add(c, rep[i], first_ok);
            }
            if recent {
                out.last_7d.add(c, rep[i], first_ok);
                own_7d.add(c, rep[i], first_ok);
            }
        }
        keep_top(&mut out.top_stream, own);
        keep_top(&mut out.last_7d_top_stream, own_7d);
    }
    out
}

#[cfg(test)]
#[path = "friction_tests.rs"]
mod tests;
