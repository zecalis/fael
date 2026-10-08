//! PLAN-fael-experience-loop chunk 2: a ReportFindings call says each
//! finding once as a ready `fael add issue`, five per session, and its
//! usage line names the files offered.

use super::{fael, repo};
use std::path::Path;

fn report(d: &Path, session: &str, findings: &[(&str, i64)]) -> String {
    let list: Vec<_> = findings
        .iter()
        .map(|(f, l)| serde_json::json!({"file": f, "line": l, "summary": format!("{f} \"breaks\" on retry"), "failure_scenario": "x"}))
        .collect();
    let input = serde_json::json!({"cwd": d, "session_id": session, "tool_name": "ReportFindings", "tool_input": {"findings": list}});
    let (ok, out, err) = fael(
        d,
        &["hook", "review", "--client", "claude"],
        &input.to_string(),
    );
    assert!(ok, "{err}");
    serde_json::from_str::<serde_json::Value>(&out)
        .ok()
        .and_then(|v| {
            v["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .map(String::from)
        })
        .unwrap_or_default()
}

#[test]
fn each_finding_is_a_ready_add_issue_said_once() {
    let d = repo();
    let first = report(&d, "s1", &[("src/a.rs", 3), ("src/b.rs", 9)]);
    assert!(
        first.contains("`fael add issue \"src/a.rs breaks on retry\" --files src/a.rs`")
            && first.contains("--files src/b.rs`"),
        "{first}"
    );
    // the same findings again are silent; a new line in the same file is not
    assert_eq!(report(&d, "s1", &[("src/a.rs", 3)]), "");
    assert!(report(&d, "s1", &[("src/a.rs", 4)]).contains("--files src/a.rs`"));
    // another session is told again
    assert!(report(&d, "s2", &[("src/a.rs", 3)]).contains("--files src/a.rs`"));
    let usage = std::fs::read_to_string(d.join("state/usage.jsonl")).unwrap();
    assert!(
        usage.contains(r#""event":"review""#)
            && usage.contains(r#"{"kind":"finding","key":"src/b.rs"}"#),
        "{usage}"
    );
}

#[test]
fn five_findings_per_session_and_nothing_for_an_empty_report() {
    let d = repo();
    let files: Vec<String> = (0..8).map(|i| format!("src/f{i}.rs")).collect();
    let call: Vec<(&str, i64)> = files.iter().map(|f| (f.as_str(), 1)).collect();
    let said = report(&d, "s1", &call);
    assert_eq!(said.matches("fael add issue").count(), 5, "{said}");
    assert_eq!(
        report(&d, "s1", &call[5..]),
        "",
        "cap spent for the session"
    );
    assert_eq!(report(&d, "s3", &[]), "");
}

/// No session id: nothing remembered, so every call says again, still five at most.
#[test]
fn no_session_says_every_call_but_five_at_most() {
    let d = repo();
    let files: Vec<String> = (0..8).map(|i| format!("src/f{i}.rs")).collect();
    let call: Vec<(&str, i64)> = files.iter().map(|f| (f.as_str(), 1)).collect();
    for _ in 0..2 {
        assert_eq!(report(&d, "", &call).matches("fael add issue").count(), 5);
    }
}

/// A finding already offered takes no slot of the cap: the fresh ones behind it still come.
#[test]
fn an_offered_finding_does_not_use_up_the_cap() {
    let d = repo();
    report(
        &d,
        "s1",
        &[("src/a.rs", 1), ("src/b.rs", 1), ("src/c.rs", 1)],
    );
    let again = report(
        &d,
        "s1",
        &[
            ("src/a.rs", 1),
            ("src/b.rs", 1),
            ("src/c.rs", 1),
            ("src/d.rs", 1),
            ("src/e.rs", 1),
        ],
    );
    assert_eq!(again.matches("fael add issue").count(), 2, "{again}");
    assert!(
        again.contains("src/d.rs") && again.contains("src/e.rs"),
        "{again}"
    );
}
