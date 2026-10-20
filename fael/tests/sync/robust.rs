//! Ingest never stops the team's sync and never touches the working tree, and
//! two syncs of one clone cannot append the same rows twice.

use super::secret::{git_in, plant};
use super::*;

/// A commit holding just `meta.json` = `meta`, written straight to `rname` on
/// the bare remote — what a newer fael's `format_version` bump looks like.
fn plant_ref(remote: &Path, rname: &str, meta: &str) {
    let blob = git_in(remote, &["hash-object", "-w", "--stdin"], meta);
    let tree = git_in(
        remote,
        &["mktree"],
        &format!("100644 blob {blob}\tmeta.json\n"),
    );
    let id = ["-c", "user.name=new", "-c", "user.email=new@example.com"];
    let commit = git_out(
        remote,
        &[&id[..], &["commit-tree", &tree, "-m", "newer fael"]].concat(),
    );
    git(remote, &["update-ref", rname, &commit]);
}

fn journal(d: &Path) -> PathBuf {
    let common = git_out(
        d,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    );
    Path::new(&common).join("fael")
}

#[test]
fn a_bad_ref_or_row_is_skipped_and_everything_else_still_lands() {
    let remote = bare("robust");
    let src = repo("robust-src", "Seed", "seed@example.com");
    let a = clone(&src, "robust-a", "Alice", "alice@example.com");
    let b = clone(&src, "robust-b", "Bob", "bob@example.com");
    point(&a, &remote);
    point(&b, &remote);
    let first = add(&a, "row from a healthy writer");
    assert!(sync(&a).0);

    // sorts before alice's ref, so a hard error would stop her row arriving
    let id = repoid(&a).unwrap();
    let bad = format!("refs/fael/{id}/aaa-newer");
    plant_ref(&remote, &bad, r#"{"format_version":99,"future":true}"#);
    // and one row on her ref that `append` refuses (writer id is not a folder name)
    let own = format!("refs/fael/{id}/{}", writer("Alice", "alice@example.com"));
    let line = r#"{"v":1,"id":"01M3ZZZZZZZZZZZZZZZZZZZZZY","ts":"2026-09-30T06:00:00.000Z","by":"_evil","kind":"note","text":"bad writer","files":["doc:sync"]}"#;
    plant(&remote, &own, line);

    let (ok, out, err) = sync(&b);
    assert!(ok, "one bad ref must not fail the sync: {err}");
    assert!(err.contains(&format!("skipped {bad}")), "{err}");
    assert!(
        err.contains("skipped row 01M3ZZZZZZZZZZZZZZZZZZZZZY"),
        "{err}"
    );
    assert!(out.contains("ingested 1"), "the healthy row lands: {out}");
    assert!(ids(&b).contains(&first));
}

#[test]
fn ingest_never_writes_the_working_tree() {
    let remote = bare("tree");
    let src = repo("tree-src", "Seed", "seed@example.com");
    let a = clone(&src, "tree-a", "Alice", "alice@example.com");
    let b = clone(&src, "tree-b", "Bob", "bob@example.com");
    point(&a, &remote);
    point(&b, &remote);
    let theirs = add(&a, "row filed by another writer");
    assert!(sync(&a).0);
    let before = git_out(&b, &["status", "--porcelain", "-uall"]);

    let (ok, out, err) = sync(&b);
    assert!(ok, "{err}");
    assert!(out.contains("ingested 1"), "{out}");
    assert!(ids(&b).contains(&theirs), "the journal has it");
    let who = writer("Alice", "alice@example.com");
    assert!(
        !b.join(".fael/log").join(&who).exists(),
        "a teammate's rows must not land in .fael/log"
    );
    assert_eq!(git_out(&b, &["status", "--porcelain", "-uall"]), before);
}

#[test]
fn a_second_sync_stops_instead_of_appending_twice() {
    let remote = bare("lock");
    let a = repo("lock-a", "Alice", "alice@example.com");
    point(&a, &remote);
    add(&a, "a row to sync");

    std::fs::create_dir_all(journal(&a)).unwrap();
    let held = std::fs::File::create(journal(&a).join("sync.lock")).unwrap();
    held.lock().unwrap(); // a sync in progress, from another worktree or process
    let (ok, _, err) = sync(&a);
    assert!(!ok && err.contains("another sync is running"), "{err}");
    assert!(
        fael_refs(&remote).is_empty(),
        "the blocked sync pushed nothing"
    );

    drop(held);
    let (ok, out, err) = sync(&a);
    assert!(ok, "{err}");
    assert!(out.contains("pushed 1"), "{out}");
}

#[test]
fn many_writers_ingest_in_one_pass() {
    let remote = bare("many");
    let src = repo("many-src", "Seed", "seed@example.com");
    let who = [("Alice", "alice@"), ("Bob", "bob@"), ("Carol", "carol@")];
    let mut want = vec![];
    for (name, mail) in who {
        let d = clone(
            &src,
            &format!("many-{name}"),
            name,
            &format!("{mail}example.com"),
        );
        point(&d, &remote);
        want.push(add(&d, &format!("row from {name}")));
        assert!(sync(&d).0);
    }
    let reader = clone(&src, "many-reader", "Dave", "dave@example.com");
    point(&reader, &remote);
    let (ok, out, err) = sync(&reader);
    assert!(ok, "{err}");
    assert!(out.contains("ingested 3"), "{out}");
    let seen = unique_ids(&reader);
    assert!(want.iter().all(|id| seen.contains(id)), "{seen:?}");
    // a second pass has nothing left to ingest
    let again = sync(&reader).1;
    assert!(again.contains("nothing to sync"), "{again}");
}
