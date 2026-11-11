//! `chunk edit --scope` alone that widens keeps the owner's approval (owner, 2026-10-15),
//! and so does `chunk unafter`. Linking issues (`--issue +<id>`, issue
//! 01M75C6A9GQZN93JMSZZ0S3MTR) keeps it when every new issue's files sit under
//! the scope, clears it otherwise — exact field values asserted.

use super::super::{Fields, Store};
use super::tests::{NOW, add, get};
use rusqlite::params;

fn approved(s: &mut Store, scope: &str) -> String {
    let c = add(s, "c", Some("brief"));
    s.conn
        .execute(
            "UPDATE chunk SET scope = ?1, approved_at = ?2, approved_model = 'opus', approved_client = 'claude' WHERE uid = ?3",
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

fn issues(ids: &[&str], files: &[&[&str]]) -> Fields {
    Fields {
        issues: ids.iter().map(|s| s.to_string()).collect(),
        issue_files: files
            .iter()
            .map(|fs| fs.iter().map(|s| s.to_string()).collect())
            .collect(),
        ..Fields::default()
    }
}

fn kept(s: &Store, c: &str) -> bool {
    get(s, "SELECT approved_model FROM chunk WHERE uid = ?1", c).as_deref() == Some("opus")
}

fn approval(s: &Store, c: &str) -> (Option<String>, Option<String>, Option<String>) {
    (
        get(s, "SELECT approved_at FROM chunk WHERE uid = ?1", c),
        get(s, "SELECT approved_model FROM chunk WHERE uid = ?1", c),
        get(s, "SELECT approved_client FROM chunk WHERE uid = ?1", c),
    )
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

#[test]
fn linking_an_in_scope_issue_keeps_approval_exact() {
    let mut s = Store::open_in_memory().unwrap();
    let c = approved(&mut s, "a.rs");
    let text = s.edit(&c, &issues(&["I1"], &[&["a.rs"]]), NOW).unwrap();
    assert_eq!(text, "edited issue");
    assert_eq!(
        approval(&s, &c),
        (
            Some(NOW.to_string()),
            Some("opus".to_string()),
            Some("claude".to_string())
        )
    );
    let linked: String = s
        .conn
        .query_row(
            "SELECT GROUP_CONCAT(row_id, ',') FROM chunk_issue WHERE chunk = (SELECT id FROM chunk WHERE uid = ?1)",
            [c.clone()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(linked, "I1");
}

#[test]
fn linking_an_out_of_scope_issue_clears_approval_and_says_so() {
    let mut s = Store::open_in_memory().unwrap();
    let c = approved(&mut s, "a.rs");
    let text = s.edit(&c, &issues(&["I1"], &[&["b.rs"]]), NOW).unwrap();
    assert_eq!(text, "edited issue · approval cleared");
    assert_eq!(approval(&s, &c), (None, None, None));
}

#[test]
fn relinking_does_not_clear_and_scope_widening_still_applies() {
    let mut s = Store::open_in_memory().unwrap();
    let c = approved(&mut s, "a.rs");
    s.edit(&c, &issues(&["I1"], &[&["a.rs"]]), NOW).unwrap();
    // already linked: a no-op keeps the exact approval
    s.edit(&c, &issues(&["I1"], &[&["a.rs"]]), NOW).unwrap();
    assert_eq!(
        approval(&s, &c),
        (
            Some(NOW.to_string()),
            Some("opus".to_string()),
            Some("claude".to_string())
        )
    );
    // scope widening alone still keeps; with an out-of-scope issue it clears
    let mut both = scope(&["a/", "b.rs"]);
    both.issues = vec!["I2".into()];
    both.issue_files = vec![vec!["b.rs".into()]];
    let text = s.edit(&c, &both, NOW).unwrap();
    assert!(text.contains("approval cleared"), "{text}");
    assert_eq!(approval(&s, &c), (None, None, None));
}
