//! Two writers of one repo push at the same time: two refs under one
//! repo-id, no non-fast-forward, and each ref carries only its own rows.

use super::*;

#[test]
fn two_writers_pushing_at_once_land_both_refs() {
    let remote = bare("writers");
    let src = repo("writers-src", "Seed", "seed@example.com");
    let a = clone(&src, "writers-a", "Alice", "alice@example.com");
    let b = clone(&src, "writers-b", "Bob", "bob@example.com");
    point(&a, &remote);
    point(&b, &remote);
    let from_a = add(&a, "row from alice");
    let from_b = add(&b, "row from bob");

    let (ra, rb) = std::thread::scope(|s| {
        let h1 = s.spawn(|| sync(&a));
        let h2 = s.spawn(|| sync(&b));
        (h1.join().unwrap(), h2.join().unwrap())
    });
    assert!(ra.0, "alice's sync must succeed: {}", ra.2);
    assert!(rb.0, "bob's sync must succeed: {}", rb.2);
    let err = format!("{}{}", ra.2, rb.2);
    assert!(
        !err.contains("remote moved twice"),
        "a push was rejected: {err}"
    );

    // each clone reads the other writer back (a push may have raced past the
    // other's ref, so one more round converges them for the assertions)
    assert!(sync(&a).0, "alice converges");
    assert!(sync(&b).0, "bob converges");

    let id = repoid(&a).unwrap();
    assert_eq!(repoid(&b).unwrap(), id, "one repo, one repo-id");
    let r_alice = format!("refs/fael/{id}/{}", writer("Alice", "alice@example.com"));
    let r_bob = format!("refs/fael/{id}/{}", writer("Bob", "bob@example.com"));
    let refs = fael_refs(&remote);
    assert_eq!(refs.len(), 2, "{refs:?}");
    assert!(refs.contains(&r_alice), "{refs:?}");
    assert!(refs.contains(&r_bob), "{refs:?}");

    // rows of whom they are filed by: a writer's ref never carries another
    // writer's rows — the push path filters to its own journal
    let body = ref_body(&remote, &r_alice);
    assert!(body.contains("row from alice"), "{body}");
    assert!(
        !body.contains("row from bob"),
        "foreign row in alice's ref: {body}"
    );
    let body = ref_body(&remote, &r_bob);
    assert!(body.contains("row from bob"), "{body}");
    assert!(
        !body.contains("row from alice"),
        "foreign row in bob's ref: {body}"
    );

    // both clones end with the same two rows, each id exactly once
    for d in [&a, &b] {
        let seen = unique_ids(d);
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert!(seen.contains(&from_a) && seen.contains(&from_b), "{seen:?}");
    }
}
