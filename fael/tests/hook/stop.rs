//! Stop-event bug rule: an announcement with no issue row since is flagged
//! on the next push, never a block — and fail-open on garbage.

use super::{fael, flagged, json, repo};

#[test]
fn stop_bug_signal_needs_issue_row() {
    let d = repo();
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");

    let t = d.join("t.jsonl");
    std::fs::write(
        &t,
        r#"{"message":{"role":"assistant","content":[{"type":"text","text":"I found a bug in login"}]}}"#,
    )
    .unwrap();
    let input = format!(r#"{{"cwd":{},"session":{}}}"#, json(&d), json(&t));
    let (ok, out, _) = fael(&d, &["hook", "stop"], &input);
    assert!(ok && out.contains(r#""block":false"#), "{out}");
    assert!(flagged(&d, &json(&t)));
    // shown once
    assert!(!flagged(&d, &json(&t)));

    // any client: the assistant text arrives in the Event, no transcript needed
    let session = r#""2020-01-01T00:00:00Z""#;
    let neutral = format!(
        r#"{{"cwd":{},"session":{session},"text":"bug confirmed in logout"}}"#,
        json(&d)
    );
    let (ok, out, _) = fael(&d, &["hook", "stop"], &neutral);
    assert!(ok && out.contains(r#""block":false"#), "{out}");
    assert!(flagged(&d, session));

    // an issue row filed in this session clears it
    let (ok, _, err) = fael(
        &d,
        &["add", "issue", "login loops", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    assert!(fael(&d, &["hook", "stop"], &input).0);
    assert!(!flagged(&d, &json(&t)));
}

#[test]
fn stop_fails_open() {
    let d = repo();
    // garbage in, unknown client, no adopted log — all allow, all exit 0
    let (ok, out, _) = fael(&d, &["hook", "stop"], "not json");
    assert!(ok && out.contains(r#""block":false"#), "{out}");
    let (ok, out, _) = fael(&d, &["hook", "stop", "--client", "nope"], "{}");
    assert!(ok, "{out}");
    let (ok, out, _) = fael(
        &d,
        &["hook", "stop", "--client", "claude"],
        r#"{"cwd":"/"}"#,
    );
    assert!(ok && out.is_empty(), "{out}");
}
