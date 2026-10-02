//! The late line (PLAN-fael-local-first chunk 2): a failed sync leaves this
//! writer's rows named in `doctor` until a good sync clears it, and a
//! `--remote` that synced becomes `fael.remote`; one that failed never does.
//! The line follows `fael.remote` only, never counts without a watermark, and
//! names a repo that used to share by commit and now cannot.

use super::*;

#[test]
fn a_failed_sync_is_named_until_a_good_one() {
    let d = repo("late-fail", "Alice", "alice@example.com");
    add(&d, "filed before any remote works");
    let (ok, _, _) = fael(&d, &["sync", "--remote", "/nowhere/fael.git"]);
    assert!(!ok, "a missing remote fails");
    let kept = Command::new("git")
        .args(["config", "fael.remote"])
        .current_dir(&d)
        .output()
        .unwrap();
    assert!(!kept.status.success(), "a failed --remote is never kept");
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Late]: last sync failed (")
            && out.contains("may be only in this clone"),
        "no watermark yet — no count: {out}"
    );

    let remote = bare("late-ok");
    let url = remote.to_str().unwrap().to_string();
    let (ok, out, err) = fael(&d, &["sync", "--remote", url.as_str()]);
    assert!(ok, "{err}");
    assert!(out.contains("fael.remote = "), "{out}");
    assert_eq!(git_out(&d, &["config", "fael.remote"]), url);
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[Late]"), "a good sync clears it: {out}");
}

/// Doctor's `[Late]` line, if any.
fn late(d: &Path) -> Option<String> {
    let (_, out, _) = fael(d, &["doctor"]);
    out.lines().find(|l| l.contains("[Late]")).map(String::from)
}

#[test]
fn after_a_good_sync_a_failure_counts_only_newer_rows() {
    let d = repo("late-count", "Alice", "alice@example.com");
    point(&d, &bare("late-count"));
    add(&d, "synced already");
    assert!(sync(&d).0);
    add(&d, "filed while the remote is down");
    point(&d, Path::new("/nowhere/fael.git"));
    assert!(!sync(&d).0);
    let l = late(&d).unwrap_or_default();
    assert!(l.contains("1 row(s) only in this clone"), "{l}");
}

#[test]
fn sync_with_no_remote_leaves_no_late_line() {
    let d = repo("late-none", "Alice", "alice@example.com");
    add(&d, "a single clone is complete");
    let (ok, _, err) = sync(&d);
    assert!(!ok && err.contains("no fael.remote"), "{err}");
    assert_eq!(late(&d), None);
}

#[test]
fn a_one_off_remote_never_clears_fael_remote_failure() {
    let d = repo("late-oneoff", "Alice", "alice@example.com");
    add(&d, "never reached the team remote");
    point(&d, Path::new("/nowhere/team.git"));
    assert!(!sync(&d).0);
    let before = late(&d);
    assert!(before.is_some());
    let backup = bare("late-backup");
    assert!(fael(&d, &["sync", "--remote", backup.to_str().unwrap()]).0);
    assert_eq!(late(&d), before, "the team remote still lacks the row");
}

#[test]
fn a_repo_that_shared_by_commit_says_new_rows_stay_here() {
    let d = repo("late-legacy", "Alice", "alice@example.com");
    std::fs::create_dir_all(d.join(".fael/log")).unwrap();
    std::fs::write(d.join(".fael/log/2026-09.jsonl"), "").unwrap();
    git(&d, &["add", ".fael"]);
    git(&d, &["commit", "-q", "-m", "memory"]);
    add(&d, "lands in the journal only");
    let l = late(&d).unwrap_or_default();
    assert!(l.contains("new rows stay in this clone"), "{l}");
    point(&d, &bare("late-legacy"));
    assert_eq!(late(&d), None, "a remote shares them");
}
