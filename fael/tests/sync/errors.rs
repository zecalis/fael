//! The two fail shapes the contract pins: no remote is a one-line error that
//! pushes nothing, and an empty journal is a no-op that creates no ref.

use super::*;

#[test]
fn no_remote_is_a_one_line_error() {
    let d = repo("errors-none", "Alice", "alice@example.com");
    add(&d, "a row that cannot ship yet");

    let (ok, out, err) = sync(&d);
    assert!(!ok, "sync without a destination must fail");
    assert_eq!(
        err.trim(),
        "fael: no fael.remote — set it with: git config fael.remote <url>"
    );
    assert!(out.is_empty(), "nothing on stdout: {out}");

    // the flag wins over the missing config — same push, no config needed
    let remote = bare("errors-flag");
    let url = remote.to_str().unwrap().to_string();
    let (ok, out, err) = fael(&d, &["sync", "--remote", url.as_str()]);
    assert!(ok, "{err}");
    assert!(out.contains("synced: pushed 1"), "{out}");
    assert_eq!(fael_refs(&remote).len(), 1);
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
