//! Tests apart from `sessions.rs`, to keep it under the file-size limit.

use super::sessions::*;
use crate::stats::FlowRun;
use crate::stats::outcome::ListedPr;
use crate::stats::parse::{Parsed, parse};
use crate::ts_ms;
use std::collections::HashMap;
use std::path::Path;

fn line(ts: &str, session: &str, rest: &str) -> String {
    format!(r#"{{"ts":"{ts}","repo":"/w/r","client":"claude","session":"{session}",{rest}}}"#)
}

/// s1: pushed row A, edited its file (a hit), one ok and one rejected
/// call; s2: edited, was handed nothing.
fn fixture() -> Parsed {
    let lines = [
        line(
            "2026-10-01T00:00:00.000Z",
            "s0",
            r#""event":"in-context","in_context":["Z"],"files":[]"#,
        ),
        line(
            "2026-10-02T00:00:00.000Z",
            "s1",
            r#""event":"session-start","bytes":9,"est_tokens":3,"ids":["A"]"#,
        ),
        line(
            "2026-10-02T00:01:00.000Z",
            "s1",
            r#""event":"edit","bytes":9,"est_tokens":4,"ids":[],"branch":"feat/x","files":["a.rs"]"#,
        ),
        line(
            "2026-10-02T00:01:00.000Z",
            "s1",
            r#""event":"in-context","in_context":["A"],"files":["a.rs"]"#,
        ),
        line(
            "2026-10-02T00:02:00.000Z",
            "s1",
            r#""event":"call","cmd":"add","outcome":"ok""#,
        ),
        line(
            "2026-10-02T00:03:00.000Z",
            "s1",
            r#""event":"call","cmd":"add","outcome":"reject","reason":"x""#,
        ),
        line(
            "2026-10-02T00:03:30.000Z",
            "s1",
            r#""event":"call","cmd":"add","outcome":"reject","reason":"shape_gate""#,
        ),
        line(
            "2026-10-02T00:04:00.000Z",
            "s1",
            r#""event":"call","cmd":"add","outcome":"ok","who":"human""#,
        ),
        line(
            "2026-10-03T00:00:00.000Z",
            "s2",
            r#""event":"edit","bytes":0,"est_tokens":0,"ids":[]"#,
        ),
    ];
    parse(&lines.join("\n"), Path::new("/state"), &[])
}

#[test]
fn a_session_row_reads_the_aggregates_rules() {
    let run = FlowRun {
        session: Some("/p/s2.jsonl".into()),
        ..Default::default()
    };
    let (list, sp) = sessions(&fixture(), &HashMap::new(), &[run], &[]);
    let s1 = list.iter().find(|v| v.session == "s1").unwrap();
    assert_eq!((s1.shown, s1.scored, s1.in_context, s1.acted), (1, 1, 1, 1));
    assert_eq!((s1.edits, s1.calls, s1.rejects), (1, 2, 1));
    assert_eq!(s1.pushed_tokens["session-start"], 3);
    assert_eq!(s1.branch.as_deref(), Some("feat/x"));
    assert_eq!(s1.first, "2026-10-02T00:00:00.000Z");
    assert_eq!(s1.rows_filed, None); // no log loaded
    // newest first
    assert_eq!(list[0].session, "s2");
    assert!(list[0].in_run && !s1.in_run);
    assert_eq!(
        (sp.pushed.with.sessions, sp.pushed.without.sessions),
        (1, 1)
    );
    assert_eq!(
        (sp.in_run.with.sessions, sp.in_run.without.sessions),
        (1, 1)
    );
    assert_eq!(sp.pushed.with.rejects, 1);
}

#[test]
fn two_sessions_on_one_branch_both_show_its_pr() {
    let mut p = ListedPr {
        number: 7,
        branch: Some("feat/x".into()),
        ..Default::default()
    };
    let (list, _) = sessions(&fixture(), &HashMap::new(), &[], std::slice::from_ref(&p));
    let s1 = list.iter().find(|v| v.session == "s1").unwrap();
    assert_eq!(
        s1.pr,
        Some(SessionPr {
            number: 7,
            merged: None
        })
    );
    assert!(
        list.iter()
            .find(|v| v.session == "s2")
            .unwrap()
            .pr
            .is_none()
    );
    p.merged_ms = ts_ms("2026-10-05T00:00:00Z");
    let (list, sp) = sessions(&fixture(), &HashMap::new(), &[], &[p]);
    let s1 = list.iter().find(|v| v.session == "s1").unwrap();
    assert_eq!(
        s1.pr.as_ref().unwrap().merged.as_deref(),
        Some("2026-10-05")
    );
    assert_eq!(sp.pushed.with.merged_pr, 1);
}

#[test]
fn a_unique_prefix_finds_one_and_an_ambiguous_one_names_the_stems() {
    let (list, _) = sessions(&fixture(), &HashMap::new(), &[], &[]);
    assert_eq!(find_session(&list, "s1").unwrap().len(), 1);
    assert_eq!(
        find_session(&list, "/h/x/s2.jsonl").unwrap()[0].session,
        "s2"
    );
    assert!(find_session(&list, "s").unwrap_err().contains("s0, s1, s2"));
    assert!(
        find_session(&list, "zz")
            .unwrap_err()
            .contains("no session")
    );
}
