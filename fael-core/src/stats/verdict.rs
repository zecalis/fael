//! The file-hash verdict at push (01M43517 part 3): of the rows a read push
//! showed, how many got a verdict (`changed` / `unchanged`) and how many got
//! none. The hook records both lists on the usage line (`hook/usage.rs`
//! `record_usage_shadow`); a row in neither had no stamp, sits on a file over
//! the push cap, or the file is gone — the usage line cannot say which, so
//! the log is joined to split off the rows that carry no stamp (`no_fh`).
//! PLAN-fael-file-hash chunk 4b adds the retire rate of each side
//! (`retire_split.rs`) and the gate over it. A count of what push could not
//! say, never of what it saved. Pure: parsed usage and logs in.

use super::parse::Parsed;
use super::retire_split::{RetireSplit, retire_split};
use crate::{Log, Row, ts_ms};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// (session, row) pairs shown by a push that measured the verdict, by
/// outcome. `changed + unchanged + no_verdict` is every measured pair.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FileVerdict {
    /// The row's files changed since it was written.
    pub changed: usize,
    /// The row's files all still match.
    pub unchanged: usize,
    /// Shown, in neither list: no `fh` stamp, a stamped file over the push cap
    /// or gone.
    pub no_verdict: usize,
    /// Of `no_verdict`, the rows whose log entry carries no `fh` at all (every
    /// row written before the stamp existed, or on anchors only). The rest,
    /// `no_verdict - no_fh`, are stamped rows with no verdict (a file over the
    /// push cap or gone) — and rows whose log entry could not be found.
    pub no_fh: usize,
    /// Retire rate of the changed and unchanged pairs (chunk 4b).
    pub retire: RetireSplit,
}

/// Pairs a push needs on each side before the gate says anything.
pub const GATE_MIN_PAIRS: usize = 30;

impl FileVerdict {
    /// The chunk 4 gate (PLAN-fael-file-hash §6.4): `None` until each retire
    /// side holds `GATE_MIN_PAIRS`; then `Some(pass)` where pass needs both
    /// (1) the changed retire rate at least twice the unchanged one (and not
    /// zero) and (2) changed rows at most a third of every shown pair, so a
    /// `(changed)` label is not on most of what push says. Integer math only.
    pub fn gate(&self) -> Option<bool> {
        let (c, u) = (&self.retire.changed, &self.retire.unchanged);
        if c.pairs < GATE_MIN_PAIRS || u.pairs < GATE_MIN_PAIRS {
            return None;
        }
        let lift = c.retired > 0 && c.retired * u.pairs >= 2 * u.retired * c.pairs;
        let shown = self.changed + self.unchanged + self.no_verdict;
        Some(lift && self.changed * 3 <= shown)
    }
}

/// One measured (repo, session, row) pair: `Some(true)` changed,
/// `Some(false)` unchanged, `None` shown with no verdict. `ms` is the push
/// time (`None` when the line's `ts` does not parse).
pub(super) struct Pair<'a> {
    pub repo: &'a str,
    pub id: &'a str,
    pub ms: Option<i64>,
    pub changed: Option<bool>,
}

/// Read the shadow keys off the usage lines that carry them. A line from
/// before the keys existed has no `changed` array: not measured, so never
/// counted as `no_verdict`. Each (repo, session, row) pair counts once, for
/// the first measured line that showed it — the `value.by_event` key; a line
/// with no session is left out (a pair needs one).
// ponytail: file order is time order (usage.jsonl is append-only)
pub(super) fn pairs(parsed: &Parsed) -> Vec<Pair<'_>> {
    let mut seen: HashSet<(&str, &str, &str)> = HashSet::new();
    let mut out = vec![];
    for v in &parsed.kept {
        // not measured: `changed` is written on every shadowed line, empty or not
        if v["event"] == "in-context" || !v["changed"].is_array() {
            continue;
        }
        let (Some(repo), Some(session)) = (v["repo"].as_str(), v["session"].as_str()) else {
            continue;
        };
        let list = |k: &str| -> HashSet<&str> {
            v[k].as_array()
                .into_iter()
                .flatten()
                .filter_map(|i| i.as_str())
                .collect()
        };
        let (changed, unchanged) = (list("changed"), list("unchanged"));
        let ms = v["ts"].as_str().and_then(ts_ms);
        for id in list("ids") {
            if !seen.insert((repo, session, id)) {
                continue;
            }
            let verdict = if changed.contains(id) {
                Some(true)
            } else {
                unchanged.contains(id).then_some(false)
            };
            out.push(Pair {
                repo,
                id,
                ms,
                changed: verdict,
            });
        }
    }
    out
}

/// The rows by id, per repo, as the log folds them: the row itself, never a
/// bump event (those carry `bumps`, not content).
pub(super) fn row_index(logs: &HashMap<String, Log>) -> HashMap<&str, HashMap<&str, &Row>> {
    logs.iter()
        .map(|(repo, log)| {
            let rows = log.rows.iter().filter(|r| r.bumps.is_none());
            (repo.as_str(), rows.map(|r| (r.id.as_str(), r)).collect())
        })
        .collect()
}

pub(super) fn file_verdict(parsed: &Parsed, logs: &HashMap<String, Log>) -> FileVerdict {
    let pairs = pairs(parsed);
    let index = row_index(logs);
    let mut out = FileVerdict::default();
    for p in &pairs {
        match p.changed {
            Some(true) => out.changed += 1,
            Some(false) => out.unchanged += 1,
            None => {
                out.no_verdict += 1;
                // ponytail: the row as the log folds it now — a no-stamp row
                // restamped by a bump after the push still reads as stamped.
                // A row the log cannot find stays in `no_verdict - no_fh`.
                let row = index.get(p.repo).and_then(|r| r.get(p.id));
                out.no_fh += usize::from(row.is_some_and(|r| r.file_hashes().is_none()));
            }
        }
    }
    out.retire = retire_split(&pairs, parsed, logs, &index);
    out
}

#[cfg(test)]
mod tests {
    use super::{FileVerdict, file_verdict};
    use std::collections::HashMap;
    use std::path::Path;

    fn count(usage: &str) -> FileVerdict {
        let parsed = super::super::parse::parse(usage, Path::new("/s/usage.jsonl"), &[]);
        file_verdict(&parsed, &HashMap::new())
    }

    fn line(event: &str, session: &str, extra: &str) -> String {
        let s = if session.is_empty() {
            String::new()
        } else {
            format!(",\"session\":\"{session}\"")
        };
        format!(
            "{{\"ts\":\"2026-10-04T00:00:00.000Z\",\"repo\":\"/w\",\"client\":\"claude\",\"event\":\"{event}\",\"bytes\":9,\"est_tokens\":2{s}{extra}}}\n"
        )
    }

    fn v(changed: usize, unchanged: usize, no_verdict: usize) -> FileVerdict {
        FileVerdict {
            changed,
            unchanged,
            no_verdict,
            ..Default::default()
        }
    }

    #[test]
    fn lines_before_the_keys_are_not_measured() {
        // old read push: ids, no `changed` key — not a no_verdict
        let usage = line("read", "s1", ",\"ids\":[\"A\",\"B\"]")
            + &line("session-start", "s1", ",\"ids\":[\"C\"]");
        assert_eq!(count(&usage), v(0, 0, 0));
        assert_eq!(count(""), v(0, 0, 0));
    }

    #[test]
    fn ids_split_across_the_three_buckets() {
        let usage = line(
            "read",
            "s1",
            ",\"ids\":[\"A\",\"B\",\"C\",\"D\"],\"changed\":[\"A\"],\"unchanged\":[\"B\"]",
        );
        assert_eq!(count(&usage), v(1, 1, 2));
    }

    #[test]
    fn measured_but_empty_lists_are_all_no_verdict() {
        let usage = line(
            "read",
            "s1",
            ",\"ids\":[\"A\"],\"changed\":[],\"unchanged\":[]",
        );
        assert_eq!(count(&usage), v(0, 0, 1));
        // nothing shown, nothing counted
        let usage = line("read", "s1", ",\"ids\":[],\"changed\":[],\"unchanged\":[]");
        assert_eq!(count(&usage), v(0, 0, 0));
    }

    #[test]
    fn a_pair_counts_once_the_first_measured_line_decides() {
        let first = ",\"ids\":[\"A\"],\"changed\":[],\"unchanged\":[]";
        let changed = ",\"ids\":[\"A\"],\"changed\":[\"A\"],\"unchanged\":[]";
        let usage = line("read", "s1", first)
            + &line("read", "s1", changed)
            // another session is another pair
            + &line("read", "s2", changed)
            // a duplicate id inside one line is one pair
            + &line(
                "read",
                "s1",
                ",\"ids\":[\"B\",\"B\"],\"changed\":[],\"unchanged\":[\"B\"]",
            );
        assert_eq!(count(&usage), v(1, 1, 1));
        // so is another repo
        let other_repo = line("read", "s1", changed).replace("\"/w\"", "\"/x\"");
        assert_eq!(
            count(&(line("read", "s1", first) + &other_repo)),
            v(1, 0, 1)
        );
    }

    #[test]
    fn an_unmeasured_line_does_not_claim_the_pair() {
        // session-start showed A (no keys); a later measured push of A still counts
        let usage = line("session-start", "s1", ",\"ids\":[\"A\"]")
            + &line(
                "read",
                "s1",
                ",\"ids\":[\"A\"],\"changed\":[],\"unchanged\":[\"A\"]",
            );
        assert_eq!(count(&usage), v(0, 1, 0));
    }

    #[test]
    fn sessionless_lines_and_other_lines_are_left_out() {
        let usage = line(
            "read",
            "",
            ",\"ids\":[\"A\"],\"changed\":[\"A\"],\"unchanged\":[]",
        )
        // an in-context line and a pull carry no verdict
        + &line("in-context", "s1", ",\"ids\":[],\"in_context\":[\"A\"]")
        + &line(
            "find",
            "s1",
            ",\"ids\":[],\"found\":[\"A\"],\"q\":{\"id\":\"A\"}",
        );
        assert_eq!(count(&usage), v(0, 0, 0));
    }

    use crate::{Log, Row};

    fn log_of(rows: &str) -> Log {
        let mut parsed: Vec<Row> = vec![];
        crate::log::parse(rows.as_bytes(), "t.jsonl", &mut parsed, &mut vec![]);
        Log {
            rows: parsed,
            ..Default::default()
        }
    }

    fn row(id: &str, extra: &str) -> String {
        format!(
            "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"2026-10-04T00:00:00Z\",\"by\":\"w\",\"kind\":\"decision\",\"text\":\"t\",\"files\":[\"a.rs\"]{extra}}}\n"
        )
    }

    #[test]
    fn no_verdict_splits_off_rows_with_no_stamp() {
        // A has no fh, B is stamped (over the cap or gone), C is not in the log
        let logs = HashMap::from([(
            "/w".to_string(),
            log_of(&(row("A", "") + &row("B", ",\"fh\":{\"a.rs\":\"0123456789ab\"}"))),
        )]);
        let usage = line(
            "read",
            "s1",
            ",\"ids\":[\"A\",\"B\",\"C\"],\"changed\":[],\"unchanged\":[]",
        );
        let parsed = super::super::parse::parse(&usage, Path::new("/s/usage.jsonl"), &[]);
        let got = file_verdict(&parsed, &logs);
        assert_eq!((got.no_verdict, got.no_fh), (3, 1));
    }

    fn arms(c: (usize, usize), u: (usize, usize), changed: usize, none: usize) -> FileVerdict {
        use super::super::retire_split::{Arm, RetireSplit};
        FileVerdict {
            changed,
            unchanged: 100,
            no_verdict: none,
            no_fh: 0,
            retire: RetireSplit {
                changed: Arm {
                    pairs: c.0,
                    retired: c.1,
                },
                unchanged: Arm {
                    pairs: u.0,
                    retired: u.1,
                },
            },
        }
    }

    #[test]
    fn the_gate_waits_for_thirty_pairs_on_each_side() {
        assert_eq!(arms((29, 29), (30, 0), 10, 0).gate(), None);
        assert_eq!(arms((30, 30), (29, 0), 10, 0).gate(), None);
        assert_eq!(arms((30, 30), (30, 0), 10, 0).gate(), Some(true));
    }

    #[test]
    fn the_gate_needs_twice_the_unchanged_retire_rate() {
        // 20/40 = 50% against 10/40 = 25%: exactly 2x passes, a hair under fails
        assert_eq!(arms((40, 20), (40, 10), 10, 0).gate(), Some(true));
        assert_eq!(arms((40, 19), (40, 10), 10, 0).gate(), Some(false));
        // nothing retired on either side is no lift, not an infinite one
        assert_eq!(arms((40, 0), (40, 0), 10, 0).gate(), Some(false));
    }

    #[test]
    fn the_gate_caps_the_label_at_a_third_of_shown_pairs() {
        // shown = changed + 100 unchanged + none: 50 / 150 is exactly a third
        assert_eq!(arms((40, 40), (40, 0), 50, 0).gate(), Some(true));
        assert_eq!(arms((40, 40), (40, 0), 51, 0).gate(), Some(false));
        // pairs with no verdict still count as shown
        assert_eq!(arms((40, 40), (40, 0), 51, 3).gate(), Some(true));
    }
}
