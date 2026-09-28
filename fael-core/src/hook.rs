//! The stop decision, written once for every adapter: block the session that
//! edited files or made commits (or announced a bug) without recording a mem row.
//! Edits are the primary signal — many agents are told never to commit.
//! Pure — git and transcript reads live in the adapter (`fael/src/hook.rs`).

use crate::Log;
use crate::id::ts_ms;

/// A bug-announcement match from the transcript tail or the turn text:
/// Strong (confirmed-bug words) blocks without an issue row; Weak (risk
/// words) never blocks — it rides along as a one-line note, or the adapter
/// stashes it for the next push when there is no work block to join.
#[derive(Clone, Debug)]
pub struct BugSignal {
    pub marker: String,
    pub strong: bool,
    /// ms of the matched transcript line (or the session start for free text)
    pub at_ms: i64,
}

/// What the adapter learned about this turn.
pub struct StopFacts {
    /// The client already fired this hook once — letting through avoids a loop.
    pub stop_active: bool,
    /// Repo-relative files edited after the session's last row (or since the
    /// start when it has none), first-seen order — already filtered, so
    /// `new_row` does not excuse them.
    pub edits: Vec<String>,
    /// `git log --format=%h %s` since the session start — only filled when
    /// `edits` is empty (fallback for edits made outside the edit hook).
    pub commits: Vec<String>,
    /// Any add or close row stamped at or after the session start.
    pub new_row: bool,
    /// The repo has a `.fael/log` at all — a repo that never adopted fael
    /// fails open instead of blocking every turn.
    pub has_log: bool,
    /// A bug-announcement phrase from the transcript tail, if any.
    pub bug_signal: Option<BugSignal>,
    /// An `issue` row exists at or after the match (`at_ms` above) — an issue
    /// filed before the words does not clear them.
    pub bug_row_since: bool,
}

/// The block reason, or `None` to let the turn end. Never fails — every
/// unknown is an allow, because a memory tool must not break the session.
pub fn decide_stop(f: &StopFacts) -> Option<String> {
    if f.stop_active {
        return None;
    }
    // Bug rule first: independent of commits (a reported bug with no row is
    // lost when the room closes, whatever else the turn did). Only a Strong
    // signal blocks — a Weak one joins the work block below, or the adapter
    // stashes it for the next push when there is nothing to join.
    // Chunk 6c: the command carries the actual files — runnable as printed, no
    // `find` first (falls back to the placeholder only with no edit signal).
    if let Some(sig) = &f.bug_signal
        && sig.strong
        && !f.bug_row_since
    {
        let files = if f.edits.is_empty() {
            "<files>".to_string()
        } else {
            f.edits.join(",")
        };
        return Some(format!(
            "This turn reported a problem (\"{}\") but no issue row exists for this session.\n\
             Record it before ending: fael add issue \"<what is broken or at risk>\" --files {files}\n\
             Already filed, or not a problem? End the turn again — this fires once per session.",
            sig.marker
        ));
    }
    // Work rule: files edited since the last row (or, failing that, commits
    // with no row at all this session).
    if !f.has_log || (f.edits.is_empty() && (f.commits.is_empty() || f.new_row)) {
        return None;
    }
    // markdown like render(): one `- ` line per item, --files prefilled
    let (what, items, files) = if f.edits.is_empty() {
        (
            format!("{} commit(s)", f.commits.len()),
            &f.commits,
            "<files>".to_string(),
        )
    } else {
        (
            format!("{} file(s) edited", f.edits.len()),
            &f.edits,
            f.edits.join(","),
        )
    };
    let mut out = vec![format!(
        "{what} this session with no mem row for this work:"
    )];
    out.extend(items.iter().take(10).map(|c| format!("- {c}")));
    if items.len() > 10 {
        out.push(format!("- … +{} more", items.len() - 10));
    }
    out.push(format!(
        "Record one before ending: fael add <decision|issue|note> \"<what happened>\" --files {files}"
    ));
    // a Weak signal joins the single work block — one block, not two
    if let Some(sig) = &f.bug_signal
        && !sig.strong
        && !f.bug_row_since
    {
        out.push(format!(
            "Also, this turn mentioned a possible problem (\"{}\") — file an issue too if it holds up: fael add issue \"<what is at risk>\" --files {files}",
            sig.marker
        ));
        // chunk 6c: two rows to file — one batch call instead of two turns
        out.push(
            "Filing both? Batch them: fael add --json - < rows.json (a JSON array of rows)".into(),
        );
    }
    out.push("Nothing worth recording? End the turn again — this fires once per session.".into());
    Some(out.join("\n"))
}

/// The newest add or close row stamped at or after `since_ms`, in ms. Numeric
/// on both sides — a whole-second string compare reads a row filed just
/// before the session start as newer whenever they share a second.
pub fn last_row_ms(log: &Log, since_ms: i64) -> Option<i64> {
    log.rows
        .iter()
        .chain(log.closes.iter())
        .filter_map(|r| ts_ms(&r.ts))
        .filter(|&ms| ms >= since_ms)
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Row;

    fn facts() -> StopFacts {
        StopFacts {
            stop_active: false,
            edits: vec![],
            commits: vec!["abc123 fix login".into()],
            new_row: false,
            has_log: true,
            bug_signal: None,
            bug_row_since: false,
        }
    }

    #[test]
    fn allows_everything_unknown() {
        assert!(
            decide_stop(&StopFacts {
                stop_active: true,
                ..facts()
            })
            .is_none()
        );
        assert!(
            decide_stop(&StopFacts {
                commits: vec![],
                ..facts()
            })
            .is_none()
        );
        assert!(
            decide_stop(&StopFacts {
                has_log: false,
                ..facts()
            })
            .is_none()
        );
        assert!(
            decide_stop(&StopFacts {
                new_row: true,
                ..facts()
            })
            .is_none()
        );
    }

    #[test]
    fn blocks_commit_without_row() {
        let r = decide_stop(&facts()).unwrap();
        assert!(r.contains("1 commit(s)"), "{r}");
        assert!(r.contains("fael add <decision|issue|note>"), "{r}");
    }

    #[test]
    fn edits_win_over_commits_and_prefill_files() {
        let r = decide_stop(&StopFacts {
            edits: vec!["src/a.rs".into(), "src/b.rs".into()],
            ..facts()
        })
        .unwrap();
        assert!(
            r.contains("2 file(s) edited") && r.contains("\n- src/a.rs"),
            "{r}"
        );
        assert!(
            r.contains("--files src/a.rs,src/b.rs") && !r.contains("abc123"),
            "{r}"
        );
        // edits arrive filtered to after the last row — a row earlier in the
        // session does not excuse them
        assert!(
            decide_stop(&StopFacts {
                edits: vec!["a".into()],
                new_row: true,
                ..facts()
            })
            .is_some()
        );
    }

    #[test]
    fn bug_rule_needs_no_commit() {
        let strong = || BugSignal {
            marker: "found a bug".into(),
            strong: true,
            at_ms: 0,
        };
        let r = decide_stop(&StopFacts {
            commits: vec![],
            bug_signal: Some(strong()),
            ..facts()
        })
        .unwrap();
        assert!(r.contains("fael add issue"), "{r}");
        assert!(
            decide_stop(&StopFacts {
                commits: vec![],
                bug_signal: Some(strong()),
                bug_row_since: true,
                ..facts()
            })
            .is_none()
        );
    }

    #[test]
    fn weak_joins_work_block_never_alone() {
        let weak = || BugSignal {
            marker: "out of sync".into(),
            strong: false,
            at_ms: 0,
        };
        // no work: no block (the adapter stashes the note for the next push)
        assert!(
            decide_stop(&StopFacts {
                commits: vec![],
                bug_signal: Some(weak()),
                ..facts()
            })
            .is_none()
        );
        // work: one block carrying both the files and the risk
        let r = decide_stop(&StopFacts {
            edits: vec!["src/a.rs".into()],
            bug_signal: Some(weak()),
            ..facts()
        })
        .unwrap();
        assert!(
            r.contains("1 file(s) edited") && r.contains("out of sync"),
            "{r}"
        );
        // an issue since the match quiets the note, not the work
        let r = decide_stop(&StopFacts {
            edits: vec!["src/a.rs".into()],
            bug_signal: Some(weak()),
            bug_row_since: true,
            ..facts()
        })
        .unwrap();
        assert!(!r.contains("out of sync"), "{r}");
    }

    #[test]
    fn new_row_compares_ms_not_seconds() {
        let mut log = Log::default();
        let mut r = Row::new("t-0000", "note", "x", vec!["a.rs".into()]);
        r.ts = "2026-09-25T10:00:01Z".into();
        log.rows.push(r);
        assert!(last_row_ms(&log, ts_ms("2026-09-25T10:00:01.500Z").unwrap()).is_none());
        assert!(last_row_ms(&log, ts_ms("2026-09-25T10:00:01Z").unwrap()).is_some());
    }
}
