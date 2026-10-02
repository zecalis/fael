//! Deterministic search scenarios (PLAN-fael-moat-token chunk 4): each states
//! the rows a non-developer expects and asserts that `find` returns exactly
//! those — recall and precision in one `assert_eq!`. S1 (word-AND text) sits
//! in `select.rs`, S4 (`--branches` under `store = "local"`) in `fael/tests/branches.rs`.

use super::{ids, row};
use fael_core::*;

fn rid(n: u32) -> String {
    format!("A{:0>25}", n)
}

fn note(n: u32, files: &[&str], key: Option<&str>, text: &str) -> Row {
    let mut r = row(&rid(n), "note", files, key);
    r.text = text.into();
    r
}

fn log_of(rows: Vec<Row>) -> Log {
    Log {
        rows,
        closes: vec![],
        warnings: vec![],
    }
}

/// S2: a busy file pushes its freshest five whatever the topic (chunk 2,
/// `hot_file_pushes_the_freshest_five_whatever_the_topic`); the rows the cut
/// hid come back with `--files` plus one word.
#[test]
fn s2_files_plus_a_word_reaches_the_rows_the_push_cut() {
    let rows = (1..=30)
        .map(|n| {
            let t = if n == 2 || n == 5 {
                "kickoff ranks by freshness"
            } else {
                "some other topic"
            };
            note(n, &["docs/architecture.md"], None, t)
        })
        .collect();
    let l = log_of(rows);
    let f = Filter {
        files: vec!["docs/architecture.md".into()],
        text: Some("kickoff".into()),
        ..Filter::default()
    };
    assert_eq!(ids(&find(&l, &f)), ["05", "02"]);
}

/// S3: a key glob lists one topic — `feature:*` takes every `feature:` key,
/// never a lookalike prefix or the word in the middle of another key.
#[test]
fn s3_a_key_glob_lists_one_topic_and_nothing_else() {
    let l = log_of(vec![
        note(1, &["a.md"], Some("feature:login:timeout"), "a"),
        note(2, &["a.md"], Some("feature:billing"), "b"),
        note(3, &["a.md"], Some("featureflag:dark"), "c"),
        note(4, &["a.md"], Some("ops:feature:x"), "d"),
        note(5, &["a.md"], None, "e"),
    ]);
    let f = Filter {
        key: Some("feature:*".into()),
        ..Filter::default()
    };
    assert_eq!(ids(&find(&l, &f)), ["02", "01"]);
}

/// A blank text holds no word: it narrows nothing, so `find` answers with
/// the brief (the same rows as no text), never "every row matches a space".
#[test]
fn blank_text_narrows_nothing() {
    let blank = Filter {
        text: Some("  ".into()),
        ..Filter::default()
    };
    assert!(blank.is_empty());
    let l = log_of(vec![
        note(1, &["a.md"], None, "a"),
        note(2, &["b.md"], None, "b"),
    ]);
    assert_eq!(ids(&find(&l, &blank)), ids(&find(&l, &Filter::default())));
}

/// An empty find names the part that matched nothing: each word, file and
/// filter counted on its own, so the agent fixes one word instead of guessing.
#[test]
fn an_empty_find_counts_each_part_alone() {
    let l = log_of(vec![
        note(1, &["src/a.rs"], None, "find ranks by freshness"),
        note(2, &["src/a.rs"], None, "search stays deterministic"),
        note(3, &["src/b.rs"], None, "search of find"),
    ]);
    let f = Filter {
        text: Some("find search".into()),
        kind: Some("issue".into()),
        ..Filter::default()
    };
    assert!(find(&l, &f).is_empty());
    assert_eq!(
        why_empty(&l, &f, "--files"),
        "no rows match — each alone: \"find\" ×2 · \"search\" ×2 · kind=issue ×0"
    );
    // a word no row uses reads ×0, and a path typed as a word points at --files
    let f = Filter {
        text: Some("vector decide.rs".into()),
        ..Filter::default()
    };
    assert_eq!(
        why_empty(&l, &f, "files"),
        "no rows match — each alone: \"vector\" ×0 · \"decide.rs\" ×0 · a path? use files"
    );
    // closed rows count only under --all, like the find itself
    assert_eq!(
        why_empty(&l, &Filter::default(), "--files"),
        "no rows match"
    );
}

/// Two rows or fewer from the first page show their bodies; a paged, limited
/// or longer list stays titles.
#[test]
fn a_short_first_page_shows_bodies() {
    let q = Filter {
        key: Some("k:a".into()),
        ..Filter::default()
    };
    assert!(!expands(0, &q) && expands(1, &q) && expands(EXPAND_MAX, &q));
    assert!(!expands(EXPAND_MAX + 1, &q));
    let paged = Filter {
        offset: 1,
        ..q.clone()
    };
    assert!(!expands(1, &paged));
    let limited = Filter {
        limit: Some(5),
        ..q
    };
    assert!(!expands(1, &limited));
    // the brief (no narrowing) stays a map of titles
    assert!(!expands(1, &Filter::default()));
}
