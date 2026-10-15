//! `chunk edit --scope` alone that widens keeps the owner's approval (owner, 2026-10-15),
//! and so does `chunk unafter`.

use super::super::{Fields, Store};
use super::tests::{NOW, add, get};
use rusqlite::params;

fn approved(s: &mut Store, scope: &str) -> String {
    let c = add(s, "c", Some("brief"));
    s.conn
        .execute(
            "UPDATE chunk SET scope = ?1, approved_at = ?2, approved_model = 'opus' WHERE uid = ?3",
            params![scope, NOW, c],
        )
        .unwrap();
    c
}

fn scope(paths: &[&str]) -> Fields {
    Fields {
        scope: Some(paths.iter().map(|p| p.to_string()).collect()),
        ..Fields::default()
    }
}

fn kept(s: &Store, c: &str) -> bool {
    get(s, "SELECT approved_model FROM chunk WHERE uid = ?1", c).as_deref() == Some("opus")
}

#[test]
fn a_scope_that_keeps_every_old_path_keeps_the_approval() {
    let mut s = Store::open_in_memory().unwrap();
    let c = approved(&mut s, "apps/web/a.ts\napps/core/b.ts");
    // a.ts is still listed, b.ts now sits under its folder, c.ts is new
    s.edit(
        &c,
        &scope(&["apps/web/a.ts", "apps/core/", "apps/domain/c.ts"]),
        NOW,
    )
    .unwrap();
    assert!(kept(&s, &c));
    let ev = get(
        &s,
        "SELECT text FROM event WHERE chunk_uid = ?1 ORDER BY id DESC LIMIT 1",
        &c,
    );
    assert_eq!(ev.as_deref(), Some("edited scope"));
}

#[test]
fn a_scope_that_drops_a_path_or_comes_with_another_field_clears_it() {
    let mut s = Store::open_in_memory().unwrap();
    let narrowed = approved(&mut s, "apps/web/\napps/core/b.ts");
    s.edit(&narrowed, &scope(&["apps/web/a.ts", "apps/core/b.ts"]), NOW)
        .unwrap();
    assert!(!kept(&s, &narrowed));

    let both = approved(&mut s, "apps/web/a.ts");
    let f = Fields {
        size: Some("M".into()),
        ..scope(&["apps/web/"])
    };
    s.edit(&both, &f, NOW).unwrap();
    assert!(!kept(&s, &both));
}

#[test]
fn unafter_drops_the_edge_and_keeps_the_approval() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("brief"));
    let f = Fields {
        title: Some("b".into()),
        brief: Some("brief".into()),
        ..Fields::default()
    };
    let b = s.add("inbox", &f, std::slice::from_ref(&a), NOW).unwrap();
    s.conn
        .execute(
            "UPDATE chunk SET approved_at = ?1, approved_model = 'opus' WHERE uid = ?2",
            params![NOW, b],
        )
        .unwrap();
    s.unafter(&b, &a, "a shipped another way", NOW).unwrap();
    assert!(kept(&s, &b));
    let edges: i64 = s
        .conn
        .query_row("SELECT COUNT(*) FROM edge WHERE kind = 'after'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(edges, 0);
    let e = s.unafter(&b, &a, "again", NOW).unwrap_err();
    assert!(e.contains("does not wait on"), "{e}");
}
