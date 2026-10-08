//! Yield tests apart from `said.rs`, to keep it under the file-size limit:
//! the `count` line's call, `finding` (PLAN-fael-experience-loop chunk 2) and
//! `carry` (chunk 6).

use super::said::{Pull, counted, tests::rows, yields};
use crate::Log;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A finding earns only on an issue naming its file, filed by the same
/// session after the line: not before it, not by another session, not on
/// another file, not a decision.
#[test]
fn a_finding_earns_only_on_the_sessions_later_issue_on_its_file() {
    let row = |id: &str, ts: &str, kind: &str, file: &str, session: &str| {
        format!(
            "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"2026-09-26T{ts}Z\",\"by\":\"w\",\"kind\":\"{kind}\",\"text\":\"t\",\"files\":[\"{file}\"],\"session\":\"{session}\"}}\n"
        )
    };
    let usage = |file: &str| {
        format!(
            "{{\"ts\":\"2026-09-26T00:05:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"/t/s1.jsonl\",\"event\":\"review\",\"ids\":[],\"said\":[{{\"kind\":\"finding\",\"key\":\"{file}\"}}]}}\n"
        )
    };
    let earned = |body: String, file: &str| {
        let log = Log {
            rows: rows(&body),
            ..Log::default()
        };
        let p = super::parse::parse(
            &usage(file),
            Path::new("/w/state/usage.jsonl"),
            &[PathBuf::from("/tmp")],
        );
        let y = yields(&p, &HashMap::from([("/w/r".to_string(), log)]));
        (y["finding"].said, y["finding"].earned)
    };
    let ok = row("A", "00:06:00", "issue", "a.rs", "s1");
    assert_eq!(earned(ok, "a.rs"), (1, 1));
    let cases = [
        (
            "before the line",
            row("A", "00:04:00", "issue", "a.rs", "s1"),
        ),
        (
            "another session",
            row("A", "00:06:00", "issue", "a.rs", "s2"),
        ),
        ("another file", row("A", "00:06:00", "issue", "b.rs", "s1")),
        ("a decision", row("A", "00:06:00", "decision", "a.rs", "s1")),
    ];
    for (why, body) in cases {
        assert_eq!(earned(body, "a.rs"), (1, 0), "{why}");
    }
}

#[test]
fn a_count_line_earns_on_the_call_it_printed() {
    let pull = |files: &[&'static str], key: Option<&'static str>| Pull {
        ms: 0,
        key,
        files: files.to_vec(),
        id: None,
    };
    let files = |p: &Pull| counted("src/a.rs,src/b.rs|file", p);
    assert!(files(&pull(&["src/b.rs"], None)), "a file it named");
    assert!(!files(&pull(&["lib/"], None)), "another directory");
    assert!(
        !files(&pull(&["src/a"], None)),
        "a prefix that is no directory"
    );
    assert!(!files(&pull(&[], Some("k:a"))), "a key pull");
    let dir = |p: &Pull| counted("src/a.rs|dir:src/", p);
    assert!(dir(&pull(&["src/"], None)), "`+N more in src/`");
    assert!(dir(&pull(&["src"], None)), "the same call, normalised");
    let key = |p: &Pull| counted("src/a.rs|key:auth:session", p);
    assert!(key(&pull(&[], Some("auth:session"))), "`+N more with #key`");
    assert!(!key(&pull(&[], Some("auth:other"))), "a key it never named");
    assert!(!key(&pull(&["src/a.rs"], None)), "a file pull");
    let keys = |p: &Pull| counted("src/a.rs|keys", p);
    assert!(keys(&pull(&[], Some("any:key"))), "`+N more under 3 keys`");
}

/// A carry line earns on the session's later `fael find` of the closed issue
/// (a short id is a prefix) or a row superseding it within a day — not on a
/// pull before the line, nor on a pull of another id.
#[test]
fn a_carry_line_earns_on_a_later_find_or_supersede() {
    let usage = |pulls: &str| {
        format!(
            "{{\"ts\":\"2026-09-26T00:05:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"/t/s1.jsonl\",\"event\":\"edit\",\"ids\":[],\"said\":[{{\"kind\":\"carry\",\"key\":\"01ABCDEFGH\"}}]}}\n{pulls}"
        )
    };
    let pull = |min: u8, id: &str| {
        format!(
            "{{\"ts\":\"2026-09-26T00:0{min}:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"s1\",\"event\":\"find\",\"found\":[],\"q\":{{\"id\":\"{id}\"}}}}\n"
        )
    };
    let earned = |pulls: String, rows_: &str| {
        let log = Log {
            rows: rows(rows_),
            ..Log::default()
        };
        let p = super::parse::parse(
            &usage(&pulls),
            Path::new("/w/state/usage.jsonl"),
            &[PathBuf::from("/tmp")],
        );
        let y = yields(&p, &HashMap::from([("/w/r".to_string(), log)]));
        (y["carry"].said, y["carry"].earned)
    };
    assert_eq!(earned(pull(6, "01ABCDEF"), ""), (1, 1), "short id after");
    assert_eq!(earned(pull(4, "01ABCDEF"), ""), (1, 0), "before the line");
    assert_eq!(earned(pull(6, "01ZZZZZZ"), ""), (1, 0), "another id");
    let again = "{\"v\":1,\"id\":\"B\",\"ts\":\"2026-09-26T00:07:00Z\",\"by\":\"w\",\"kind\":\"issue\",\"text\":\"t\",\"files\":[\"a.rs\"],\"supersedes\":\"01ABCDEFGH\"}\n";
    assert_eq!(earned(String::new(), again), (1, 1), "came back, re-filed");
}
