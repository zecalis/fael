//! The file-hash verdict at push (01M43517 part 3): of the rows a read push
//! showed, how many got a verdict (`changed` / `unchanged`) and how many got
//! none. The hook records both lists on the usage line (`hook/usage.rs`
//! `record_usage_shadow`); a row in neither had no stamp, sits on a file over
//! the push cap, or the file is gone — the count cannot say which. A count of
//! what push could not say, never of what it saved. Pure: parsed usage in.

use super::parse::Parsed;
use serde::Serialize;
use std::collections::HashSet;

/// (session, row) pairs shown by a push that measured the verdict, by
/// outcome. `changed + unchanged + no_verdict` is every measured pair.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FileVerdict {
    /// The row's files changed since it was written.
    pub changed: usize,
    /// The row's files all still match.
    pub unchanged: usize,
    /// Shown, in neither list: no `fh` stamp, a stamped file over the push cap
    /// or gone — the reasons are not split.
    pub no_verdict: usize,
}

/// Read the shadow keys off the usage lines that carry them. A line from
/// before the keys existed has no `changed` array: not measured, so never
/// counted as `no_verdict`. Each (repo, session, row) pair counts once, for
/// the first measured line that showed it — the `value.by_event` key; a line
/// with no session is left out (a pair needs one).
// ponytail: file order is time order (usage.jsonl is append-only)
pub(super) fn file_verdict(parsed: &Parsed) -> FileVerdict {
    let mut seen: HashSet<(&str, &str, &str)> = HashSet::new();
    let mut out = FileVerdict::default();
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
        for id in list("ids") {
            if !seen.insert((repo, session, id)) {
                continue;
            }
            if changed.contains(id) {
                out.changed += 1;
            } else if unchanged.contains(id) {
                out.unchanged += 1;
            } else {
                out.no_verdict += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{FileVerdict, file_verdict};
    use std::path::Path;

    fn count(usage: &str) -> FileVerdict {
        let parsed = super::super::parse::parse(usage, Path::new("/s/usage.jsonl"), &[]);
        file_verdict(&parsed)
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
}
