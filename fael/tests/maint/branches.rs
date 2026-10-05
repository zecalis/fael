//! `doctor` for branch notes: orphan rows on dead branches and merged
//! branches left behind.

use super::{fael, repo, state_env};
use std::process::Command;

#[test]
fn doctor_flags_orphan_branches() {
    let d = repo();
    // file the row on a doomed branch: the stamp comes from git, never flags
    assert!(
        Command::new("git")
            .args(["checkout", "-b", "feat/doomed"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    let (ok, _, err) = fael(&d, &["add", "note", "doomed work", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    let (ok, _, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok);
    // canned gh answers through FAEL_GH_JSON: no shell/batch fake survives
    // Windows (CreateProcess resolves .exe only) or runners with a real gh
    let doctor = |json: Option<&str>| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
        c.args(["doctor"]).current_dir(&d);
        state_env(&mut c, &d);
        if let Some(json) = json {
            c.env("FAEL_GH_JSON", json);
        } else {
            c.env_remove("FAEL_GH_JSON");
        }
        let o = c.output().unwrap();
        (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
        )
    };
    // the PR closed unmerged: Orphan names the branch (text and json)
    let (ok, out) = doctor(Some(r#"[{"headRefName":"feat/doomed","mergedAt":null}]"#));
    assert!(ok, "{out}");
    assert!(
        out.contains("note [Orphan]: 1 open row(s)") && out.contains("feat/doomed"),
        "{out}"
    );
    // merged after all: silent
    let (ok, out) = doctor(Some(
        r#"[{"headRefName":"feat/doomed","mergedAt":"2026-09-27T04:50:08Z"}]"#,
    ));
    assert!(ok && !out.contains("[Orphan]"), "{out}");
    // unparseable answer: skipped silently
    let (ok, out) = doctor(Some("not json"));
    assert!(ok && !out.contains("[Orphan]"), "{out}");
    // no seam: the real gh in a remote-less repo fails -> skipped silently
    let (ok, out) = doctor(None);
    assert!(ok && !out.contains("[Orphan]"), "{out}");
}

#[test]
fn doctor_flags_merged_branches() {
    let d = repo();
    let git = |args: &[&str]| {
        let o = Command::new("git")
            .args(args)
            .current_dir(&d)
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    };
    // branches only exist after the first commit (unborn HEAD has none)
    git(&["commit", "-q", "--allow-empty", "-m", "init"]);
    let branch = |args: &[&str]| {
        let o = Command::new("git")
            .args(args)
            .current_dir(&d)
            .output()
            .unwrap();
        assert!(o.status.success(), "git {args:?}");
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    };
    let main = branch(&["symbolic-ref", "--short", "HEAD"]);
    git(&["checkout", "-qb", "feat/landed"]);
    git(&["checkout", "-q", &main]);
    // canned gh answers through FAEL_GH_MERGED_JSON (same reason as orphan's
    // FAEL_GH_JSON: no shell/batch fake survives Windows or real-gh runners)
    let doctor = |json: Option<&str>| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
        c.args(["doctor"]).current_dir(&d);
        state_env(&mut c, &d);
        if let Some(json) = json {
            c.env("FAEL_GH_MERGED_JSON", json);
        } else {
            c.env_remove("FAEL_GH_MERGED_JSON");
        }
        let o = c.output().unwrap();
        (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
        )
    };
    // the PR merged but the branch still exists: Merged names it with the delete
    let (ok, out) = doctor(Some(r#"[{"headRefName":"feat/landed"}]"#));
    assert!(ok, "{out}");
    assert!(
        out.contains("note [Merged]: 1 local branch(es)")
            && out.contains("git branch -D feat/landed"),
        "{out}"
    );
    // a branch with no merged PR is not flagged
    let (ok, out) = doctor(Some(r#"[{"headRefName":"feat/other"}]"#));
    assert!(ok && !out.contains("[Merged]"), "{out}");
    // no merged PR at all: silent
    let (ok, out) = doctor(Some("[]"));
    assert!(ok && !out.contains("[Merged]"), "{out}");
    // the current branch never counts, even when its PR merged
    git(&["checkout", "-q", "feat/landed"]);
    let (ok, out) = doctor(Some(r#"[{"headRefName":"feat/landed"}]"#));
    assert!(ok && !out.contains("[Merged]"), "{out}");
    git(&["checkout", "-q", &main]);
    // `main` never counts either
    let (ok, out) = doctor(Some(format!(r#"[{{"headRefName":{main:?}}}]"#).as_str()));
    assert!(ok && !out.contains("[Merged]"), "{out}");
    // unparseable answer: skipped silently
    let (ok, out) = doctor(Some("not json"));
    assert!(ok && !out.contains("[Merged]"), "{out}");
    // the default branch comes from origin/HEAD, not a hardcoded `main`
    git(&["branch", "develop"]);
    git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/develop",
    ]);
    let (ok, out) = doctor(Some(r#"[{"headRefName":"develop"}]"#));
    assert!(ok && !out.contains("[Merged]"), "{out}");
    // no seam: the real gh in a remote-less repo fails -> skipped silently
    let (ok, out) = doctor(None);
    assert!(ok && !out.contains("[Merged]"), "{out}");
}
