//! Two clones of one repo converge on one journal: the same rows in both,
//! a flat ref tree, and a working tree (and branch) sync never touches.

use super::*;

#[test]
fn two_clones_converge_on_one_journal_without_duplicate_ids() {
    let remote = bare("share");
    let a = repo("share-a", "Alice", "alice@example.com");
    // same identity, two checkouts: one writer, one ref, two machines
    let b = clone(&a, "share-b", "Alice", "alice@example.com");
    point(&a, &remote);
    point(&b, &remote);

    let first = add(&a, "row filed on the first clone");
    let (ok, out, err) = sync(&a);
    assert!(ok, "{err}");
    assert!(out.contains("synced: pushed 1"), "{out}");

    // the second clone reads the row back: ingest lands in its journal (its
    // first sync may also push — meta.json is re-derived per clone, so the
    // checkout dir and origin label can differ from the first push's)
    let (ok, out, err) = sync(&b);
    assert!(ok, "{err}");
    assert!(out.contains("ingested 1"), "the row arrives: {out}");
    assert!(ids(&b).contains(&first), "ingest must reach the journal");

    let second = add(&b, "row filed on the second clone");
    assert!(sync(&b).0, "the second clone pushes its row");
    let (ok, out, err) = sync(&a);
    assert!(ok, "{err}");
    assert!(out.contains("ingested 1"), "the second row arrives: {out}");

    // both clones list the same rows — and each id exactly once
    for d in [&a, &b] {
        let seen = unique_ids(d);
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert!(seen.contains(&first) && seen.contains(&second), "{seen:?}");
    }
    let mut ra = rows(&a);
    let mut rb = rows(&b);
    ra.sort_by_key(|r| r["id"].as_str().unwrap_or("").to_string());
    rb.sort_by_key(|r| r["id"].as_str().unwrap_or("").to_string());
    assert_eq!(ra, rb, "the row set must be identical in both clones");

    // one repo-id, one writer, one ref — refs/fael/<repo-id>/<writer>
    let id = repoid(&a).unwrap();
    assert_eq!(repoid(&b).unwrap(), id, "both clones derive one repo-id");
    let refs = fael_refs(&remote);
    assert_eq!(
        refs,
        vec![format!(
            "refs/fael/{id}/{}",
            writer("Alice", "alice@example.com")
        )]
    );

    // the pushed tree is flat and carries the contract's meta.json
    for f in ref_files(&remote, &refs[0]) {
        assert!(!f.contains('/'), "the ref tree is flat: {f}");
        assert!(f == "meta.json" || f.ends_with(".jsonl"), "{f}");
    }
    let meta = ref_meta(&remote, &refs[0]);
    assert_eq!(meta["format_version"], 1, "{meta}");
    assert_eq!(meta["repo_id"], id, "{meta}");
    assert!(meta["name"].is_string(), "{meta}");

    // an unchanged journal builds the tip's tree again → no commit, no push
    let (ok, out, err) = sync(&a);
    assert!(ok, "{err}");
    assert!(out.contains("synced: pushed 0, ingested 0"), "{out}");
}

#[test]
fn sync_never_moves_the_branch_or_the_working_tree() {
    let remote = bare("tree");
    let d = repo("tree-a", "Alice", "alice@example.com");
    std::fs::write(d.join("src.rs"), "fn a() {}\n").unwrap();
    std::fs::write(d.join(".gitignore"), ".fael/\n").unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "sources"]);
    point(&d, &remote);
    add(&d, "a row filed while the tree is dirty");
    // uncommitted work in progress — sync must leave it exactly where it is
    std::fs::write(d.join("src.rs"), "fn a() { /* wip */ }\n").unwrap();

    let head = git_out(&d, &["rev-parse", "HEAD"]);
    let status = git_out(&d, &["status", "--porcelain"]);
    let diff = git_out(&d, &["diff"]);
    assert!(
        status.contains("src.rs"),
        "the snapshot must be dirty: {status}"
    );

    let (ok, _, err) = sync(&d);
    assert!(ok, "{err}");

    assert_eq!(
        git_out(&d, &["rev-parse", "HEAD"]),
        head,
        "no commit on the branch"
    );
    assert_eq!(
        git_out(&d, &["status", "--porcelain"]),
        status,
        "status unchanged"
    );
    assert_eq!(git_out(&d, &["diff"]), diff, "the PR diff is unchanged");
    // and the remote gained the fael namespace only — no branch ever moved
    let all = git_out(&remote, &["ls-remote", remote.to_str().unwrap()]);
    assert!(!all.contains("refs/heads/"), "a branch ref appeared: {all}");
}
