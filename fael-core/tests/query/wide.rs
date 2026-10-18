//! `--recall wide` (f1): exact hits, key-sharing rows, unfiled text hits.

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

/// A on file x keyed k matching the query, B on file y keyed k with other
/// text, C on file z matching the query, D unrelated.
fn log() -> Log {
    Log {
        rows: vec![
            note(1, &["x"], Some("k"), "sunset deploy on x"),
            note(2, &["y"], Some("k"), "unrelated bookkeeping"),
            note(3, &["z"], None, "sunset deploy on z"),
            note(4, &["w"], None, "unrelated other"),
        ],
        closes: vec![],
        warnings: vec![],
        dormant: Default::default(),
        retired: Default::default(),
    }
}

#[test]
fn wide_returns_exact_key_sharing_and_text_in_order() {
    let l = log();
    let f = Filter {
        files: vec!["x".into()],
        text: Some("sunset deploy".into()),
        recall: Some("wide".into()),
        ..Filter::default()
    };
    assert_eq!(ids(&find(&l, &f)), ["01", "02", "03"]);
}

#[test]
fn default_stays_today_result() {
    let l = log();
    let f = Filter {
        files: vec!["x".into()],
        text: Some("sunset deploy".into()),
        ..Filter::default()
    };
    assert_eq!(ids(&find(&l, &f)), ["01"]);
}

#[test]
fn wide_without_text_widens_by_key_only() {
    let l = log();
    let f = Filter {
        files: vec!["x".into()],
        recall: Some("wide".into()),
        ..Filter::default()
    };
    assert_eq!(ids(&find(&l, &f)), ["01", "02"]);
}

#[test]
fn recall_rejects_anything_but_wide() {
    assert!(parse_recall(None).unwrap().is_none());
    assert_eq!(parse_recall(Some("wide")).unwrap(), Some("wide".into()));
    assert!(parse_recall(Some("broad")).is_err());
}
