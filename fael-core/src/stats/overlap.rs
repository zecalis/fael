//! Sessions editing one file at about the same time (01M411WF step 2),
//! counted from the `files` that edit usage lines carry. Deterministic only:
//! two distinct sessions, one repo path, one file, edit lines no further apart
//! than `OVERLAP_WINDOW_MS`. Never a verdict that anyone collided, and never a guess at
//! a session whose lines name no file. Pure: parsed usage in.

use super::cross::stem;
use super::parse::Parsed;
use crate::ts_ms;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet};

/// Two sessions count as concurrent on a file when each has an edit line for
/// it and the two lines are at most this far apart: 10 minutes, a short window
/// on purpose (a session idle longer is not shown to be at the file).
pub const OVERLAP_WINDOW_MS: i64 = 10 * 60 * 1000;

/// Usage lines written when the agent edits: an `edit` push, the shell-call
/// label of one, and the `in-context` line the same edit writes.
const EDIT_EVENTS: [&str; 3] = ["edit", "shell-edit", "in-context"];

/// Counted over usage lines that carry `files` — lines from before they were
/// recorded, and edits where fael said nothing and found nothing in context
/// (no line is written), are invisible here, so every number is a lower bound.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SameFile {
    /// Distinct sessions with at least one edit line naming a file — the
    /// base the other two read against; 0 means nothing could be counted yet.
    pub sessions_seen: usize,
    /// Distinct (repo path, file) pairs two sessions edited within the window.
    /// Worktrees have their own repo path, so a file edited in two worktrees
    /// is never matched.
    pub files: usize,
    /// Distinct unordered session pairs concurrent on at least one file.
    pub session_pairs: usize,
}

pub(super) fn same_file(parsed: &Parsed) -> SameFile {
    // (repo, file) → (ms, session) per edit line
    let mut edits: HashMap<(&str, &str), Vec<(i64, &str)>> = HashMap::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let edit_lines = parsed.kept.iter().filter(|v| {
        v["event"]
            .as_str()
            .is_some_and(|e| EDIT_EVENTS.contains(&e))
    });
    for v in edit_lines {
        let (Some(repo), Some(session), Some(ms), Some(files)) = (
            v["repo"].as_str(),
            v["session"].as_str(),
            v["ts"].as_str().and_then(ts_ms),
            v["files"].as_array(),
        ) else {
            continue;
        };
        let session = stem(session);
        for f in files.iter().filter_map(|f| f.as_str()) {
            edits.entry((repo, f)).or_default().push((ms, session));
            seen.insert(session);
        }
    }
    let mut out = SameFile {
        sessions_seen: seen.len(),
        ..SameFile::default()
    };
    let mut pairs: BTreeSet<(&str, &str)> = BTreeSet::new();
    for lines in edits.values_mut() {
        lines.sort_unstable();
        let mut hit = false;
        for (i, (t, a)) in lines.iter().enumerate() {
            for (u, b) in &lines[i + 1..] {
                if u - t > OVERLAP_WINDOW_MS {
                    break;
                }
                if a != b {
                    hit = true;
                    pairs.insert((*a.min(b), *a.max(b)));
                }
            }
        }
        out.files += usize::from(hit);
    }
    out.session_pairs = pairs.len();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(ts: &str, event: &str, session: &str, repo: &str, files: &str) -> String {
        format!(
            "{{\"ts\":\"{ts}\",\"repo\":\"{repo}\",\"client\":\"claude\",\"event\":\"{event}\",\"bytes\":9,\"est_tokens\":2,\"ids\":[],\"session\":\"{session}\",\"files\":[{files}]}}\n"
        )
    }

    fn count(usage: &str) -> SameFile {
        let p = super::super::parse::parse(usage, std::path::Path::new("/s/usage.jsonl"), &[]);
        same_file(&p)
    }

    #[test]
    fn two_sessions_on_one_file_within_the_window_count() {
        let a = "\"src/a.rs\"";
        let u = line("2026-10-04T00:00:00.000Z", "edit", "s1", "/w", a)
            + &line("2026-10-04T00:10:00.000Z", "edit", "s2", "/w", a)
            // the same session again: never its own pair
            + &line("2026-10-04T00:10:30.000Z", "edit", "s1", "/w", a);
        let c = count(&u);
        assert_eq!((c.sessions_seen, c.files, c.session_pairs), (2, 1, 1));
    }

    #[test]
    fn the_window_other_files_repos_and_unnamed_lines_stay_out() {
        let bare = |ts: &str, extra: &str| {
            format!(
                "{{\"ts\":\"{ts}\",\"repo\":\"/w\",\"client\":\"claude\",\"event\":\"edit\",\"bytes\":9,\"est_tokens\":2,\"ids\":[]{extra}}}\n"
            )
        };
        // 10 minutes and a second apart: outside
        let u = line("2026-10-04T00:00:00.000Z", "edit", "s1", "/w", "\"a.rs\"")
            + &line("2026-10-04T00:10:01.000Z", "edit", "s2", "/w", "\"a.rs\"")
            // inside the window but another file, and another repo path
            + &line("2026-10-04T00:10:02.000Z", "edit", "s3", "/w", "\"b.rs\"")
            + &line("2026-10-04T00:10:03.000Z", "edit", "s4", "/other", "\"a.rs\"")
            // no files (a line from before they were recorded), no session
            + &bare("2026-10-04T00:10:04.000Z", ",\"session\":\"s5\"")
            + &bare("2026-10-04T00:10:05.000Z", ",\"files\":[\"a.rs\"]")
            // a read names no edit
            + &line("2026-10-04T00:10:06.000Z", "read", "s6", "/w", "\"a.rs\"");
        let c = count(&u);
        assert_eq!((c.sessions_seen, c.files, c.session_pairs), (4, 0, 0));
    }

    #[test]
    fn counts_pairs_files_and_joins_a_claude_path_to_its_stem() {
        // s1 (usage names the transcript path in one line, the stem in another)
        // overlaps s2 on two files and s3 on one; shell edits and in-context
        // lines count as edits
        let u = line(
            "2026-10-04T00:00:00.000Z",
            "edit",
            "/p/s1.jsonl",
            "/w",
            "\"a.rs\",\"b.rs\"",
        ) + &line(
            "2026-10-04T00:01:00.000Z",
            "shell-edit",
            "s2",
            "/w",
            "\"a.rs\",\"b.rs\"",
        ) + &line(
            "2026-10-04T00:02:00.000Z",
            "in-context",
            "s3",
            "/w",
            "\"b.rs\"",
        ) + &line("2026-10-04T00:03:00.000Z", "edit", "s1", "/w", "\"c.rs\"");
        let c = count(&u);
        // a.rs: s1+s2; b.rs: s1+s2, s1+s3, s2+s3; c.rs: s1 alone
        assert_eq!((c.sessions_seen, c.files, c.session_pairs), (3, 2, 3));
    }
}
