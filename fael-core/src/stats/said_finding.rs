//! Yield of the `finding` line (PLAN-fael-experience-loop chunk 2), apart
//! from `said.rs` to keep it under the file-size limit.

use super::said::{tests::rows, yields};
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
