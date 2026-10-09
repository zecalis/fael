//! The label contract's measures (PLAN-fael-label chunk 3, `format:label`,
//! `docs/format.md` § Label): does the agent write to the convention? Never
//! whether the experience helped — that is the `said` yields. Each measure is
//! `{state, num, den, since}`, the rate left to the reader, so a missing base
//! reads `unmeasurable`, never 0%. Pure: kept usage rows and loaded logs in.

use super::experience::{Shape, close_shape};
use super::parse::Parsed;
use crate::{Log, ts_ms};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// The `format:label` contract decision's ts: closes and adds before it were
/// written before the convention, so no measure counts them.
pub const LABEL_SINCE: &str = "2026-10-09T03:58:54.270Z";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Measured,
    /// Counted, but a repo with usage since the contract has no log left.
    Partial,
    /// No base: `den = 0`, or the data was never kept.
    #[default]
    Unmeasurable,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Measure {
    pub state: State,
    pub num: usize,
    pub den: usize,
    /// Where the count starts (RFC 3339); `None` when nothing is ever kept.
    pub since: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Label {
    /// Issues whose latest close (at or after `since`) has `cause → fix`.
    pub close_core: Measure,
    /// …whose latest close names a check: a number to read, no bar — a fix
    /// with no single guarding file is no defect.
    pub guard: Measure,
    /// Keyed adds (supersedes left out: they reuse a key by design) whose key
    /// an earlier row already held, rows ordered by (ts, id) across writers.
    pub key_reuse: Measure,
    /// Always unmeasurable: usage keeps no missed find and no query (01M4FCPA).
    pub find_hit: Measure,
}

/// Each issue's latest close text: a close row, or the close `fael compact`
/// folded into the row. Ties go to the one read last.
fn latest_closes(log: &Log) -> HashMap<&str, (i64, &str)> {
    let issues: HashSet<&str> = log
        .rows
        .iter()
        .filter(|r| r.kind == "issue")
        .map(|r| r.id.as_str())
        .collect();
    let rows = log.closes.iter().filter_map(|c| {
        let id = c.reference.as_deref()?;
        Some((id, ts_ms(&c.ts)?, c.text.as_str()))
    });
    let folded = log.rows.iter().filter_map(|r| {
        let c = r.extra.get("closed")?;
        Some((
            r.id.as_str(),
            ts_ms(c["ts"].as_str()?)?,
            c["text"].as_str()?,
        ))
    });
    let mut out: HashMap<&str, (i64, &str)> = HashMap::new();
    for (id, ms, text) in rows.chain(folded).filter(|(id, ..)| issues.contains(id)) {
        if out.get(id).is_none_or(|(m, _)| ms >= *m) {
            out.insert(id, (ms, text));
        }
    }
    out
}

/// Each keyed add since `from` (by id): was its key already held?
fn key_reuse(log: &Log, from: i64) -> Vec<(&str, bool)> {
    let mut adds: Vec<_> = log
        .rows
        .iter()
        .filter(|r| !r.kind.is_empty())
        .filter_map(|r| Some((ts_ms(&r.ts)?, r.id.as_str(), r)))
        .collect();
    adds.sort_unstable_by_key(|(ms, id, _)| (*ms, *id));
    let (mut held, mut out) = (HashSet::new(), vec![]);
    for (ms, id, r) in adds {
        let Some(key) = r.key.as_deref() else {
            continue;
        };
        if ms >= from && r.supersedes.is_none() {
            out.push((id, held.contains(key)));
        }
        held.insert(key);
    }
    out
}

fn measure(num: usize, den: usize, gone: bool) -> Measure {
    let state = match (den, gone) {
        (0, _) => State::Unmeasurable,
        (_, true) => State::Partial,
        _ => State::Measured,
    };
    Measure {
        state,
        num,
        den,
        since: Some(LABEL_SINCE.to_string()),
    }
}

pub(super) fn label(parsed: &Parsed, logs: &HashMap<String, Log>) -> Label {
    let from = ts_ms(LABEL_SINCE).unwrap_or_default();
    // by id: worktrees of one repo read the same log, each row counts once
    let (mut shut, mut keyed) = (HashMap::new(), HashMap::new());
    // a removed worktree last used before the contract had nothing to count
    let gone = (parsed.rows.iter()).any(|u| u.ms >= from && !logs.contains_key(&u.repo));
    for repo in parsed.first_seen.keys() {
        let Some(log) = logs.get(repo) else { continue };
        for (id, (ms, text)) in latest_closes(log) {
            if ms >= from {
                shut.insert(id, close_shape(text));
            }
        }
        keyed.extend(key_reuse(log, from));
    }
    let count = |f: fn(&Shape) -> bool| shut.values().filter(|s| f(s)).count();
    let reused = keyed.values().filter(|r| **r).count();
    Label {
        close_core: measure(count(|s| s.core), shut.len(), gone),
        guard: measure(count(|s| s.guard), shut.len(), gone),
        key_reuse: measure(reused, keyed.len(), gone),
        find_hit: Measure::default(),
    }
}
