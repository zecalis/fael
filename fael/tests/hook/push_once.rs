//! Push once per (session_id, row_id) (c3): the dedupe key is the row id
//! in the session's seen file — never text, file, or query. Three edits of
//! the same file in one session push the body once; a process restart inside
//! the same session counts as already-pushed (every `hook edit` below is its
//! own OS process, so silence proves the file-backed seen list survived the
//! "restart"); a new session still gets the row; a budget cut keeps its keys
//! for a later push. No ledger mutation carries exposure: only the seen file
//! (per session) and usage rows say what was handed over.

use super::{fael, fael_at_env, json, repo};
use std::path::Path;

/// One edit push of `file` in `session`: its own OS process, like production.
fn edit(d: &Path, session: &str, file: &str) -> (bool, String) {
    let input = format!(
        r#"{{"cwd":{},"session":{},"files":[{}]}}"#,
        json(d),
        serde_json::Value::String(session.into()),
        serde_json::Value::String(file.into()),
    );
    let (ok, out, _) = fael(d, &["hook", "edit"], &input);
    (ok, out)
}

/// Usage rows of this test's state dir with `event`.
fn usage_events(d: &Path, event: &str) -> Vec<serde_json::Value> {
    let text = std::fs::read_to_string(d.join("state/usage.jsonl")).unwrap_or_default();
    text.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .filter(|v: &serde_json::Value| v["event"] == event)
        .collect()
}

/// Three edits of the same file in one session push the row body once: the
/// first carries it, the rest are silent of it, and usage holds one push.
#[test]
fn three_edits_push_the_body_once() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "issue", "kept login loops", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let (ok, first) = edit(&d, "s1", "src/a.rs");
    assert!(ok, "{first}");
    assert!(first.contains("kept login loops"), "{first}");
    for (i, out) in [edit(&d, "s1", "src/a.rs"), edit(&d, "s1", "src/a.rs")]
        .iter()
        .enumerate()
    {
        assert!(out.0, "edit {}: {}", i + 2, out.1);
        assert!(!out.1.contains("kept login loops"), "edit {}: {}", i + 2, out.1);
    }
    // one push carried the id; the later edits only marked it in context
    let pushed: Vec<_> = usage_events(&d, "edit")
        .into_iter()
        .filter(|v| {
            v["ids"].as_array().is_some_and(|ids| {
                ids.iter().any(|id| id.as_str().is_some_and(|s| !s.is_empty()))
            })
        })
        .collect();
    assert_eq!(pushed.len(), 1, "{pushed:?}");
    let (ok, out, err) = fael_at_env(
        &d.join("state"),
        &d,
        &["stats", "--day", "--json"],
        "",
        &[("FAEL_TZ_OFFSET", "Z")],
    );
    assert!(ok, "{out}{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["all"]["delivered"]["rows"], 1, "{v}");
}

/// Crash boundary: the first push's process is gone before the second runs,
/// yet the same session stays silent — already-pushed survives the restart.
#[test]
fn restart_in_session_counts_as_pushed() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "issue", "survives restart", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let (ok, first) = edit(&d, "crash", "src/a.rs");
    assert!(ok && first.contains("survives restart"), "{first}");
    // no in-process memory carries over: this is a fresh process, and the
    // row stays silent because the seen file persisted the first push
    let (ok, second) = edit(&d, "crash", "src/a.rs");
    assert!(ok, "{second}");
    assert!(!second.contains("survives restart"), "{second}");
}

/// Cross-session visibility: a new session gets the row the old one saw.
#[test]
fn two_sessions_both_get_the_row() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "issue", "shared row", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let (ok, s1) = edit(&d, "s1", "src/a.rs");
    assert!(ok && s1.contains("shared row"), "{s1}");
    let (ok, s2) = edit(&d, "s2", "src/a.rs");
    assert!(ok && s2.contains("shared row"), "{s2}");
    let (ok, again) = edit(&d, "s1", "src/a.rs");
    assert!(ok && !again.contains("shared row"), "{again}");
}

/// A budget cut keeps its keys: rows that did not fit push later, each
/// exactly once across the session's edits.
#[test]
fn budget_cut_rows_push_later_exactly_once() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::write(d.join(".fael/config.toml"), "[budget]\npush_rows = 2\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "issue", "capped issue", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    for text in ["capped one", "capped two"] {
        let (ok, _, err) = fael(
            &d,
            &["add", "decision", text, "--files", "src/a.rs", "--force"],
            "",
        );
        assert!(ok, "{err}");
    }
    let outs: Vec<String> = ["k1", "k1", "k1"]
        .iter()
        .map(|s| {
            let (ok, out) = edit(&d, s, "src/a.rs");
            assert!(ok, "{out}");
            let v: serde_json::Value = serde_json::from_str(&out).unwrap();
            v["context"].as_str().unwrap_or_default().to_string()
        })
        .collect();
    let all = outs.join("\n");
    // the issue always shows first; every row lands exactly once in total
    assert!(outs[0].contains("capped issue"), "{}", outs[0]);
    assert!(!outs[1].contains("capped issue"), "{}", outs[1]);
    for text in ["capped issue", "capped one", "capped two"] {
        let rows = all
            .lines()
            .filter(|l| l.starts_with("- [") && l.contains(text))
            .count();
        assert_eq!(rows, 1, "{text}: {all}");
    }
    assert!(!outs[2].contains("fael mem for"), "{}", outs[2]);
}
