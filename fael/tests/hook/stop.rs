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

/// A Claude stop sums the transcript once: three assistant messages (one of
/// them split over two lines) make one `session-tokens` line, the next stop
/// writes a fresh total, and `stats --session` prints the last one.
#[test]
fn stop_writes_the_transcript_total_once_per_stop() {
    let d = repo();
    let asst = |id: &str, i: u64, o: u64| {
        format!(
            r#"{{"type":"assistant","message":{{"id":"{id}","usage":{{"input_tokens":{i},"cache_creation_input_tokens":10,"cache_read_input_tokens":100,"output_tokens":{o}}}}}}}"#
        )
    };
    let t = d.join("sess-t1.jsonl");
    let body = [
        asst("a", 1, 5),
        asst("a", 1, 5),
        asst("b", 2, 6),
        asst("c", 3, 7),
    ];
    std::fs::write(&t, body.join("\n") + "\n").unwrap();
    let input = format!(r#"{{"cwd":{},"transcript_path":{}}}"#, json(&d), json(&t));
    let tokens = || -> Vec<serde_json::Value> {
        std::fs::read_to_string(d.join("state/usage.jsonl"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
            .filter(|l| l["event"] == "session-tokens")
            .collect()
    };
    let (ok, _, err) = fael(&d, &["hook", "stop", "--client", "claude"], &input);
    assert!(ok, "{err}");
    let rows = tokens();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["session"], "sess-t1");
    assert_eq!(
        rows[0]["real_tokens"],
        serde_json::json!({"input_tokens": 6, "cache_creation_input_tokens": 30,
            "cache_read_input_tokens": 300, "output_tokens": 18})
    );
    // a later stop sums the grown transcript; stats reads the last line
    std::fs::write(&t, body.join("\n") + "\n" + &asst("d", 4, 8) + "\n").unwrap();
    assert!(fael(&d, &["hook", "stop", "--client", "claude"], &input).0);
    assert_eq!(tokens().len(), 2);
    let repo_key = serde_json::to_string(&rows[0]["repo"]).unwrap();
    let edit = format!(
        r#"{{"ts":"2026-10-02T00:00:00.000Z","repo":{},"client":"claude","session":"sess-t1","event":"edit","bytes":9,"est_tokens":4,"ids":[],"files":["a.rs"]}}"#,
        repo_key
    );
    let usage = d.join("state/usage.jsonl");
    let mut text = std::fs::read_to_string(&usage).unwrap();
    text.push_str(&(edit + "\n"));
    std::fs::write(&usage, text).unwrap();
    let (ok, out, err) =
        super::fael_at(&d.join("state"), &d, &["stats", "--session", "sess-t1"], "");
    assert!(ok, "{err}");
    assert!(
        out.contains("session tokens: in 10 · cache 440 · out 26"),
        "{out}"
    );
    assert!(
        out.contains("fael pushed: 4 est tokens (0.89% of input)"),
        "{out}"
    );
}
