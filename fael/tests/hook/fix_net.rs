//! PLAN-fael-experience-loop chunk 5, the fix-time net: the agent says it
//! fixed a bug (5a) or commits a `fix:` naming no row (5b) and filed nothing
//! — the next push offers the add + close once per session. Never a block,
//! never a judgment of which issue a fix is.

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

fn read(d: &Path, session: &str) -> String {
    let input = format!(
        r#"{{"cwd":{},"session":{},"files":["src/nothing.rs"]}}"#,
        json(d),
        serde_json::to_string(session).unwrap()
    );
    let (ok, out, err) = fael(d, &["hook", "read"], &input);
    assert!(ok, "{err}");
    out
}

const SAID: &str = "said it fixed a bug";

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
    let out = read(&d, s);
    assert!(
        out.matches(SAID).count() == 1
            && out.contains("fixed the bug")
            && out.contains("fael add issue")
            && out.contains("fael close --key"),
        "{out}"
    );
    assert!(!read(&d, s).contains(SAID), "shown once");
    // a later turn of the same session says it again: no second line
    stop(&d, s, "The root cause was a stale cache.");
    assert!(!read(&d, s).contains(SAID), "once per session");
    assert!(!read(&d, s).contains("possible problem"));
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
    assert!(read(&d, &s).contains(SAID), "an older issue never clears");
    // a close after the words clears them
    let s = fael_core::rfc3339(fael_core::now_ms());
    std::thread::sleep(std::time::Duration::from_millis(5));
    let (ok, _, err) = fael(&d, &["close", "--key", "a:retry", "cap at 3"], "");
    assert!(ok, "{err}");
    stop(&d, &s, "fixed the bug in the retry loop");
    assert!(!read(&d, &s).contains(SAID));
}

fn commit(d: &Path, msg: &str) -> String {
    let payload = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_name":"Bash","tool_input":{{"command":{}}},"tool_response":{{}}}}"#,
        json(d),
        serde_json::to_string(&format!("git commit -m \"{msg}\"")).unwrap(),
    );
    let (ok, out, err) = fael(d, &["hook", "search", "--client", "claude"], &payload);
    assert!(ok, "{err}");
    out
}

const FIX: &str = "commit names no fael row";

#[test]
fn a_fix_commit_naming_no_row_is_said_once_per_session() {
    let d = super::repo();
    assert!(!commit(&d, "feat: retry cap").contains(FIX), "not a fix");
    assert!(!commit(&d, "prefix: retry cap").contains(FIX));
    let out = commit(&d, "fix(hook): cap retries at 3");
    assert!(
        out.contains(FIX) && out.contains("fael close --key"),
        "{out}"
    );
    assert!(
        !commit(&d, "fix: another").contains(FIX),
        "once per session"
    );
}

#[test]
fn a_fix_commit_naming_a_row_or_after_a_close_is_silent() {
    let d = super::repo();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "retry loops",
            "--files",
            "src/a.rs",
            "--key",
            "a:retry",
            "--json",
        ],
        "",
    );
    assert!(ok, "{err}");
    let v: serde_json::Value = out
        .lines()
        .find_map(|l| serde_json::from_str(l).ok())
        .unwrap();
    let id = v["id"].as_str().unwrap();
    let msg = format!("fix: cap retries\n\n(fael:{})", &id[..8]);
    assert!(!commit(&d, &msg).contains(FIX), "names a row");
    // a session that closed a row already filed what it fixed
    let (ok, _, err) = fael_env(
        &d,
        &["close", "--key", "a:retry", "cap at 3"],
        "",
        &[("CLAUDE_CODE_SESSION_ID", "s1")],
    );
    assert!(ok, "{err}");
    assert!(!commit(&d, "fix: cap retries").contains(FIX));
}

/// How Claude Code commits: a heredoc message, a payload carrying the
/// transcript path (the hook's session) beside the bare UUID (`fael`'s env).
fn claude_commit(d: &Path, subject: &str) -> String {
    let cmd = format!("git commit -m \"$(cat <<'EOF'\n{subject}\n\nbody\nEOF\n)\"");
    let payload = format!(
        r#"{{"cwd":{},"session_id":"u1","transcript_path":"/t/u1.jsonl","tool_name":"Bash","tool_input":{{"command":{}}},"tool_response":{{}}}}"#,
        json(d),
        serde_json::to_string(&cmd).unwrap(),
    );
    let (ok, out, err) = fael(d, &["hook", "search", "--client", "claude"], &payload);
    assert!(ok, "{err}");
    out
}

fn yield_of(d: &Path, kind: &str) -> (u64, u64) {
    let (ok, out, err) = fael(d, &["stats", "--json"], "");
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let n = |k: &str| v["said"][kind][k].as_u64().unwrap();
    (n("said"), n("earned"))
}

#[test]
fn a_heredoc_fix_commit_is_said_and_earned_by_an_issue_after() {
    let d = super::repo();
    assert!(!claude_commit(&d, "feat: cap").contains(FIX));
    assert!(claude_commit(&d, "fix(hook): cap retries").contains(FIX));
    assert_eq!(yield_of(&d, "fixcommit"), (1, 0));
    // a Claude env id stamps a row only once an edit hook recorded it
    // (01M47N67); the per-call id stamps it here
    let (ok, _, err) = fael_env(
        &d,
        &["add", "issue", "retry loops", "--files", "src/a.rs"],
        "",
        &[("FAEL_SESSION", "u1")],
    );
    assert!(ok, "{err}");
    assert_eq!(yield_of(&d, "fixcommit"), (1, 1));
}

/// A close before the session's first edit hook: `fael close` knows only the
/// UUID, the commit hook the transcript path — still one session.
#[test]
fn a_close_under_the_session_uuid_silences_the_commit_line() {
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
    let (ok, _, err) = fael_env(
        &d,
        &["close", "--key", "a:retry", "cap at 3"],
        "",
        &[("CLAUDE_CODE_SESSION_ID", "u1")],
    );
    assert!(ok, "{err}");
    let out = claude_commit(&d, "fix: cap retries");
    assert!(!out.contains(FIX), "{out}");
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
    assert!(read(&d, s).contains(SAID));
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
    let out = read(&d, s);
    assert!(
        out.contains(SAID) && !out.contains("possible problem"),
        "{out}"
    );
    stop(&d, s, "Found a bug in the parser. Fixed the bug too.");
    let out = read(&d, s);
    assert!(
        !out.contains(SAID) && out.contains("possible problem"),
        "{out}"
    );
}
