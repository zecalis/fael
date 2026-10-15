//! Stop-hook adoption: a log in the clone's journal counts, not only one in the
//! worktree's own `.fael/log` — a `store = "local"` repo keeps no rows in the tree.
//! Stop never blocks; a bug announcement is a line on the next push.

use super::{fael, json, repo, transcript};
use std::path::Path;

/// An RFC 3339 session: stop reads the turn from it, no transcript needed.
const SESSION: &str = "2020-01-01T00:00:00Z";

fn stop(d: &Path, extra: &str) -> String {
    let input = format!(r#"{{"cwd":{},"session":"{SESSION}",{extra}}}"#, json(d));
    let (ok, out, err) = fael(d, &["hook", "stop"], &input);
    assert!(ok, "{err}");
    out
}

fn next_push(d: &Path) -> String {
    let input = format!(
        r#"{{"cwd":{},"session":"{SESSION}","files":["src/nothing.rs"]}}"#,
        json(d)
    );
    fael(d, &["hook", "edit"], &input).1
}

const BUG: &str = r#""text":"bug confirmed in logout""#;

fn seed(d: &Path) {
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let (ok, _, err) = fael(d, &["add", "note", "seed row", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
}

/// `store = "local"`: rows live in the journal only, yet stop still runs the bug rule.
#[test]
fn a_journal_only_repo_runs_the_bug_rule() {
    let d = repo();
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "store = \"local\"\n").unwrap();
    seed(&d);
    assert!(!d.join(".fael/log").exists(), "the tree holds no log");
    stop(&d, BUG);
    assert!(next_push(&d).contains("possible problem"));
}

/// A repo fael never touched stays untouched: no log anywhere, nothing written.
#[test]
fn a_repo_without_any_log_is_left_alone() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    stop(&d, BUG);
    assert!(!d.join(".fael/log").exists() && !d.join(".git/fael").exists());
}

/// A `fael <kind>:` line in the reply is plain text now: nothing is filed.
#[test]
fn a_reply_line_files_nothing() {
    let d = repo();
    seed(&d);
    stop(
        &d,
        r#""reply":"done\n\nfael issue: Retry loop has no backoff [files: src/a.rs]""#,
    );
    assert!(fael(&d, &["find", "backoff"], "").1.is_empty());
}

#[test]
fn stop_never_returns_a_block() {
    let d = repo();
    seed(&d);
    // work: an edit after the last row, no row for it
    std::fs::write(d.join("src/b.rs"), "//\n").unwrap();
    let edit = format!(
        r#"{{"cwd":{},"session":"{SESSION}","files":["src/b.rs"]}}"#,
        json(&d)
    );
    assert!(fael(&d, &["hook", "edit"], &edit).0);
    let out = stop(&d, r#""text":"finished""#);
    assert!(out.contains(r#""block":false"#), "work: {out}");

    // bug: a strong announcement with no issue row is a hint, not a block
    let out = stop(&d, BUG);
    assert!(out.contains(r#""block":false"#), "bug: {out}");
    assert!(next_push(&d).contains("possible problem"), "bug hint");

    // claude adapter: no `decision: block` line on stdout
    let t = transcript(&d, "t.jsonl");
    let input = format!(r#"{{"cwd":{},"transcript_path":{}}}"#, json(&d), json(&t));
    super::commit(&d, "work with no row");
    let (ok, out, _) = fael(&d, &["hook", "stop", "--client", "claude"], &input);
    assert!(ok && !out.contains("block"), "claude: {out}");
}
