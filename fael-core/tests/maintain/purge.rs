//! `purge_row` against real files in throwaway dirs: tree + journal rewrite,
//! close cascading, and every refusal (edges, close-event ids, immutable
//! files, unreadable lines, unknown/ambiguous ids).

use crate::common::*;
use fael_core::*;
use std::fs;

const W: &str = "tester-0000";

/// Two stores (tree + journal) holding the same add row; the union log over
/// both, as the adapter reads it.
fn two_stores(id: &str) -> (std::path::PathBuf, std::path::PathBuf, Log) {
    let r = root();
    let fael = fael_of(&r);
    let journal = tmp().join("journal");
    let line = || row(id, "note", &["a.rs"]).to_line();
    write_lines(&month_file(&fael, W, MONTH, false), &[line()]);
    write_lines(&month_file(&journal, W, MONTH, false), &[line()]);
    let log = Log {
        rows: vec![row(id, "note", &["a.rs"])],
        closes: vec![],
        warnings: vec![],
    };
    (fael, journal, log)
}

fn read_all(p: &std::path::Path) -> String {
    fs::read_to_string(p).unwrap_or_default()
}

#[test]
fn purge_removes_row_from_tree_and_journal() {
    let (fael, journal, log) = two_stores("01M3QA00000000000000000001");
    let out = purge_row(&fael, Some(&journal), &log, "01M3QA00000000000000000001").unwrap();
    assert_eq!(out.rows, 2, "one line per store");
    assert_eq!(out.closes, 0);
    assert_eq!(out.files.len(), 2);
    for f in &out.files {
        assert!(read_all(f).is_empty(), "nothing left: {}", f.display());
    }
    let gone = format!("{:?}", read(&fael));
    assert!(!gone.contains("01M3QA00000000000000000001"));
}

#[test]
fn purge_by_prefix_takes_close_events_with_it() {
    let (fael, journal, _) = two_stores("01M3QA00000000000000000002");
    let mut c = Row::close(W, "01M3QA00000000000000000002", "done");
    c.id = "01M3QA00000000000000000003".into();
    write_lines(&month_file(&fael, W, MONTH, true), &[c.to_line()]);
    let log = Log {
        rows: vec![row("01M3QA00000000000000000002", "note", &["a.rs"])],
        closes: vec![c],
        warnings: vec![],
    };
    let out = purge_row(&fael, Some(&journal), &log, "01M3QA00000000000000000002").unwrap();
    assert_eq!(out.rows, 2);
    assert_eq!(out.closes, 1);
    assert!(read_all(&month_file(&fael, W, MONTH, true)).is_empty());
}

#[test]
fn purge_takes_bump_events_with_it() {
    let (fael, journal, _) = two_stores("01M3QA00000000000000000006");
    let mut ev = Row::bumped(W, "01M3QA00000000000000000006");
    ev.id = "01M3QA00000000000000000007".into();
    let mut other = Row::bumped(W, "01M3QA0000000000000000000X");
    other.id = "01M3QA00000000000000000008".into();
    let f = month_file(&fael, W, MONTH, false);
    let base = read_all(&f);
    write_lines(&f, &[base.trim_end().into(), ev.to_line(), other.to_line()]);
    let log = Log {
        rows: vec![row("01M3QA00000000000000000006", "note", &["a.rs"]), ev],
        closes: vec![],
        warnings: vec![],
    };
    let out = purge_row(&fael, Some(&journal), &log, "01M3QA00000000000000000006").unwrap();
    assert_eq!(out.rows, 3, "the row in both stores plus its bump event");
    let left = read_all(&f);
    assert!(!left.contains("01M3QA00000000000000000006"), "{left}");
    assert!(
        left.contains("01M3QA00000000000000000008"),
        "another row's event stays: {left}"
    );
}

#[test]
fn purge_refuses_live_edges() {
    let (fael, journal, _) = two_stores("01M3QA00000000000000000004");
    let mut b = row("01M3QA00000000000000000005", "note", &["a.rs"]);
    b.supersedes = Some("01M3QA00000000000000000004".into());
    let log = Log {
        rows: vec![row("01M3QA00000000000000000004", "note", &["a.rs"]), b],
        closes: vec![],
        warnings: vec![],
    };
    let e = purge_row(&fael, Some(&journal), &log, "01M3QA00000000000000000004").unwrap_err();
    assert!(
        e.contains("01M3QA00000000000000000005"),
        "names the blocker: {e}"
    );
    assert!(
        !read_all(&month_file(&fael, W, MONTH, false)).is_empty(),
        "nothing rewritten"
    );
}

#[test]
fn purge_refuses_close_event_id() {
    let (fael, journal, _) = two_stores("01M3QA00000000000000000006");
    let mut c = Row::close(W, "01M3QA00000000000000000006", "done");
    c.id = "01M3QA00000000000000000007".into();
    let log = Log {
        rows: vec![row("01M3QA00000000000000000006", "note", &["a.rs"])],
        closes: vec![c],
        warnings: vec![],
    };
    let e = purge_row(&fael, Some(&journal), &log, "01M3QA00000000000000000007").unwrap_err();
    assert!(e.contains("restore"), "points at fael restore: {e}");
}

#[test]
fn purge_refuses_immutable_compact_files() {
    let r = root();
    let fael = fael_of(&r);
    let dir = fael.join("log").join(W);
    fs::create_dir_all(&dir).unwrap();
    let line = row("01M3QA00000000000000000008", "note", &["a.rs"]).to_line();
    write_lines(&dir.join("compact.99.jsonl"), &[line]);
    let log = Log {
        rows: vec![row("01M3QA00000000000000000008", "note", &["a.rs"])],
        closes: vec![],
        warnings: vec![],
    };
    let e = purge_row(&fael, None, &log, "01M3QA00000000000000000008").unwrap_err();
    assert!(e.contains("immutable"), "{e}");
}

#[test]
fn purge_refuses_broken_lines() {
    let (fael, journal, _) = two_stores("01M3QA00000000000000000009");
    let p = month_file(&fael, W, MONTH, false);
    let mut body = read_all(&p);
    body.push_str("{broken\n");
    fs::write(&p, body).unwrap();
    let log = Log {
        rows: vec![row("01M3QA00000000000000000009", "note", &["a.rs"])],
        closes: vec![],
        warnings: vec![],
    };
    let e = purge_row(&fael, Some(&journal), &log, "01M3QA00000000000000000009").unwrap_err();
    assert!(e.contains("doctor --fix"), "{e}");
    assert!(
        read_all(&p).contains("01M3QA00000000000000000009"),
        "target line still there"
    );
}

#[test]
fn purge_rejects_unknown_and_ambiguous_ids() {
    let (fael, journal, log) = two_stores("01M3QA0000000000000000000A");
    let e = purge_row(&fael, Some(&journal), &log, "01M3QA00NOPE").unwrap_err();
    assert!(e.contains("no row"), "{e}");
    // a second row sharing the prefix makes it ambiguous
    let log2 = Log {
        rows: vec![
            row("01M3QA0000000000000000000A", "note", &["a.rs"]),
            row("01M3QA0000000000000000000B", "note", &["a.rs"]),
        ],
        closes: vec![],
        warnings: vec![],
    };
    let e = purge_row(&fael, Some(&journal), &log2, "01M3QA00").unwrap_err();
    assert!(e.contains("matches 2 rows"), "{e}");
}

#[test]
fn purge_keeps_neighbours_byte_identical() {
    let (fael, journal, _) = two_stores("01M3QA0000000000000000000C");
    let keep = row("01M3QA0000000000000000000D", "note", &["b.rs"]).to_line();
    for store in [&fael, &journal] {
        let p = month_file(store, W, MONTH, false);
        let mut body = read_all(&p);
        body.push_str(&keep);
        body.push('\n');
        fs::write(&p, body).unwrap();
    }
    let log = Log {
        rows: vec![
            row("01M3QA0000000000000000000C", "note", &["a.rs"]),
            row("01M3QA0000000000000000000D", "note", &["b.rs"]),
        ],
        closes: vec![],
        warnings: vec![],
    };
    purge_row(&fael, Some(&journal), &log, "01M3QA0000000000000000000C").unwrap();
    for store in [&fael, &journal] {
        assert_eq!(
            read_all(&month_file(store, W, MONTH, false)),
            format!("{keep}\n")
        );
    }
}

/// store=local worktree: `.fael` symlinks to a main checkout that has none —
/// purge skips the empty tree instead of failing EEXIST creating its lock.
#[cfg(unix)]
#[test]
fn purge_skips_dangling_tree_symlink() {
    let (_, journal, log) = two_stores("01M3QA00000000000000000009");
    let fael = tmp().join("wt").join(".fael");
    fs::create_dir_all(fael.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(tmp().join("missing"), &fael).unwrap();
    let out = purge_row(&fael, Some(&journal), &log, "01M3QA00000000000000000009").unwrap();
    assert_eq!(out.rows, 1, "journal only");
}
