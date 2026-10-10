//! Session Focus (PLAN-fael-push-focus chunk 2): session-start writes
//! `focus.json` beside the seen file — the start branch plus the keys of the
//! open rows filed on it — and the push reads it. A row on the session branch
//! or sharing one of those keys lands in Now, ahead of a fresher tier-0
//! decision; no file or an unparsable one is `Focus::default()` (today's
//! order), and the push itself never spawns git.

use super::{fael, fael_at, fael_env, git, json, repo, state};
use std::path::{Path, PathBuf};

/// Two branches of history: a keyed issue filed on `feat/focus` (its key is
/// what the Focus picks up), then an older keyed decision and a fresher plain
/// decision on the start branch, both exact hits on `src/a.rs`.
fn seed(d: &Path, base: &str) {
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::create_dir_all(d.join("lib")).unwrap();
    std::fs::write(d.join("lib/z.rs"), "// z\n").unwrap();
    git(d, &["checkout", "-q", "-b", "feat/focus"]);
    let (ok, _, err) = fael(
        d,
        &[
            "add",
            "issue",
            "keyed on the focus branch",
            "--files",
            "lib/z.rs",
            "--key",
            "auth:session",
        ],
        "",
    );
    assert!(ok, "{err}");
    git(d, &["checkout", "-q", base]);
    // the fresher row carries its own key: without one auto-key adopts the
    // file's key, and it would be Now whatever the Focus says
    for (text, key) in [
        ("older keyed decision", "auth:session"),
        ("fresher tier-0 decision", "db:migrate"),
    ] {
        let args = ["add", "decision", text, "--files", "src/a.rs", "--key", key];
        let (ok, _, err) = fael(d, &args, "");
        assert!(ok, "{err}");
    }
}

fn session_start(d: &Path, state: Option<&Path>, session: &str) {
    let input = format!(r#"{{"cwd":{},"session":"{session}"}}"#, json(d));
    let (ok, _, err) = match state {
        Some(s) => fael_at(s, d, &["hook", "session-start"], &input),
        None => fael(d, &["hook", "session-start"], &input),
    };
    assert!(ok, "{err}");
}

fn edit(d: &Path, state: Option<&Path>, session: &str) -> String {
    let input = format!(
        r#"{{"cwd":{},"session":"{session}","files":["src/a.rs"]}}"#,
        json(d)
    );
    let (ok, out, err) = match state {
        Some(s) => fael_at(s, d, &["hook", "edit"], &input),
        None => fael(d, &["hook", "edit"], &input),
    };
    assert!(ok, "{err}");
    out
}

/// Which of the two decisions the push said first — the whole point of L3.
fn order(out: &str) -> (usize, usize) {
    let keyed = out
        .find("older keyed decision")
        .unwrap_or_else(|| panic!("keyed row missing: {out}"));
    let plain = out
        .find("fresher tier-0 decision")
        .unwrap_or_else(|| panic!("tier-0 row missing: {out}"));
    (keyed, plain)
}

fn focus_files(state: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(state.join("sessions"))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "json"))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

#[test]
fn focus_rows_lead_a_fresher_tier0_decision() {
    let d = repo();
    let base = git(&d, &["rev-parse", "--abbrev-ref", "HEAD"]);
    seed(&d, &base);
    git(&d, &["checkout", "-q", "feat/focus"]);
    // session-start writes the Focus: start branch + the keys filed on it
    session_start(&d, None, "focus-1");
    let files = focus_files(&state(&d));
    assert_eq!(files.len(), 1, "{files:?}");
    let body = std::fs::read_to_string(&files[0]).unwrap();
    assert!(body.contains("feat/focus"), "{body}");
    assert!(body.contains("auth:session"), "{body}");

    // with the Focus: the older keyed row is Now, the fresher tier-0 row is not
    let out = edit(&d, None, "focus-1");
    let (keyed, plain) = order(&out);
    assert!(keyed < plain, "keyed row must lead: {keyed} {plain}\n{out}");
    // the lib/z.rs issue came by its key, not the file: its line says so,
    // and the rows about src/a.rs itself carry no label
    let sibling = out
        .split("\\n")
        .find(|l| l.contains("keyed on the focus branch"))
        .unwrap_or_else(|| panic!("sibling missing: {out}"));
    assert!(sibling.contains("] (same key) issue"), "{sibling}");
    assert_eq!(out.matches("(same ").count(), 1, "{out}");

    // no session-start for this session = no Focus = today's order, freshest first
    let (keyed, plain) = order(&edit(&d, None, "no-focus"));
    assert!(
        plain < keyed,
        "freshness must decide with no Focus: {keyed} {plain}"
    );
}

#[test]
fn bad_focus_file_falls_back_to_default_order() {
    let d = repo();
    let base = git(&d, &["rev-parse", "--abbrev-ref", "HEAD"]);
    seed(&d, &base);
    git(&d, &["checkout", "-q", "feat/focus"]);
    let state = d.join("state-bad");
    session_start(&d, Some(&state), "bad-1");
    let files = focus_files(&state);
    assert_eq!(files.len(), 1, "{files:?}");
    std::fs::write(&files[0], "{not json").unwrap();
    // unreadable Focus = `Focus::default()` — freshness decides, no error
    let (keyed, plain) = order(&edit(&d, Some(&state), "bad-1"));
    assert!(
        plain < keyed,
        "a bad Focus must not reorder: {keyed} {plain}"
    );
}

#[test]
fn edit_push_spawns_no_git() {
    let d = repo();
    let base = git(&d, &["rev-parse", "--abbrev-ref", "HEAD"]);
    seed(&d, &base);
    git(&d, &["checkout", "-q", "feat/focus"]);
    let trace = d.join("git-trace.log");
    let tripwire = [("GIT_TRACE", trace.to_str().unwrap())];
    // control: session-start does spawn git, so the tripwire really fires
    let input = format!(r#"{{"cwd":{},"session":"nogit"}}"#, json(&d));
    let (ok, _, err) = fael_env(&d, &["hook", "session-start"], &input, &tripwire);
    assert!(ok, "{err}");
    assert!(
        trace.exists(),
        "GIT_TRACE never fired — the tripwire is dead"
    );
    std::fs::remove_file(&trace).unwrap();
    // same session, same file: the push only reads focus.json
    let input = format!(
        r#"{{"cwd":{},"session":"nogit","files":["src/a.rs"]}}"#,
        json(&d)
    );
    let (ok, out, _) = fael_env(&d, &["hook", "edit"], &input, &tripwire);
    assert!(ok, "{out}");
    assert!(
        !trace.exists(),
        "the edit push spawned git:\n{}",
        std::fs::read_to_string(&trace).unwrap_or_default()
    );
}

#[test]
fn head_switch_mid_session_rebuilds_focus() {
    let d = repo();
    let base = git(&d, &["rev-parse", "--abbrev-ref", "HEAD"]);
    seed(&d, &base);
    // the session starts on base: both decisions are filed there, so both
    // are Now and freshness decides
    let state = d.join("state-head");
    session_start(&d, Some(&state), "head-1");
    // another session switches the worktree; the push follows HEAD
    git(&d, &["checkout", "-q", "feat/focus"]);
    let (keyed, plain) = order(&edit(&d, Some(&state), "head-1"));
    assert!(keyed < plain, "Focus stuck on {base}: {keyed} {plain}");
    let files = focus_files(&state);
    let body = std::fs::read_to_string(&files[0]).unwrap();
    assert!(body.contains("feat/focus"), "{body}");
}
