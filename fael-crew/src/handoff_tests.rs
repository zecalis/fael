//! Hand-offs by file: a done chunk that touched files under this scope speaks in the
//! brief of a chunk written before it (issue 01M5FMHQX).

use super::super::chunk::Here;
use super::super::chunk::tests::{NOW, add, here, here_on, start};
use super::super::{Fields, Start, Store};
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
        orphaned: vec![],
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
        orphaned: vec![],
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
        orphaned: vec![],
    }
    .text();
    assert!(t.contains("> text-3") && t.contains("> text-2"), "{t}");
    assert!(!t.contains("text-1") && !t.contains("text-0"), "{t}");
    assert!(
        t.contains("- c1 — src/f1.rs") && t.contains("- c0 — src/f0.rs"),
        "{t}"
    );
}

/// Live touch (issue 01M5PB7Z): a running chunk's `touch` shows in the brief of a
/// chunk whose scope it meets — exactly what the agent reported, newest run first,
/// distinct from the done-claimed section.
#[test]
fn a_running_chunks_touch_shows_live_in_a_scope_overlapping_brief() {
    let mut s = Store::open_in_memory().unwrap();
    let me = scoped(&mut s, "src/");
    let a = add(&mut s, "a", Some("b"));
    let f = Fields {
        scope: Some(vec!["src/".into()]),
        ..Fields::default()
    };
    s.edit(&a, &f, NOW).unwrap();
    start(&mut s, &a, "/wa").unwrap();
    s.touch(
        &a,
        &["src/a.rs".to_string(), "src/b.rs".to_string()],
        &here("/wa"),
    )
    .unwrap();
    let b = s.show(&me).unwrap();
    assert_eq!(b.live.len(), 1, "{:?}", b.live);
    assert_eq!(b.live[0].0, "a");
    let files: Vec<&str> = b.live[0].1.lines().collect();
    assert_eq!(files, vec!["src/a.rs", "src/b.rs"], "{:?}", b.live);
    assert!(b.by_file.is_empty(), "done-claimed stays empty: {b:?}");
    let t = super::super::start::Started {
        run: "R".into(),
        chunks: vec![b],
        orphaned: vec![],
    }
    .text();
    assert!(
        t.contains("live touching files in this scope:"),
        "live section distinct from done-claimed: {t}"
    );
    assert!(
        t.contains("files it is touching now: src/a.rs, src/b.rs"),
        "{t}"
    );
    assert!(
        !t.contains("touched files in this scope, handed off"),
        "no done hand-off for a running chunk: {t}"
    );
}

#[test]
fn touch_unions_never_infers_and_only_running_counts() {
    let mut s = Store::open_in_memory().unwrap();
    let me = scoped(&mut s, "src/");
    let a = add(&mut s, "a", Some("b"));
    let f = Fields {
        scope: Some(vec!["src/".into()]),
        ..Fields::default()
    };
    s.edit(&a, &f, NOW).unwrap();
    start(&mut s, &a, "/wa").unwrap();
    // a bad path rejects the whole call: nothing stored
    let e = s
        .touch(&a, &["../etc/passwd".to_string()], &here("/wa"))
        .unwrap_err();
    assert!(e.contains("scope"), "{e}");
    assert!(s.show(&me).unwrap().live.is_empty());
    // two calls union; a file outside my scope is stored but never shown to me
    s.touch(&a, &["src/a.rs".to_string()], &here("/wa"))
        .unwrap();
    s.touch(
        &a,
        &["src/a.rs".to_string(), "other/z.rs".to_string()],
        &here("/wa"),
    )
    .unwrap();
    let b = s.show(&me).unwrap();
    assert_eq!(b.live.len(), 1);
    assert_eq!(
        b.live[0].1.lines().collect::<Vec<_>>(),
        vec!["src/a.rs", "other/z.rs"],
        "stored whole, shown because one meets my scope: {b:?}"
    );
    // done chunks never show as live, even with a live row left behind
    let d = add(&mut s, "d", Some("b"));
    s.edit(
        &d,
        &Fields {
            scope: Some(vec!["src/".into()]),
            ..Fields::default()
        },
        NOW,
    )
    .unwrap();
    s.start(
        &d,
        &Start {
            force: true,
            ..Start::default()
        },
        &here("/wd"),
    )
    .unwrap();
    s.touch(&d, &["src/d.rs".to_string()], &here("/wd"))
        .unwrap();
    s.done(&d, "d done", Some(1), None, &here("/wd")).unwrap();
    let b = s.show(&me).unwrap();
    assert!(
        b.live.iter().all(|(t, _)| t != "d"),
        "done chunk is no live entry: {b:?}"
    );
    // newest run first
    let c = add(&mut s, "c", Some("b"));
    s.edit(
        &c,
        &Fields {
            scope: Some(vec!["src/".into()]),
            ..Fields::default()
        },
        NOW,
    )
    .unwrap();
    s.start(
        &c,
        &Start {
            force: true,
            ..Start::default()
        },
        &here("/wc"),
    )
    .unwrap();
    s.touch(&c, &["src/c.rs".to_string()], &here("/wc"))
        .unwrap();
    let b = s.show(&me).unwrap();
    assert_eq!(b.live.len(), 2);
    assert_eq!(b.live[0].0, "c", "newest first: {b:?}");
    assert_eq!(b.live[1].0, "a");
    // a fenced touch (another worktree without --run) changes nothing
    let e = s
        .touch(
            &a,
            &["src/nope.rs".to_string()],
            &here_on("/elsewhere", "x"),
        )
        .unwrap_err();
    assert!(
        e.contains("was started in /wa, not here (/elsewhere)"),
        "{e}"
    );
}

/// The `--run R` seat: names the live run, like wait/after/done — a foreign R is
/// fenced off, an ended R is no live run.
#[test]
fn touch_with_run_names_the_live_run() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("b"));
    let r = start(&mut s, &a, "/wa").unwrap();
    let live = Here {
        worktree: "/elsewhere",
        branch: Some("b"),
        now: NOW,
        run: Some(&r),
    };
    // another worktree with --run R answers: the seat rule is the run, not the cwd
    s.touch(&a, &["src/a.rs".to_string()], &live).unwrap();
    assert_eq!(s.show(&a).unwrap().live, vec![]);
    // a foreign R is fenced off before anything is stored
    let bad = Here {
        worktree: "/wa",
        branch: Some("b"),
        now: NOW,
        run: Some("R-BAD"),
    };
    let e = s.touch(&a, &["src/b.rs".to_string()], &bad).unwrap_err();
    assert!(e.contains("no longer holds"), "{e}");
    // an ended R names no live run
    s.run_end(&r, None, NOW).unwrap();
    let e = s.touch(&a, &["src/c.rs".to_string()], &live).unwrap_err();
    assert!(e.contains("live run"), "{e}");
}
