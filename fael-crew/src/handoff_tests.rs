//! Hand-offs by file: a done chunk that touched files under this scope speaks in the
//! brief of a chunk written before it (issue 01M5FMHQX).

use super::super::chunk::tests::{NOW, add, here, start};
use super::super::{Fields, Store};
use rusqlite::params;

/// A chunk that is done having touched `file`, its hand-off `text`.
fn finish(s: &mut Store, title: &str, file: &str, text: &str) -> String {
    let u = add(s, title, Some("b"));
    start(s, &u, &format!("/{title}")).unwrap();
    s.claim(&u, &[file.to_string()]).unwrap();
    s.done(&u, text, Some(3), None, &here(&format!("/{title}")))
        .unwrap();
    u
}

fn scoped(s: &mut Store, scope: &str) -> String {
    let u = add(s, "mine", Some("b"));
    let f = Fields {
        scope: Some(vec![scope.into()]),
        ..Fields::default()
    };
    s.edit(&u, &f, NOW).unwrap();
    u
}

#[test]
fn a_done_chunk_on_my_files_is_handed_over_with_its_left_ids() {
    let mut s = Store::open_in_memory().unwrap();
    let me = scoped(&mut s, "src/");
    finish(&mut s, "x", "src/a.rs", "x done\nleft: 01ABC");
    finish(&mut s, "y", "docs/y.md", "y done\nleft: none");
    let b = s.show(&me).unwrap();
    assert_eq!(b.by_file.len(), 1, "only the chunk that touched src/");
    assert!(b.by_file[0].1.contains("left: 01ABC"), "{:?}", b.by_file);
    assert_eq!(b.by_file[0].2, "src/a.rs");
    let t = super::super::start::Started {
        run: "R".into(),
        chunks: vec![b],
    }
    .text();
    assert!(
        t.contains("x, touched files in this scope, handed off:"),
        "{t}"
    );
    assert!(t.contains("> left: 01ABC"), "{t}");
}

#[test]
fn a_brief_edit_after_the_done_hides_it_and_an_after_edge_is_not_repeated() {
    let mut s = Store::open_in_memory().unwrap();
    let me = scoped(&mut s, "src/");
    let x = finish(&mut s, "x", "src/a.rs", "x done");
    assert_eq!(s.show(&me).unwrap().by_file.len(), 1);
    // the brief is rewritten after x's done: it already knows
    let f = Fields {
        brief: Some("rewritten".into()),
        ..Fields::default()
    };
    s.edit(&me, &f, NOW).unwrap();
    assert!(s.show(&me).unwrap().by_file.is_empty());
    // a chunk done after the edit shows again, unless an `after` edge already prints it
    let y = finish(&mut s, "y", "src/b.rs", "y done");
    assert_eq!(s.show(&me).unwrap().by_file.len(), 1);
    s.conn
        .execute(
            "INSERT INTO edge(src, dst, kind) SELECT m.id, c.id, 'after'
             FROM chunk m, chunk c WHERE m.uid = ?1 AND c.uid = ?2",
            params![me, y],
        )
        .unwrap();
    let b = s.show(&me).unwrap();
    assert!(b.by_file.is_empty(), "y goes through after");
    assert_eq!(b.after.len(), 1);
    let _ = x;
}

#[test]
fn by_file_is_newest_first_and_capped_with_the_rest_counted() {
    let mut s = Store::open_in_memory().unwrap();
    let me = scoped(&mut s, "src/");
    for i in 0..7 {
        finish(
            &mut s,
            &format!("c{i}"),
            &format!("src/f{i}.rs"),
            &format!("done {i}"),
        );
    }
    let b = s.show(&me).unwrap();
    assert_eq!((b.by_file.len(), b.by_file_more), (5, 2));
    assert_eq!(b.by_file[0].0, "c6", "newest first");
    let t = super::super::start::Started {
        run: "R".into(),
        chunks: vec![b],
    }
    .text();
    assert!(t.contains("(2 more such chunks not shown)"), "{t}");
}

#[test]
fn more_touched_files_under_the_scope_beats_newer() {
    let mut s = Store::open_in_memory().unwrap();
    let me = scoped(&mut s, "src/");
    let u = add(&mut s, "wide", Some("b"));
    start(&mut s, &u, "/wide").unwrap();
    let files: Vec<String> = (0..3).map(|i| format!("src/w{i}.rs")).collect();
    s.claim(&u, &files).unwrap();
    s.done(&u, "wide done", Some(3), None, &here("/wide"))
        .unwrap();
    finish(&mut s, "narrow", "src/n.rs", "narrow done");
    let b = s.show(&me).unwrap();
    assert_eq!(b.by_file[0].0, "wide", "{:?}", b.by_file);
    assert_eq!(b.by_file[1].0, "narrow");
}

#[test]
fn entries_after_the_second_are_one_line_without_handoff_text() {
    let mut s = Store::open_in_memory().unwrap();
    let me = scoped(&mut s, "src/");
    for i in 0..4 {
        finish(
            &mut s,
            &format!("c{i}"),
            &format!("src/f{i}.rs"),
            &format!("text-{i}"),
        );
    }
    let t = super::super::start::Started {
        run: "R".into(),
        chunks: vec![s.show(&me).unwrap()],
    }
    .text();
    assert!(t.contains("> text-3") && t.contains("> text-2"), "{t}");
    assert!(!t.contains("text-1") && !t.contains("text-0"), "{t}");
    assert!(
        t.contains("- c1 — src/f1.rs") && t.contains("- c0 — src/f0.rs"),
        "{t}"
    );
}
