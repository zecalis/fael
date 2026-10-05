//! Retire rate of the rows a read push called `changed` against the ones it
//! called `unchanged` (PLAN-fael-file-hash chunk 4b) — the number the
//! `(changed)` label is gated on. Counted in events, never days: a pair is
//! (repo, session, row) from `verdict::pairs`, kept only once an edit push in
//! that repo touched one of the row's files twice after the read push (the
//! first edit is where the ask is said, the second is the deadline). It
//! counts as retired when the row was closed, superseded or bumped after the
//! read push and before that second edit. Pairs edited fewer times are not in
//! the denominator. Pure: parsed usage and logs in, counts out.

use super::parse::Parsed;
use super::retire::retire_events;
use super::verdict::Pair;
use crate::{Log, Row, ts_ms};
use serde::Serialize;
use std::collections::HashMap;

/// Edits after which a pair is settled: the deadline is the second one.
const EDITS: usize = 2;

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

/// Edit pushes per repo as (ms, files), oldest first.
fn edits(parsed: &Parsed) -> HashMap<&str, Vec<(i64, Vec<&str>)>> {
    let mut out: HashMap<&str, Vec<(i64, Vec<&str>)>> = HashMap::new();
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
        out.entry(repo).or_default().push((ms, files));
    }
    for e in out.values_mut() {
        e.sort_by_key(|(ms, _)| *ms);
    }
    out
}

/// When the pair's `EDITS`th edit after `from` landed, if it did. Files are
/// compared as the hooks wrote them. ponytail: no alias resolution, a row on
/// a since-renamed file never reaches its deadline and drops out.
fn deadline(row: &Row, from: i64, edits: &[(i64, Vec<&str>)]) -> Option<i64> {
    edits
        .iter()
        .filter(|(ms, files)| *ms > from && files.iter().any(|f| row.files.iter().any(|r| r == f)))
        .nth(EDITS - 1)
        .map(|(ms, _)| *ms)
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
        let Some(due) = edits.get(p.repo).and_then(|e| deadline(row, from, e)) else {
            continue;
        };
        let retired = events
            .get(p.repo)
            .and_then(|e| e.get(p.id))
            .is_some_and(|t| t.iter().any(|ms| *ms > from && *ms < due));
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
    fn only_pairs_edited_twice_are_in_the_denominator() {
        // A (changed) is edited twice after the push, B (unchanged) once
        let usage = read(0, "s1", "\"A\",\"B\"", "\"A\"", "\"B\"")
            + &edit(1, "a.rs")
            + &edit(2, "b.rs")
            + &edit(3, "a.rs");
        let got = split(&usage, &log(""));
        assert_eq!((got.changed, got.unchanged), (arm(1, 0), arm(0, 0)));
    }

    #[test]
    fn retired_between_the_push_and_the_second_edit_counts() {
        let usage = read(0, "s1", "\"A\",\"B\"", "\"A\",\"B\"", "")
            + &edit(10, "a.rs")
            + &edit(20, "a.rs")
            + &edit(10, "b.rs")
            + &edit(20, "b.rs");
        // A bumped between the two edits; B bumped after the second one
        let logs = log(&(bump("X1", 15, "A") + &bump("X2", 25, "B")));
        assert_eq!(split(&usage, &logs).changed, arm(2, 1));
    }

    #[test]
    fn retired_before_the_push_does_not_count_but_a_later_bump_does() {
        let usage = read(30, "s1", "\"A\"", "\"A\"", "") + &edit(40, "a.rs") + &edit(50, "a.rs");
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
            + &edit(3, "a.rs");
        assert_eq!(split(&usage, &log("")).changed, arm(0, 0));
    }

    #[test]
    fn a_pair_with_no_verdict_or_no_row_is_left_out() {
        // Z has a verdict but no row in the log; B is a normal unchanged pair
        let usage =
            read(0, "s1", "\"B\",\"Z\"", "\"Z\"", "\"B\"") + &edit(1, "b.rs") + &edit(2, "b.rs");
        let got = split(&usage, &log(""));
        assert_eq!((got.changed, got.unchanged), (arm(0, 0), arm(1, 0)));
    }

    #[test]
    fn no_edits_and_no_usage_are_zero() {
        assert_eq!(split("", &HashMap::new()), RetireSplit::default());
    }

    #[test]
    fn the_split_is_not_a_calendar_window() {
        // a bump a week later, still before the second edit: counted — the
        // 24 h `RETIRE_WINDOW_MS` of `retire.rs` is not this measure's clock
        let usage = read(0, "s1", "\"A\"", "\"A\"", "")
            + &edit(1, "a.rs")
            + "{\"ts\":\"2026-10-12T00:00:00.000Z\",\"repo\":\"/w\",\"client\":\"claude\",\"event\":\"edit\",\"bytes\":9,\"est_tokens\":2,\"ids\":[],\"files\":[\"a.rs\"]}\n";
        let week = "{\"v\":1,\"id\":\"X1\",\"ts\":\"2026-10-11T00:00:00Z\",\"by\":\"w\",\"text\":\"b\",\"bumps\":\"A\"}\n";
        assert_eq!(split(&usage, &log(week)).changed, arm(1, 1));
    }
}
