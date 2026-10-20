//! The two fail shapes the contract pins: no remote is a one-line error that
//! pushes nothing, and an empty journal is a no-op that creates no ref.

use super::*;

#[test]
fn no_remote_pushes_nothing_not_even_to_origin() {
    // decision 01M5D4KX: fael.remote is never defaulted — an `origin` that
    // exists is not a destination until fael.remote names it on purpose
    let origin = bare("errors-origin");
    let d = repo("errors-none", "Alice", "alice@example.com");
    git(
        &d,
        &["config", "remote.origin.url", origin.to_str().unwrap()],
    );
    add(&d, "a row that cannot ship yet");

    let (ok, out, err) = sync(&d);
    assert!(!ok, "sync without a destination must fail");
    assert!(
        err.starts_with("fael: no fael.remote") && err.contains("git config fael.remote <url>"),
        "{err}"
    );
    assert_eq!(err.trim().lines().count(), 1, "one line: {err}");
    assert!(out.is_empty(), "nothing on stdout: {out}");
    assert!(fael_refs(&origin).is_empty(), "nothing reached origin");
}

#[test]
fn an_empty_journal_creates_no_ref() {
    let remote = bare("errors-empty");
    let d = repo("errors-empty-a", "Alice", "alice@example.com");
    point(&d, &remote);

    let (ok, out, err) = sync(&d);
    assert!(ok, "{err}");
    assert_eq!(out.trim(), "nothing to sync");
    assert!(fael_refs(&remote).is_empty(), "no ref for an empty journal");

    // and it stays a no-op — never a ref, never a commit
    let (ok, out, err) = sync(&d);
    assert!(ok, "{err}");
    assert_eq!(out.trim(), "nothing to sync");
    assert!(fael_refs(&remote).is_empty());
}
