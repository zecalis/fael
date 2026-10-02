//! The late line (PLAN-fael-local-first chunk 2): a failed sync leaves this
//! writer's rows named in `doctor` until a good sync clears it, and a
//! `--remote` that synced becomes `fael.remote`; one that failed never does.

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
        out.contains("note [Late]: 1 row(s) only in this clone — last sync failed ("),
        "{out}"
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
