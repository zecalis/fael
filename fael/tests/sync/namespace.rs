//! Two repos on one remote keep their own namespaces, and one repo keeps one
//! repo-id across every clone and branch — two root commits included.

use super::*;

#[test]
fn two_repos_on_one_remote_never_share_a_ref() {
    let remote = bare("ns");
    let one = repo("ns-one", "Alice", "alice@example.com");
    let two = repo("ns-two", "Carol", "carol@example.com");
    point(&one, &remote);
    point(&two, &remote);
    let mine = add(&one, "row of repo one");
    let theirs = add(&two, "row of repo two");
    assert!(sync(&one).0, "repo one pushes");
    assert!(sync(&two).0, "repo two pushes");

    // different roots, different repo-ids → two ref prefixes, neither wins
    let id_one = repoid(&one).unwrap();
    let id_two = repoid(&two).unwrap();
    assert_ne!(id_one, id_two, "two repos must not share a repo-id");
    let refs = fael_refs(&remote);
    assert_eq!(refs.len(), 2, "{refs:?}");
    assert!(
        refs.iter()
            .any(|r| r.starts_with(&format!("refs/fael/{id_one}/"))),
        "{refs:?}"
    );
    assert!(
        refs.iter()
            .any(|r| r.starts_with(&format!("refs/fael/{id_two}/"))),
        "{refs:?}"
    );

    // repo one reads only its own prefix back: no cross-repo ingest
    assert!(sync(&one).0);
    let seen = ids(&one);
    assert!(seen.contains(&mine), "{seen:?}");
    assert!(
        !seen.contains(&theirs),
        "rows of another repo leaked in: {seen:?}"
    );
}

#[test]
fn one_repo_keeps_one_repo_id_across_clones_and_branches() {
    let remote = bare("roots");
    let d = repo("roots-src", "Root Test", "root@example.com");
    // a second root commit, written without touching the working tree
    let tree = git_out(&d, &["rev-parse", "HEAD^{tree}"]);
    let second = git_out(&d, &["commit-tree", &tree, "-m", "second root"]);
    git(&d, &["branch", "second-root", &second]);
    let first = git_out(&d, &["rev-parse", "HEAD"]);
    let expected = std::cmp::min(first, second);

    point(&d, &remote);
    add(&d, "a row to pin the id with");
    assert!(sync(&d).0, "the source pushes");
    assert_eq!(repoid(&d).unwrap(), expected, "min root sha is the repo-id");

    // a clone derives the same id before anything is cached for it
    let c = clone(&d, "roots-clone", "Root Test", "root@example.com");
    point(&c, &remote);
    assert!(sync(&c).0, "the clone pushes");
    assert_eq!(repoid(&c).unwrap(), expected, "a clone derives the same id");

    // another branch, with no cache to lean on: the roots never move
    git(&c, &["checkout", "-q", "second-root"]);
    git(&c, &["config", "--unset", "fael.repoid"]);
    assert!(sync(&c).0, "sync on the second root's branch");
    assert_eq!(
        repoid(&c).unwrap(),
        expected,
        "the branch set never moves the id"
    );

    let refs = fael_refs(&remote);
    assert_eq!(refs.len(), 1, "{refs:?}");
    assert!(
        refs[0].starts_with(&format!("refs/fael/{expected}/")),
        "one ref under one id: {}",
        refs[0]
    );
}
