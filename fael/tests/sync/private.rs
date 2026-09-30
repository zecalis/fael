//! The private-memory-repo workflow (PLAN-fael-journal-transport chunk 5): the
//! source repo is public and carries no `.fael/log`, its memory lives in a
//! separate private remote, and a fresh clone of the source gets every row
//! back by one `git config fael.remote` + `fael sync`. These are the commands
//! `docs/integrate.md` §Private memory repo tells a team to run.

use super::*;

#[test]
fn fresh_clone_of_a_public_source_gets_every_row_from_the_private_remote() {
    let public = bare("private-public");
    let memory = bare("private-memory");

    // 1. the source repo: `store = local` is committed, `.fael/log` never is
    let a = repo("private-a", "Alice", "alice@example.com");
    std::fs::create_dir_all(a.join(".fael")).unwrap();
    std::fs::write(a.join(".fael/config.toml"), "store = \"local\"\n").unwrap();
    git(&a, &["add", ".fael/config.toml"]);
    git(&a, &["commit", "-qm", "fael: keep rows out of the tree"]);
    git(&a, &["remote", "add", "origin", public.to_str().unwrap()]);
    git(&a, &["push", "-q", "origin", "HEAD:refs/heads/main"]);

    // 2. memory goes to the private remote, never to origin
    point(&a, &memory);
    let first = add(&a, "decision filed on the first machine");
    let second = add(&a, "note filed on the first machine");
    let (ok, out, err) = sync(&a);
    assert!(ok, "{err}");
    assert!(out.contains("synced: pushed 2"), "{out}");
    assert!(!err.contains("publicly fetchable"), "private remote: {err}");
    assert!(
        fael_refs(&public).is_empty(),
        "origin never sees a fael ref"
    );
    assert_eq!(
        fael_refs(&memory).len(),
        1,
        "the memory remote holds the ref"
    );
    assert!(!a.join(".fael/log").exists(), "no log in the source tree");
    assert!(
        !git_out(&a, &["ls-files"]).contains(".fael/log"),
        "no log tracked"
    );

    // 3. a fresh clone of the source — the source has no rows to give it
    let b = clone(&public, "private-b", "Bob", "bob@example.com");
    assert!(ids(&b).is_empty(), "the source carries no memory");
    point(&b, &memory);
    let (ok, out, err) = sync(&b);
    assert!(ok, "{err}");
    assert!(out.contains("ingested 2"), "every row arrives: {out}");
    assert!(!err.contains("publicly fetchable"), "{err}");
    let seen = unique_ids(&b);
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert!(seen.contains(&first) && seen.contains(&second), "{seen:?}");
    assert!(
        !b.join(".fael/log").exists(),
        "ingest fills the journal only"
    );

    // 4. the second machine's own rows flow back the same way
    let third = add(&b, "note filed on the second machine");
    assert!(sync(&b).0);
    assert!(sync(&a).0);
    assert!(ids(&a).contains(&third), "the first machine reads it back");
    assert!(
        fael_refs(&public).is_empty(),
        "origin still holds no fael ref"
    );
    assert_eq!(fael_refs(&memory).len(), 2, "one ref per writer");
}
