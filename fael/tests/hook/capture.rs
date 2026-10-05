//! Capture in reply (PLAN-fael-dev-adoption chunk 1): the Stop hook files the
//! last message's `fael <kind>: … [files: …]` lines, never blocks, and
//! reports the rejects on the next push.

use super::{fael, json, repo, transcript};
use std::path::{Path, PathBuf};

const SESSION: &str = "2020-01-01T00:00:00Z";

/// An adopted repo (one row on file) with `src/a.rs` and `src/b.rs`.
fn adopted(d: PathBuf) -> PathBuf {
    for f in ["src/a.rs", "src/b.rs"] {
        std::fs::write(d.join(f), "//\n").unwrap();
    }
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    d
}

fn stop(d: &Path, extra: &str) -> String {
    let input = format!(r#"{{"cwd":{},"session":"{SESSION}",{extra}}}"#, json(d));
    let (ok, out, err) = fael(d, &["hook", "stop"], &input);
    assert!(ok, "{err}");
    out
}

fn stop_reply(d: &Path, reply: &str) -> String {
    stop(d, &format!(r#""reply":{}"#, serde_json::json!(reply)))
}

fn find(d: &Path, text: &str) -> String {
    fael(d, &["find", text], "").1
}

fn read_push(d: &Path) -> String {
    let input = format!(
        r#"{{"cwd":{},"session":"{SESSION}","files":["src/nothing.rs"]}}"#,
        json(d)
    );
    fael(d, &["hook", "read"], &input).1
}

#[test]
fn reply_lines_are_filed_and_nothing_blocks() {
    let d = adopted(repo());
    let out = stop_reply(
        &d,
        "done, tests pass\n\nfael decision: Cache keys include the tenant id [files: src/a.rs]\nfael issue: Retry loop has no backoff [files: src/b.rs]",
    );
    assert!(out.contains(r#""block":false"#), "{out}");
    assert!(find(&d, "tenant id").contains("Cache keys include"));
    assert!(
        find(&d, "no backoff").contains("[issue]") || find(&d, "no backoff").contains("Retry loop")
    );

    // the same lines again in one session (an idle that fires twice) file once
    stop_reply(
        &d,
        "fael decision: Cache keys include the tenant id [files: src/a.rs]\nfael issue: Retry loop has no backoff [files: src/b.rs]",
    );
    let (_, out, _) = fael(&d, &["stats", "--json"], "");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["capture"]["reply_stored"], 2, "{out}");
    assert_eq!(v["capture"]["reply_rejected"], 0, "{out}");
    assert_eq!(v["capture"]["reply_lines"], 2, "{out}");
}

#[test]
fn only_the_bare_last_message_lines_count() {
    let d = adopted(repo());
    let reply = [
        "```",
        "fael note: UNIQFENCE quoted [files: src/a.rs]",
        "```",
        "see fael note: UNIQMID inside a sentence [files: src/a.rs]",
        "  fael note: UNIQINDENT indented [files: src/a.rs]",
        "fael decisions: UNIQPLURAL wrong word [files: src/a.rs]",
        "Fael note: UNIQCASE wrong case [files: src/a.rs]",
    ]
    .join("\n");
    let out = stop_reply(&d, &reply);
    assert!(out.contains(r#""block":false"#), "{out}");
    for u in [
        "UNIQFENCE",
        "UNIQMID",
        "UNIQINDENT",
        "UNIQPLURAL",
        "UNIQCASE",
    ] {
        assert!(!find(&d, u).contains(u), "{u} was filed");
    }
}

#[test]
fn claude_transcript_files_only_the_final_message() {
    let d = adopted(repo());
    let t = transcript(&d, "t.jsonl");
    let line = |role: &str, text: &str| {
        serde_json::json!({"message": {"role": role, "content": [{"type": "text", "text": text}]}})
            .to_string()
    };
    std::fs::write(
        &t,
        [
            line(
                "assistant",
                "fael note: UNIQEARLY before a tool call [files: src/a.rs]",
            ),
            line("user", "tool result"),
            line(
                "assistant",
                "all done\nfael note: Final handoff line, filed [files: src/b.rs]",
            ),
        ]
        .join("\n"),
    )
    .unwrap();
    let input = format!(r#"{{"cwd":{},"transcript_path":{}}}"#, json(&d), json(&t));
    let (ok, out, _) = fael(&d, &["hook", "stop", "--client", "claude"], &input);
    assert!(ok && !out.contains("block"), "{out}");
    assert!(find(&d, "Final handoff").contains("Final handoff line"));
    assert!(!find(&d, "UNIQEARLY").contains("UNIQEARLY"));

    // the client's own last_assistant_message wins over the transcript
    let input = format!(
        r#"{{"cwd":{},"transcript_path":{},"last_assistant_message":"fael note: From the hook input [files: src/b.rs]"}}"#,
        json(&d),
        json(&t)
    );
    let (ok, out, _) = fael(&d, &["hook", "stop", "--client", "claude"], &input);
    assert!(ok && !out.contains("block"), "{out}");
    assert!(find(&d, "From the hook input").contains("From the hook input"));
}

#[test]
fn rejects_are_not_filed_and_hint_once_on_the_next_push() {
    let d = adopted(repo());
    let secret = format!("AKIA{}", "A".repeat(20));
    let reply = format!(
        "fael note: UNIQNOFILES has no scope\n\
fael note: UNIQEMPTY empty scope [files: ]\n\
fael note: UNIQSECRET {secret} leaked [files: src/a.rs]\n\
fael note: UNIQTYPO wrong path [files: src/aa.rs]"
    );
    let out = stop_reply(&d, &reply);
    assert!(out.contains(r#""block":false"#), "{out}");
    for u in ["UNIQNOFILES", "UNIQEMPTY", "UNIQSECRET", "UNIQTYPO"] {
        assert!(!find(&d, u).contains(u), "{u} was filed");
    }

    let out = read_push(&d);
    assert!(out.contains("4 `fael <kind>:` line(s)"), "{out}");
    assert!(
        !out.contains(&secret),
        "the hint must not echo the line: {out}"
    );
    assert!(!read_push(&d).contains("not filed"), "hint shows once");

    let (_, out, _) = fael(&d, &["stats", "--json"], "");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["capture"]["reply_rejected"], 4, "{out}");
    assert_eq!(v["capture"]["reply_stored"], 0, "{out}");
}

#[test]
fn default_mode_never_returns_a_block() {
    let d = adopted(repo());
    // work: an edit after the last row, no row for it
    let edit = format!(
        r#"{{"cwd":{},"session":"{SESSION}","files":["src/b.rs"]}}"#,
        json(&d)
    );
    assert!(fael(&d, &["hook", "edit"], &edit).0);
    let out = stop(&d, r#""text":"finished""#);
    assert!(out.contains(r#""block":false"#), "work: {out}");

    // bug: a strong announcement with no issue row is a hint, not a block
    let out = stop(&d, r#""text":"bug confirmed in logout""#);
    assert!(out.contains(r#""block":false"#), "bug: {out}");
    assert!(read_push(&d).contains("possible problem"), "bug hint");

    // reject: a malformed capture line
    let out = stop_reply(&d, "fael note: no scope here");
    assert!(out.contains(r#""block":false"#), "reject: {out}");

    // claude adapter: no `decision: block` line on stdout
    let t = transcript(&d, "t.jsonl");
    let input = format!(r#"{{"cwd":{},"transcript_path":{}}}"#, json(&d), json(&t));
    super::commit(&d, "work with no row");
    let (ok, out, _) = fael(&d, &["hook", "stop", "--client", "claude"], &input);
    assert!(ok && !out.contains("block"), "claude: {out}");
}

/// A sub-agent's stop (`agent` set) only files its own reply's lines: it never
/// blocks, even with uncovered work, and with no reply it does
/// nothing — `session` is the parent's, never this agent's message. The row is
/// news to the parent, so the parent's next read of the file still pushes it.
#[test]
fn a_subagent_stop_files_its_reply_and_never_blocks() {
    let d = adopted(repo());
    std::thread::sleep(std::time::Duration::from_millis(5));
    let edit = format!(
        r#"{{"cwd":{},"session":"{SESSION}","files":["src/b.rs"]}}"#,
        json(&d)
    );
    assert!(fael(&d, &["hook", "edit"], &edit).0);
    let out = stop(&d, r#""agent":"a1""#);
    assert!(out.contains(r#""block":false"#), "no reply: {out}");
    let out = stop(
        &d,
        r#""agent":"a1","reply":"fael issue: Sub-agent saw a race in the writer [files: src/a.rs]""#,
    );
    assert!(out.contains(r#""block":false"#), "{out}");
    assert!(find(&d, "race in the writer").contains("race in the writer"));
    let read = format!(
        r#"{{"cwd":{},"session":"{SESSION}","files":["src/a.rs"]}}"#,
        json(&d)
    );
    let (_, out, _) = fael(&d, &["hook", "read"], &read);
    assert!(out.contains("race in the writer"), "{out}");
}

/// A capture line cannot carry a title: a long untitled line still files
/// (the add gate is off for hook-filed rows) — rejecting it would lose it.
#[test]
fn a_long_untitled_line_still_files() {
    let d = adopted(repo());
    let long = vec!["tenant"; 70].join(" ");
    stop_reply(&d, &format!("fael note: {long} [files: src/a.rs]"));
    let (_, out, _) = fael(&d, &["stats", "--json"], "");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["capture"]["reply_stored"], 1, "{out}");
    assert_eq!(v["capture"]["reply_rejected"], 0, "{out}");
}
