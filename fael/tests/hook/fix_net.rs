//! PLAN-fael-experience-loop chunk 5a, the fix-time net: the agent says it
//! fixed a bug and filed nothing — the next push offers the add + close once
//! per session. Never a block, never a judgment of which issue a fix is. (The
//! `fix:` commit line, 5b, was cut at 2 of 65 earned — decision
//! `product:push-noise-bar`.)

use super::{fael, fael_env, json};
use std::path::Path;

fn stop(d: &Path, session: &str, text: &str) {
    let input = format!(
        r#"{{"cwd":{},"session":{},"text":{}}}"#,
        json(d),
        serde_json::to_string(session).unwrap(),
        serde_json::to_string(text).unwrap()
    );
    let (ok, out, err) = fael(d, &["hook", "stop"], &input);
    assert!(ok && out.contains(r#""block":false"#), "{out}{err}");
}

/// The next push of `session`: an edit of a file no row is on.
fn push(d: &Path, session: &str) -> String {
    let input = format!(
        r#"{{"cwd":{},"session":{},"files":["src/nothing.rs"]}}"#,
        json(d),
        serde_json::to_string(session).unwrap()
    );
    let (ok, out, err) = fael(d, &["hook", "edit"], &input);
    assert!(ok, "{err}");
    out
}

const SAID: &str = "said it fixed a bug";

/// The skill-taught close for this file's retry issue (an issue owes the
/// template's arrow and guard).
fn close_retry(d: &Path, key: &str, env: &[(&str, &str)]) {
    let (ok, _, err) = fael_env(
        d,
        &[
            "close",
            "--key",
            key,
            "retry loops → capped at 3; guard `src/a_test.rs`",
        ],
        "",
        env,
    );
    assert!(ok, "{err}");
}

#[test]
fn a_fix_phrase_with_nothing_filed_is_said_once_per_session() {
    let d = super::repo();
    // a log to read: without one fael was never adopted here
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let s = "2020-01-01T00:00:00Z";
    stop(&d, s, "Fixed the bug: the retry cap was off by one.");
    let out = push(&d, s);
    assert!(
        out.matches(SAID).count() == 1
            && out.contains("fixed the bug")
            && out.contains("fael add issue")
            && out.contains("cite `(fael:<id>)`")
            && out.contains("fael close --key"),
        "{out}"
    );
    assert!(!push(&d, s).contains(SAID), "shown once");
    // a later turn of the same session says it again: no second line
    stop(&d, s, "The root cause was a stale cache.");
    assert!(!push(&d, s).contains(SAID), "once per session");
    assert!(!push(&d, s).contains("possible problem"));
}

#[test]
fn an_issue_or_a_close_after_the_words_clears_them_one_before_does_not() {
    let d = super::repo();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "retry loops",
            "--files",
            "src/a.rs",
            "--key",
            "a:retry",
        ],
        "",
    );
    assert!(ok, "{err}");
    // the session starts after the issue: the issue is older than the words
    std::thread::sleep(std::time::Duration::from_millis(5));
    let s = fael_core::rfc3339(fael_core::now_ms());
    stop(&d, &s, "fixed the bug in the retry loop");
    assert!(push(&d, &s).contains(SAID), "an older issue never clears");
    // a close after the words clears them
    let s = fael_core::rfc3339(fael_core::now_ms());
    std::thread::sleep(std::time::Duration::from_millis(5));
    close_retry(&d, "a:retry", &[]);
    stop(&d, &s, "fixed the bug in the retry loop");
    assert!(!push(&d, &s).contains(SAID));
}

fn yield_of(d: &Path, kind: &str) -> (u64, u64) {
    let (ok, out, err) = fael(d, &["stats", "--json"], "");
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let n = |k: &str| v["said"][kind][k].as_u64().unwrap();
    (n("said"), n("earned"))
}

#[test]
fn a_fix_line_is_counted_and_earned_by_an_issue_after() {
    let d = super::repo();
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let s = "2020-01-01T00:00:00Z";
    stop(&d, s, "fixed the bug in the retry loop");
    assert!(push(&d, s).contains(SAID));
    assert_eq!(yield_of(&d, "fixed"), (1, 0));
    let (ok, _, err) = fael_env(
        &d,
        &["add", "issue", "retry loops", "--files", "src/a.rs"],
        "",
        &[("FAEL_SESSION", s)],
    );
    assert!(ok, "{err}");
    assert_eq!(yield_of(&d, "fixed"), (1, 1));
}

/// Both in one turn: the fix line takes the bug line's place. Once the fix
/// line is spent, a later bug announcement gets the bug line back.
#[test]
fn the_fix_line_wins_a_turn_and_the_bug_line_comes_back_after() {
    let d = super::repo();
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let s = "2020-01-01T00:00:00Z";
    stop(
        &d,
        s,
        "Found a bug in the retry loop. Fixed the bug by capping at 3.",
    );
    let out = push(&d, s);
    assert!(
        out.contains(SAID) && !out.contains("possible problem"),
        "{out}"
    );
    stop(&d, s, "Found a bug in the parser. Fixed the bug too.");
    let out = push(&d, s);
    assert!(
        !out.contains(SAID) && out.contains("possible problem"),
        "{out}"
    );
}

/// The fix line names what the session edited for `--files`, not only the
/// file the next push happens to be on.
#[test]
fn the_fix_line_names_the_files_the_session_edited() {
    let d = super::repo();
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let edit = |session: &str| {
        let input = format!(
            r#"{{"cwd":{},"session":{},"files":["src/fixed.rs"]}}"#,
            json(&d),
            serde_json::to_string(session).unwrap()
        );
        let (ok, _, err) = fael(&d, &["hook", "edit"], &input);
        assert!(ok, "{err}");
    };
    let s = "2020-01-01T00:00:00Z";
    edit(s);
    stop(&d, s, "Fixed the bug: the retry cap was off by one.");
    // the push is an edit too: its file joins, newest first
    let out = push(&d, s);
    assert!(
        out.contains(SAID) && out.contains("--files src/nothing.rs,src/fixed.rs --key"),
        "{out}"
    );
}

/// One Claude transcript line stamped now.
fn line_now(role: &str, text: &str) -> String {
    let ts = fael_core::rfc3339(fael_core::now_ms());
    let v = serde_json::json!({
        "message": {"role": role, "content": [{"type": "text", "text": text}]},
        "timestamp": ts,
    });
    format!("{v}\n")
}

/// The honest order is fix → close → summary: a close made in this turn
/// clears the summary's fix phrase, from the transcript or the client's
/// reply alike. A close from an earlier turn does not (fael:01M4F4CD).
#[test]
fn a_close_earlier_in_the_turn_clears_the_summarys_fix_phrase() {
    let d = super::repo();
    let pause = || std::thread::sleep(std::time::Duration::from_millis(5));
    // one session (transcript) per case; `in_turn`: the close comes after
    // the prompt, else before it; `reply`: the summary only in the client's
    // reply, the transcript lagging
    let said = |key: &str, in_turn: bool, reply: bool| {
        let args = ["add", "issue", key, "--files", "src/a.rs", "--key", key];
        assert!(fael(&d, &args, "").0);
        let close = || close_retry(&d, key, &[]);
        let t = d.join(format!("{}.jsonl", key.replace(':', "-")));
        std::fs::write(&t, "").unwrap();
        pause();
        if !in_turn {
            close();
            pause();
        }
        let mut body = line_now("user", "fix the retry loop");
        pause();
        if in_turn {
            close();
            pause();
        }
        let summary = "สาเหตุคือ retry ไม่มี cap — Fixed the bug by capping at 3.";
        if !reply {
            body.push_str(&line_now("assistant", summary));
        }
        std::fs::write(&t, body).unwrap();
        let input = format!(
            r#"{{"cwd":{},"session":{},"reply":{}}}"#,
            json(&d),
            json(&t),
            serde_json::to_string(summary).unwrap()
        );
        let (ok, out, err) = fael(&d, &["hook", "stop"], &input);
        assert!(ok && out.contains(r#""block":false"#), "{out}{err}");
        push(&d, &t.to_string_lossy()).contains(SAID)
    };
    assert!(!said("a:one", true, false), "close in turn, transcript");
    assert!(!said("a:two", true, true), "close in turn, reply");
    assert!(said("a:three", false, false), "close before the turn");
}
