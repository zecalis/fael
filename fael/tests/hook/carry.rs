//! PLAN-fael-experience-loop chunk 6, the carry-back: an edit of a file whose
//! closed issue was closed naming its fix (a sha or `(#N)`) puts "what broke
//! → how it was fixed" in front of the agent once per session; a close with
//! no fix named, or a read, is silent.

use super::{fael, fael_env, git, json, repo};
use std::path::Path;

const SAID: &str = "broke before";

fn hook(d: &Path, event: &str, session: &str) -> String {
    let input = format!(
        r#"{{"cwd":{},"session_id":"{session}","tool_input":{{"file_path":{}}}}}"#,
        json(d),
        json(&d.join("src/a.rs"))
    );
    let (ok, out, err) = fael(d, &["hook", event, "--client", "claude"], &input);
    assert!(ok, "{err}");
    out
}

/// An issue on `src/a.rs`, closed with `why`.
fn closed_with(d: &Path, why: &str) {
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let add = [
        "add",
        "issue",
        "retry loops",
        "--files",
        "src/a.rs",
        "--key",
        "a:retry",
    ];
    let (ok, _, err) = fael(d, &add, "");
    assert!(ok, "{err}");
    let (ok, _, err) = fael(d, &["close", "--key", "a:retry", why], "");
    assert!(ok, "{err}");
}

fn carry(d: &Path) -> (u64, u64) {
    let (_, out, _) = fael(d, &["stats", "--json"], "");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let n = |k: &str| v["said"]["carry"][k].as_u64().unwrap();
    (n("said"), n("earned"))
}

#[test]
fn a_fixed_bug_is_carried_back_once_per_session_and_earns_on_a_find() {
    let d = repo();
    std::fs::create_dir_all(d.join("tests")).unwrap();
    std::fs::write(d.join("tests/retry.rs"), "\n").unwrap();
    closed_with(
        &d,
        "no cap → cap at 3; guard `tests/retry.rs`; fixed in abc1234",
    );
    let first = hook(&d, "edit", "s1");
    assert!(
        first.contains("src/a.rs broke before")
            && first.contains("\\\"retry loops\\\" was fixed: no cap → cap at 3")
            && first.contains("fael find "),
        "{first}"
    );
    assert!(!hook(&d, "edit", "s1").contains(SAID));
    assert_eq!(carry(&d), (1, 0));
    // the agent pulls the whole story by the short id the line named
    let id = first.split("`fael find ").nth(1).expect(&first);
    let id = id.split('`').next().unwrap();
    let env = [("CLAUDE_CODE_SESSION_ID", "s1")];
    let (ok, _, err) = fael_env(&d, &["find", id], "", &env);
    assert!(ok, "{err}");
    assert_eq!(carry(&d), (1, 1));
    // another session is told again
    assert!(hook(&d, "edit", "s2").contains(SAID));
}

#[test]
fn no_fix_named_or_a_read_is_silent() {
    let d = repo();
    closed_with(&d, "not a bug, works as meant");
    assert!(!hook(&d, "edit", "s1").contains(SAID));
    let d = repo();
    closed_with(&d, "fixed in (#12)");
    assert!(!hook(&d, "read", "s1").contains(SAID));
    // the read spent nothing: the edit that follows is told
    assert!(hook(&d, "edit", "s1").contains(SAID));
}

/// fael:01M4GQVP: a close whose only evidence is a branch sha that never
/// reached HEAD or the default branch is not carried; once a commit there
/// keeps that sha's subject (a squash does), it is.
#[test]
fn a_fix_on_an_unmerged_branch_is_not_carried_until_its_subject_lands() {
    let d = repo();
    let home = git(&d, &["rev-parse", "--abbrev-ref", "HEAD"]);
    git(&d, &["switch", "-q", "-c", "side"]);
    git(
        &d,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "fix: cap retries at 3",
        ],
    );
    // the full sha: a short one is all digits ~4% of the time, and a close
    // naming only digits names no fix (`sha_like` needs a letter too)
    let sha = git(&d, &["rev-parse", "HEAD"]);
    git(&d, &["switch", "-q", home.trim()]);
    closed_with(&d, &format!("no cap → cap at 3; {}", sha.trim()));
    assert!(
        !hook(&d, "edit", "s1").contains(SAID),
        "unmerged fix carried"
    );
    git(
        &d,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "squash (#9)\n\n* fix: cap retries at 3",
        ],
    );
    assert!(
        hook(&d, "edit", "s2").contains(SAID),
        "merged fix not carried"
    );
}

/// The other evidence a squash keeps: a commit naming the issue id.
#[test]
fn an_unmerged_sha_is_carried_once_a_commit_cites_the_issue() {
    let d = repo();
    let home = git(&d, &["rev-parse", "--abbrev-ref", "HEAD"]);
    git(&d, &["switch", "-q", "-c", "side"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "wip"]);
    // the full sha: a short one is all digits ~4% of the time, and a close
    // naming only digits names no fix (`sha_like` needs a letter too)
    let sha = git(&d, &["rev-parse", "HEAD"]);
    git(&d, &["switch", "-q", home.trim()]);
    closed_with(&d, &format!("no cap → cap at 3; {}", sha.trim()));
    assert!(!hook(&d, "edit", "s1").contains(SAID));
    let (_, out, _) = fael(&d, &["find", "--key", "a:retry", "--all"], "");
    let id = out.split(['[', ']']).nth(1).unwrap().to_string();
    git(
        &d,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            &format!("cap retries (fael:{id})"),
        ],
    );
    assert!(hook(&d, "edit", "s2").contains(SAID));
}
