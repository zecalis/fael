//! Decision yield by lifecycle state (`value.decision_by_state`, plan fael-adoption): of the
//! decision rows a push handed over, how many were still in context at an edit, split live
//! vs held, and how many live decisions each file carries against the cap. Pure: the adapter
//! reads the disk and hands over the log marked dormant/retired plus the held ids.

use super::lifecycle::{State, state_of};
use super::parse::Parsed;
use super::value::{EventValue, pairs};
use crate::{Log, closed, is_anchor, superseded};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Dormant is not here: a dormant row is never pushed (decision 01M5528X), so a count would
/// only be state drift. The state is read now, not at push time.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DecisionByState {
    pub live: EventValue,
    pub held: EventValue,
    /// Files by how many live decisions name them: keys `"1"`..`"<max_live>"` and `">max"`,
    /// zeros included. Anchors are no files; a path is counted as written, renames not folded.
    pub live_per_file: BTreeMap<String, usize>,
}

pub fn decision_by_state(
    parsed: &Parsed,
    log: &Log,
    held: &HashSet<String>,
    max_live: usize,
) -> DecisionByState {
    let gone: HashSet<&str> = closed(log).union(&superseded(log)).copied().collect();
    let state: HashMap<&str, State> = log
        .rows
        .iter()
        .filter(|r| r.kind == "decision")
        .map(|r| (r.id.as_str(), state_of(r, &gone, log, held)))
        .collect();
    let mut out = DecisionByState::default();
    let (pushed, in_context) = pairs(parsed, true);
    for k @ (_, _, id) in pushed.keys() {
        let slot = match state.get(id) {
            Some(State::Live) => &mut out.live,
            Some(State::Held) => &mut out.held,
            _ => continue,
        };
        slot.pushed += 1;
        slot.in_context_at_edit += usize::from(in_context.contains(k));
    }
    let mut per_file: HashMap<&str, usize> = HashMap::new();
    for r in log
        .rows
        .iter()
        .filter(|r| state.get(r.id.as_str()) == Some(&State::Live))
    {
        let files: HashSet<&str> = r.files.iter().map(String::as_str).collect();
        for f in files.into_iter().filter(|f| !is_anchor(f)) {
            *per_file.entry(f).or_default() += 1;
        }
    }
    out.live_per_file = (1..=max_live).map(|n| (n.to_string(), 0)).collect();
    out.live_per_file.insert(format!(">{max_live}"), 0);
    for n in per_file.into_values() {
        let key = if n > max_live {
            format!(">{max_live}")
        } else {
            n.to_string()
        };
        *out.live_per_file.entry(key).or_default() += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Row;

    fn dec(id: &str, files: &[&str]) -> Row {
        Row {
            id: id.into(),
            kind: "decision".into(),
            files: files.iter().map(|f| f.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn splits_live_from_held_and_buckets_files() {
        let mut log = Log::default();
        // a,b,c,d live on x.rs (4 > cap 3), a also on y.rs and an anchor; h held; z dormant
        for (id, files) in [
            ("a", &["x.rs", "y.rs", "doc:t"][..]),
            ("b", &["x.rs"]),
            ("c", &["x.rs"]),
            ("d", &["x.rs"]),
            ("h", &["x.rs"]),
            ("z", &["y.rs"]),
        ] {
            log.rows.push(dec(id, files));
        }
        log.dormant = ["z".to_string()].into();
        let held: HashSet<String> = ["h".to_string()].into();
        // s1 was pushed a and h and edited both's file; s2 was pushed a, never edited; z is
        // never counted (dormant)
        let usage = r#"{"ts":"2026-09-25T00:00:00.000Z","repo":"/r","client":"claude","event":"in-context","bytes":0,"est_tokens":0,"ids":[],"in_context":["a"],"session":"s0"}
{"ts":"2026-09-26T00:00:00.000Z","repo":"/r","client":"claude","event":"session-start","bytes":9,"est_tokens":2,"ids":["a","h","z"],"session":"s1"}
{"ts":"2026-09-26T00:00:00.000Z","repo":"/r","client":"claude","event":"session-start","bytes":9,"est_tokens":2,"ids":["a"],"session":"s2"}
{"ts":"2026-09-26T00:01:00.000Z","repo":"/r","client":"claude","event":"in-context","bytes":0,"est_tokens":0,"ids":[],"in_context":["a","h"],"session":"s1"}
"#;
        let parsed = super::super::parse::parse(usage, std::path::Path::new("/s/usage.jsonl"), &[]);
        let d = decision_by_state(&parsed, &log, &held, 3);
        assert_eq!((d.live.pushed, d.live.in_context_at_edit), (2, 1));
        assert_eq!((d.held.pushed, d.held.in_context_at_edit), (1, 1));
        // y.rs has live a only; x.rs has live a, b, c, d
        let files: Vec<_> = d
            .live_per_file
            .iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect();
        assert_eq!(files, [("1", 1), ("2", 0), ("3", 0), (">3", 1)]);
    }
}
