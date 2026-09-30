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
