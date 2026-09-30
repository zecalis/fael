//! What a sync keeps true over time: a purged row stays purged, rows filed
//! under an earlier writer id still travel (once), `meta.json` belongs to the
//! first push, and `pushed N` counts what was sent.

use super::*;

fn purge(d: &Path, id: &str) {
    let (ok, _, err) = fael(d, &["purge", id]);
    assert!(ok, "{err}");
}

#[test]
fn a_purged_row_does_not_come_back_from_the_remote() {
    let remote = bare("purge");
    let src = repo("purge-src", "Seed", "seed@example.com");
    let a = clone(&src, "purge-a", "Alice", "alice@example.com");
    let b = clone(&src, "purge-b", "Bob", "bob@example.com");
    point(&a, &remote);
    point(&b, &remote);
    let keep = add(&a, "a row worth keeping");
    let leak = add(&a, "an internal detail that must go");
    assert!(sync(&a).0);

    purge(&a, &leak);
    let (ok, _, err) = sync(&a);
    assert!(ok, "{err}");
    assert!(!ids(&a).contains(&leak), "the sync brought it back");
    let own = fael_refs(&remote).remove(0);
    assert!(ref_files(&remote, &own).contains(&"purged.txt".to_string()));
    let body = ref_body(&remote, &own);
    assert!(body.contains(&keep) && !body.contains(&leak), "{body}");

    let (ok, _, err) = sync(&b);
    assert!(ok, "{err}");
    let seen = ids(&b);
    assert!(seen.contains(&keep) && !seen.contains(&leak), "{seen:?}");
}

#[test]
fn a_tombstone_beats_a_copy_of_the_row_still_on_the_ref() {
    let remote = bare("tomb");
    let src = repo("tomb-src", "Seed", "seed@example.com");
    let a = clone(&src, "tomb-a", "Alice", "alice@example.com");
    let c = clone(&src, "tomb-c", "Cara", "cara@example.com");
    point(&a, &remote);
    point(&c, &remote);
    let leak = add(&a, "filed by mistake");
    assert!(sync(&a).0);
    let own = fael_refs(&remote).remove(0);
    let line = ref_body(&remote, &own)
        .lines()
        .find(|l| l.contains(&leak))
        .unwrap()
        .to_string();
    purge(&a, &leak);
    assert!(sync(&a).0);

    // an older fael pushes the row onto the ref again, next to the tombstone
    super::secret::plant(&remote, &own, &line);
    assert!(sync(&c).0);
    assert!(!ids(&c).contains(&leak), "cara ingested a purged row");
    assert!(sync(&a).0);
    assert!(!ids(&a).contains(&leak));
    assert!(!ref_body(&remote, &own).contains(&leak), "still on the tip");
}

#[test]
fn rows_filed_under_an_earlier_writer_id_still_travel_once() {
    let remote = bare("oldid");
    let src = repo("oldid-src", "Seed", "seed@example.com");
    let a = clone(&src, "oldid-a", "Alice", "old@example.com");
    let b = clone(&src, "oldid-b", "Bob", "bob@example.com");
    point(&a, &remote);
    point(&b, &remote);
    let old = add(&a, "filed before the email changed");
    git(&a, &["config", "user.email", "new@example.com"]);
    let new = add(&a, "filed after");

    let (ok, out, err) = sync(&a);
    assert!(ok, "{err}");
    assert!(out.contains("pushed 2"), "both rows go: {out}");
    let (ok, _, err) = sync(&b);
    assert!(ok, "{err}");
    assert!(ids(&b).contains(&old) && ids(&b).contains(&new));

    // bob only received them: his sync must not copy them onto a ref of his own
    add(&b, "bob's own row");
    assert!(sync(&b).0);
    let refs = fael_refs(&remote);
    let mine = format!("/{}", writer("Bob", "bob@example.com"));
    let bobs = refs.iter().find(|r| r.ends_with(&mine)).unwrap();
    let body = ref_body(&remote, bobs);
    assert!(!body.contains(&old) && !body.contains(&new), "{body}");
}

#[test]
fn a_second_checkout_keeps_the_ref_meta_and_pushes_only_what_is_new() {
    let remote = bare("meta");
    let src = repo("meta-src", "Seed", "seed@example.com");
    let a = clone(&src, "meta-first", "Alice", "alice@example.com");
    let a2 = clone(&src, "meta-second", "Alice", "alice@example.com");
    point(&a, &remote);
    point(&a2, &remote);
    add(&a, "one");
    add(&a, "two");
    let (_, out, _) = sync(&a);
    assert!(out.contains("pushed 2"), "{out}");
    let own = fael_refs(&remote).remove(0);
    let before = tip(&remote, &own);
    let name = ref_meta(&remote, &own)["name"].clone();

    // a checkout with another dir name and nothing new adds no commit
    let (ok, _, err) = sync(&a2);
    assert!(ok, "{err}");
    assert_eq!(tip(&remote, &own), before, "a meta-only commit was pushed");

    add(&a2, "three");
    let (_, out, _) = sync(&a2);
    assert!(out.contains("pushed 1"), "only the new row: {out}");
    assert_eq!(ref_meta(&remote, &own)["name"], name, "meta was rewritten");
}
