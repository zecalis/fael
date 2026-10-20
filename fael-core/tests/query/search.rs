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
        dormant: Default::default(),
        retired: Default::default(),
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
    // one filter: no "each alone" breakdown, and the hidden closed rows named
    let f = Filter {
        kind: Some("issue".into()),
        ..Filter::default()
    };
    assert_eq!(
        why_empty(&l, &f, "--files"),
        "no rows match kind=issue (open rows only; --all adds closed)"
    );
    let all = Filter { all: true, ..f };
    assert_eq!(why_empty(&l, &all, "--files"), "no rows match kind=issue");
    // closed rows count only under --all, like the find itself
    assert_eq!(
        why_empty(&l, &Filter::default(), "--files"),
        "no rows match"
    );
}

/// An empty find names the `--key` a key-shaped text retries as — glob and
/// prefix forms included — and stays quiet for prose, paths and word lists.
#[test]
fn an_empty_find_names_the_key_a_key_shaped_text_retries_as() {
    let l = log_of(vec![note(1, &["src/a.rs"], None, "some prose")]);
    let f = |t: &str| Filter {
        text: Some(t.into()),
        ..Filter::default()
    };
    assert_eq!(
        why_empty(&l, &f("feature:*"), "--files"),
        "no rows match \"feature:*\" (open rows only; --all adds closed) · a key? use --key feature:*"
    );
    // `#` and `key:` prefixes retry stripped
    assert!(
        why_empty(&l, &f("#feature:login"), "--files")
            .ends_with("· a key? use --key feature:login")
    );
    assert!(
        why_empty(&l, &f("key:feature:login"), "--files")
            .ends_with("· a key? use --key feature:login")
    );
    // prose with a colon, several words, and paths stay quiet
    for t in ["review: skipped", "trailing:", "decide.rs", "src/a.rs:12"] {
        assert!(!why_empty(&l, &f(t), "--files").contains("a key?"), "{t}");
    }
}

/// A key typed as text (`plan:x`, `#k`, `key:k`, a glob) retries as `--key`
/// before the partial-word guesses: a glob answers the open rows its words
/// never match, and a wholly closed key answers hidden instead of missing —
/// labelled as a key answer either way.
#[test]
fn an_empty_find_retries_a_key_shaped_text_as_key() {
    let l = log_of(vec![
        note(1, &["a.md"], Some("feature:login"), "login form"),
        note(2, &["a.md"], Some("feature:billing"), "billing page"),
        note(3, &["a.md"], None, "unrelated prose"),
    ]);
    let text = |t: &str| Filter {
        text: Some(t.into()),
        ..Filter::default()
    };
    let got = |f: &Filter| {
        nearest(&l, f).map(|(label, rows)| {
            let ids = ids(&rows);
            (label, ids.iter().map(|i| i.to_string()).collect::<Vec<_>>())
        })
    };
    // a glob no word matches answers the open keys, labelled as a key answer
    assert_eq!(
        got(&text("feature:*")),
        Some((
            "no rows match text — 2 with key feature:*:".into(),
            vec!["02".to_string(), "01".to_string()]
        ))
    );
    // `#` and `key:` prefixes retry stripped, other filters kept
    assert_eq!(
        got(&text("#feature:billing")).map(|g| g.1),
        Some(vec!["02".to_string()])
    );
    assert_eq!(
        got(&text("key:feature:billing")).map(|g| g.1),
        Some(vec!["02".to_string()])
    );
    // prose with a colon is no key: the word guesses still answer, unlabelled
    assert_eq!(
        got(&text("login billing")).map(|g| g.0),
        Some("no row has every word — 2 with 1 of 2:".into())
    );
}

/// A key-shaped text whose key rows are all closed answers hidden instead of
/// missing: an exact key is already found by the closed branch (row keys still
/// match words), while a glob only `--key` matches retries into the same
/// hidden answer — both labelled, neither a miss.
#[test]
fn an_empty_find_retries_a_closed_key_as_hidden() {
    let mut l = log_of(vec![
        note(1, &["a.md"], Some("feature:login"), "login form"),
        note(2, &["a.md"], None, "unrelated prose"),
    ]);
    let mut close = row(&rid(9), "close", &[], None);
    close.reference = Some(rid(1));
    l.closes.push(close);
    let text = |t: &str| Filter {
        text: Some(t.into()),
        ..Filter::default()
    };
    let got = |f: &Filter| {
        nearest(&l, f).map(|(label, rows)| {
            let ids = ids(&rows);
            (label, ids.iter().map(|i| i.to_string()).collect::<Vec<_>>())
        })
    };
    // exact keys keep the long-standing closed label
    assert_eq!(
        got(&text("feature:login")),
        Some((
            "no open row matches — 1 closed or superseded:".into(),
            vec!["01".to_string()]
        ))
    );
    // a glob no word matches retries into the hidden key rows
    assert_eq!(
        got(&text("feature:*")),
        Some((
            "no open row matches text — 1 hidden with key feature:*:".into(),
            vec!["01".to_string()]
        ))
    );
}

/// Two rows or fewer from the first page show their bodies when the bodies
/// fit the budget; a paged, limited, longer or fat list stays titles.
#[test]
fn a_short_first_page_shows_bodies_that_fit() {
    let (a, b, c) = (
        note(1, &["src/a.rs"], None, "short body"),
        note(2, &["src/a.rs"], None, "another short body"),
        note(3, &["src/a.rs"], None, "third short body"),
    );
    let q = Filter {
        key: Some("k:a".into()),
        ..Filter::default()
    };
    assert!(expands(&[&a], 1, &q, 800) && expands(&[&a, &b], 2, &q, 800));
    assert!(!expands(&[], 0, &q, 800));
    assert!(!expands(&[&a, &b, &c], 3, &q, 800));
    let paged = Filter {
        offset: 1,
        ..q.clone()
    };
    assert!(!expands(&[&a], 1, &paged, 800));
    let limited = Filter {
        limit: Some(5),
        ..q.clone()
    };
    assert!(!expands(&[&a], 1, &limited, 800));
    // a fat body would cost more than the `find <id>` it saves: stays a title
    let fat = note(4, &["src/a.rs"], None, &"word ".repeat(1000));
    assert!(!expands(&[&fat], 1, &q, 800));
    // the brief (no narrowing) stays a map of titles
    assert!(!expands(&[&a], 1, &Filter::default(), 800));
}

/// An empty find answers in the same call: the closed rows the same call
/// would match, else the rows with the most words (at least half) — never a
/// guess off the first page or from a lone word.
#[test]
fn an_empty_find_answers_with_the_nearest_rows() {
    let mut l = log_of(vec![
        note(1, &["src/a.rs"], None, "find ranks by freshness"),
        note(2, &["src/a.rs"], None, "search stays deterministic"),
        note(3, &["src/b.rs"], None, "translate the brief"),
    ]);
    let mut close = row(&rid(9), "close", &[], None);
    close.reference = Some(rid(3));
    l.closes.push(close);
    let text = |t: &str| Filter {
        text: Some(t.into()),
        ..Filter::default()
    };
    let got = |f: &Filter| {
        nearest(&l, f).map(|(label, rows)| {
            let ids = ids(&rows);
            (label, ids.iter().map(|i| i.to_string()).collect::<Vec<_>>())
        })
    };
    // open rows miss, a closed one matches: shown, labelled closed
    assert_eq!(
        got(&text("translate")),
        Some((
            "no open row matches — 1 closed or superseded:".into(),
            vec!["03".to_string()]
        ))
    );
    // no row has both words: the rows with the most, other filters kept
    assert_eq!(
        got(&text("ranks deterministic")),
        Some((
            "no row has every word — 2 with 1 of 2:".into(),
            vec!["02".to_string(), "01".to_string()]
        ))
    );
    let on_b = Filter {
        files: vec!["src/a.rs".into()],
        ..text("freshness vector")
    };
    assert_eq!(got(&on_b).map(|g| g.1), Some(vec!["01".to_string()]));
    // one word of four is no answer; a lone word has no partial; nor page two
    assert_eq!(got(&text("find vector index cache")), None);
    assert_eq!(got(&text("vector")), None);
    let page2 = Filter {
        offset: 5,
        ..text("translate")
    };
    assert_eq!(got(&page2), None);
}
