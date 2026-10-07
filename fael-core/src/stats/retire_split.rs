//! Retire rate of the rows a read push called `changed` against the ones it
//! called `unchanged` (PLAN-fael-file-hash chunks 4b/4c) — the number the
//! `(changed)` label is gated on. Counted in events, never days: a pair is
//! (repo, session, row) from `verdict::pairs`, kept only once an edit push in
//! that repo said the row's ask (the usage line's `said:[{kind:"ask",
//! key:<row id>}]`) and a later edit push touched one of the row's files
//! again (the deadline). Since #252 the ask speaks at most once per user
//! turn, so a held-back ask lands on a later edit than the first — the window
//! starts at the edit that said the ask, never the first edit after the read
//! push. It counts as retired when the row was closed, superseded or bumped
//! after the ask was said and before that later edit. Pairs whose ask was
//! never said, or with no later edit, are not in the denominator. Pure:
//! parsed usage and logs in, counts out.

use super::parse::Parsed;
use super::retire::retire_events;
use super::verdict::Pair;
use crate::{Log, Row, ts_ms};
use serde::Serialize;
use std::collections::HashMap;

/// Pairs that reached their deadline, and how many of those were retired.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Arm {
    pub pairs: usize,
    pub retired: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RetireSplit {
    pub changed: Arm,
    pub unchanged: Arm,
}

/// One edit push: when it landed, which row files it touched, and which row
/// asks it said.
struct Edit<'a> {
    ms: i64,
    files: Vec<&'a str>,
    asks: Vec<&'a str>,
}

/// Edit pushes per repo as `Edit`, oldest first.
fn edits(parsed: &Parsed) -> HashMap<&str, Vec<Edit<'_>>> {
    let mut out: HashMap<&str, Vec<Edit>> = HashMap::new();
    for v in &parsed.kept {
        let (true, Some(repo), Some(ms)) = (
            v["event"] == "edit",
            v["repo"].as_str(),
            v["ts"].as_str().and_then(ts_ms),
        ) else {
            continue;
        };
        let files = v["files"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| f.as_str())
            .collect();
        let asks = v["said"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| e["kind"] == "ask")
            .filter_map(|e| e["key"].as_str())
            .collect();
        out.entry(repo).or_default().push(Edit { ms, files, asks });
    }
    for e in out.values_mut() {
        e.sort_by_key(|e| e.ms);
    }
    out
}

/// The pair's window: the first file-touching edit after `from` that said the
/// row's ask, and the next file-touching edit after that one (the deadline).
/// `None` when the ask was never said or no later edit landed. Files are
/// compared as the hooks wrote them. ponytail: no alias resolution, a row on
/// a since-renamed file never reaches its deadline and drops out.
fn window(row: &Row, id: &str, from: i64, edits: &[Edit]) -> Option<(i64, i64)> {
    let touches = |e: &Edit| e.files.iter().any(|f| row.files.iter().any(|r| r == f));
    let at = edits
        .iter()
        .position(|e| e.ms > from && touches(e) && e.asks.contains(&id))?;
    let due = edits[at + 1..].iter().find(|e| touches(e)).map(|e| e.ms)?;
    Some((edits[at].ms, due))
}

pub(super) fn retire_split(
    pairs: &[Pair],
    parsed: &Parsed,
    logs: &HashMap<String, Log>,
    index: &HashMap<&str, HashMap<&str, &Row>>,
) -> RetireSplit {
    let edits = edits(parsed);
    let mut events: HashMap<&str, HashMap<&str, Vec<i64>>> = HashMap::new();
    for (repo, log) in logs {
        let by_id = events.entry(repo).or_default();
        for (id, ms) in retire_events(log) {
            by_id.entry(id).or_default().push(ms);
        }
    }
    let mut out = RetireSplit::default();
    for p in pairs {
        let (Some(changed), Some(from)) = (p.changed, p.ms) else {
            continue;
        };
        let Some(row) = index.get(p.repo).and_then(|r| r.get(p.id)) else {
            continue;
        };
        let Some((start, due)) = edits.get(p.repo).and_then(|e| window(row, p.id, from, e)) else {
            continue;
        };
        let retired = events
            .get(p.repo)
            .and_then(|e| e.get(p.id))
            .is_some_and(|t| t.iter().any(|ms| *ms > start && *ms < due));
        let arm = if changed {
            &mut out.changed
        } else {
            &mut out.unchanged
        };
        arm.pairs += 1;
        arm.retired += usize::from(retired);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::parse::parse;
    use super::super::verdict::file_verdict;
    use super::{Arm, RetireSplit};
    use crate::{Log, Row};
    use std::collections::HashMap;
    use std::path::Path;

    /// Rows A (a.rs) and B (b.rs) written before the push; `extra` lines are
    /// the retire events under test.
    fn log(extra: &str) -> HashMap<String, Log> {
        let row = |id: &str, f: &str| {
            format!(
                "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"2026-10-04T00:00:00Z\",\"by\":\"w\",\"kind\":\"decision\",\"text\":\"t\",\"files\":[\"{f}\"]}}\n"
            )
        };
        let mut rows: Vec<Row> = vec![];
        let text = row("A", "a.rs") + &row("B", "b.rs") + extra;
        crate::log::parse(text.as_bytes(), "t.jsonl", &mut rows, &mut vec![]);
        HashMap::from([(
            "/w".to_string(),
            Log {
                rows,
                ..Default::default()
            },
        )])
    }

    fn read(min: u32, session: &str, ids: &str, changed: &str, unchanged: &str) -> String {
        format!(
            "{{\"ts\":\"2026-10-05T00:{min:02}:00.000Z\",\"repo\":\"/w\",\"client\":\"claude\",\"event\":\"read\",\"bytes\":9,\"est_tokens\":2,\"session\":\"{session}\",\"ids\":[{ids}],\"changed\":[{changed}],\"unchanged\":[{unchanged}]}}\n"
        )
    }

    fn edit(min: u32, file: &str) -> String {
        format!(
            "{{\"ts\":\"2026-10-05T00:{min:02}:00.000Z\",\"repo\":\"/w\",\"client\":\"claude\",\"event\":\"edit\",\"bytes\":9,\"est_tokens\":2,\"ids\":[],\"session\":\"s1\",\"files\":[\"{file}\"]}}\n"
        )
    }

    /// An edit push that said the row asks in `asks`
    /// (`said:[{kind:"ask",key:<row id>}]` on its usage line).
    fn edit_ask(min: u32, file: &str, asks: &[&str]) -> String {
        let said = asks
            .iter()
            .map(|a| format!("{{\"kind\":\"ask\",\"key\":\"{a}\"}}"))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"ts\":\"2026-10-05T00:{min:02}:00.000Z\",\"repo\":\"/w\",\"client\":\"claude\",\"event\":\"edit\",\"bytes\":9,\"est_tokens\":2,\"ids\":[],\"session\":\"s1\",\"files\":[\"{file}\"],\"said\":[{said}]}}\n"
        )
    }

    fn bump(id: &str, min: u32, of: &str) -> String {
        format!(
            "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"2026-10-05T00:{min:02}:00Z\",\"by\":\"w\",\"text\":\"b\",\"bumps\":\"{of}\"}}\n"
        )
    }

    fn split(usage: &str, logs: &HashMap<String, Log>) -> RetireSplit {
        let parsed = parse(usage, Path::new("/s/usage.jsonl"), &[]);
        file_verdict(&parsed, logs).retire
    }

    fn arm(pairs: usize, retired: usize) -> Arm {
        Arm { pairs, retired }
    }

    #[test]
    fn a_pair_whose_ask_was_never_said_is_out() {
        // A (changed) is edited twice after the push, but no edit said its ask
        let usage = read(0, "s1", "\"A\",\"B\"", "\"A\"", "\"B\"")
            + &edit(1, "a.rs")
            + &edit(2, "b.rs")
            + &edit(3, "a.rs");
        let got = split(&usage, &log(""));
        assert_eq!((got.changed, got.unchanged), (arm(0, 0), arm(0, 0)));
    }

    #[test]
    fn a_pair_counts_once_its_ask_is_said_and_a_later_edit_lands() {
        // A (changed) said its ask at the first edit and is touched again;
        // B (unchanged) is edited twice but never said its ask
        let usage = read(0, "s1", "\"A\",\"B\"", "\"A\"", "\"B\"")
            + &edit_ask(1, "a.rs", &["A"])
            + &edit(2, "b.rs")
            + &edit(3, "a.rs");
        let got = split(&usage, &log(""));
        assert_eq!((got.changed, got.unchanged), (arm(1, 0), arm(0, 0)));
    }

    #[test]
    fn the_window_starts_at_the_edit_that_said_the_ask() {
        // the ask is held back past the first edit (#252): said at minute 10,
        // deadline at minute 20 — a bump at minute 5 (after the push, before
        // the ask) does not count, one at minute 15 does
        let usage = read(0, "s1", "\"A\"", "\"A\"", "")
            + &edit(1, "a.rs")
            + &edit_ask(10, "a.rs", &["A"])
            + &edit(20, "a.rs");
        assert_eq!(split(&usage, &log(&bump("X1", 5, "A"))).changed, arm(1, 0));
        let both = bump("X1", 5, "A") + &bump("X2", 15, "A");
        assert_eq!(split(&usage, &log(&both)).changed, arm(1, 1));
    }

    #[test]
    fn retired_between_the_ask_and_the_next_edit_counts() {
        let usage = read(0, "s1", "\"A\",\"B\"", "\"A\",\"B\"", "")
            + &edit_ask(10, "a.rs", &["A"])
            + &edit_ask(10, "b.rs", &["B"])
            + &edit(20, "a.rs")
            + &edit(20, "b.rs");
        // A bumped between the ask and the deadline; B bumped after the deadline
        let logs = log(&(bump("X1", 15, "A") + &bump("X2", 25, "B")));
        assert_eq!(split(&usage, &logs).changed, arm(2, 1));
    }

    #[test]
    fn retired_before_the_push_does_not_count_but_a_later_bump_does() {
        let usage = read(30, "s1", "\"A\"", "\"A\"", "")
            + &edit_ask(40, "a.rs", &["A"])
            + &edit(50, "a.rs");
        // bumped twice: once before the push (minute 5), once after (minute 45)
        let early = bump("X1", 5, "A");
        assert_eq!(split(&usage, &log(&early)).changed, arm(1, 0));
        let both = early + &bump("X2", 45, "A");
        assert_eq!(split(&usage, &log(&both)).changed, arm(1, 1));
    }

    #[test]
    fn an_edit_of_another_file_is_not_an_edit_of_the_row() {
        let usage = read(0, "s1", "\"A\"", "\"A\"", "")
            + &edit(1, "b.rs")
            + &edit(2, "b.rs")
            + &edit_ask(3, "a.rs", &["A"]);
        // the ask was said, but no later edit touched the row's file
        assert_eq!(split(&usage, &log("")).changed, arm(0, 0));
    }

    #[test]
    fn a_pair_with_no_verdict_or_no_row_is_left_out() {
        // Z has a verdict but no row in the log; B is a normal unchanged pair
        let usage = read(0, "s1", "\"B\",\"Z\"", "\"Z\"", "\"B\"")
            + &edit_ask(1, "b.rs", &["B"])
            + &edit(2, "b.rs");
        let got = split(&usage, &log(""));
        assert_eq!((got.changed, got.unchanged), (arm(0, 0), arm(1, 0)));
    }

    #[test]
    fn no_edits_and_no_usage_are_zero() {
        assert_eq!(split("", &HashMap::new()), RetireSplit::default());
    }

    #[test]
    fn the_split_is_not_a_calendar_window() {
        // a bump a week later, still before the deadline edit: counted — the
        // 24 h `RETIRE_WINDOW_MS` of `retire.rs` is not this measure's clock
        let usage = read(0, "s1", "\"A\"", "\"A\"", "")
            + &edit_ask(1, "a.rs", &["A"])
            + "{\"ts\":\"2026-10-12T00:00:00.000Z\",\"repo\":\"/w\",\"client\":\"claude\",\"event\":\"edit\",\"bytes\":9,\"est_tokens\":2,\"ids\":[],\"files\":[\"a.rs\"]}\n";
        let week = "{\"v\":1,\"id\":\"X1\",\"ts\":\"2026-10-11T00:00:00Z\",\"by\":\"w\",\"text\":\"b\",\"bumps\":\"A\"}\n";
        assert_eq!(split(&usage, &log(week)).changed, arm(1, 1));
    }
}
