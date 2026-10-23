//! `plan lint`: H-refs and label-like tokens with no chunk row.

use super::super::section::Row;
use super::refs_in;
use std::collections::HashSet;

fn row(key: &str, body: &str) -> Row {
    Row {
        key: key.into(),
        title: key.into(),
        body: body.into(),
        updated: "2026-10-23".into(),
    }
}

#[test]
fn h_refs_and_ranges_expand() {
    assert_eq!(refs_in("see H4"), vec!["H4"]);
    assert_eq!(refs_in("H4-H8"), vec!["H4", "H5", "H6", "H7", "H8"]);
    assert_eq!(refs_in("H4 – H8"), vec!["H4", "H5", "H6", "H7", "H8"]);
    assert_eq!(refs_in("H4..H8"), vec!["H4", "H5", "H6", "H7", "H8"]);
    // endpoints cited separately stay separate
    assert_eq!(refs_in("H4, H8"), vec!["H4", "H8"]);
}

#[test]
fn label_like_tokens_caught_versions_and_paths_not() {
    assert!(refs_in("do b10 next").contains(&"b10".to_string()));
    assert!(refs_in("plan/other: see week1").contains(&"week1".to_string()));
    // a version inside a hyphenated name is not a label
    assert!(!refs_in("see board-v1.json").contains(&"v1".to_string()));
    // years and SHAs are not labels
    assert!(refs_in("on 2026-10-23").is_empty());
    // fenced code is skipped
    assert!(refs_in("```\nH99\n```").is_empty());
    assert_eq!(refs_in("H4\n```\nH99\n```\nH5"), vec!["H4", "H5"]);
}

#[test]
fn missing_reports_sections_in_first_seen_order() {
    let secs = vec![
        row("2", "Scope covers H4-H6 and b10"),
        row("3", "H5 is the risky one"),
    ];
    let labels: HashSet<String> = ["H4", "H5", "H6"].into_iter().map(String::from).collect();
    let m = super::missing(&secs, &labels);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].name, "b10");
    assert_eq!(m[0].sections, vec!["2"]);
}
