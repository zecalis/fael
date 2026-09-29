//! resolve · keys · warnings · glob — finding rows and keys by name.

use super::{log, row};
use fael_core::*;

#[test]
fn redis_glob() {
    assert!(glob("auth:*", "auth:session:timeout"));
    assert!(glob("a?c", "abc") && !glob("a?c", "ac"));
    assert!(glob("h[ae]llo", "hello") && !glob("h[ae]llo", "hillo"));
    assert!(glob("h[^e]llo", "hallo") && !glob("h[^e]llo", "hello"));
    assert!(glob("v[0-9]", "v7") && !glob("v[0-9]", "vx"));
    assert!(glob("a\\*", "a*") && !glob("a\\*", "ab"));
    assert!(!glob("auth:*", "billing:x"));
}

#[test]
fn resolve_by_unique_prefix() {
    let l = log();
    assert_eq!(resolve(&l, "b0").unwrap().id, "B0000000000000000000000015"); // case-insensitive
    assert!(resolve(&l, "A000").unwrap_err().contains("matches 5 rows"));
    assert!(resolve(&l, "Z").unwrap_err().contains("no row"));
    assert!(resolve(&l, "").is_err());
}

#[test]
fn keys_by_count() {
    let mut l = log();
    l.rows.push(row(
        "C0000000000000000000000016",
        "note",
        &["x"],
        Some("billing:invoice"),
    ));
    l.rows.push(row(
        "C0000000000000000000000017",
        "note",
        &["x"],
        Some("billing:invoice"),
    ));
    let k = keys(&l, None);
    assert_eq!(k[0].key, "billing:invoice");
    assert_eq!(k[0].count, 3);
    assert_eq!(k[0].last, "2026-09-17T00:00:00Z");
    assert_eq!((k[1].key.as_str(), k[1].count), ("auth:session", 2));
    assert_eq!(keys(&l, Some("auth:*")).len(), 1);
}

#[test]
fn add_warnings_never_reject() {
    let l = log();
    let cfg = Config {
        key_domains: vec!["auth".into()],
        warn_row_tokens: 3,
        ..Config::default()
    };
    let mut r = row(
        "D0000000000000000000000017",
        "note",
        &["x"],
        Some("auth:sesion"),
    );
    let w = warnings(&r, &l, &cfg);
    assert!(w[0].contains("similar keys exist: auth:session"), "{w:?}");
    assert!(w[1].contains("tokens (warn at 3)"), "{w:?}");
    r.key = Some("hiring:backend".into());
    assert!(warnings(&r, &l, &cfg)[0].contains("not in config key_domains"));
    r.key = Some("auth:session".into()); // existing key: no similarity noise
    r.text = "ok".into();
    assert!(warnings(&r, &l, &cfg).is_empty());
}

#[test]
fn similar_keys_parent_match_needs_three_segments() {
    let mut l = log();
    l.rows.push(row(
        "C0000000000000000000000016",
        "decision",
        &["x"],
        Some("vela:docs"),
    ));
    let cfg = Config::default();
    let mut r = row("D0000000000000000000000017", "decision", &["x"], None);
    // same domain, different topic: two-level keys share only the bare
    // domain, which is not similarity — the domain check owns that layer
    r.key = Some("vela:docs-plan".into());
    assert!(warnings(&r, &l, &cfg).is_empty());
    // a typo in the topic still warns through levenshtein
    r.key = Some("vela:docx".into());
    assert!(
        warnings(&r, &l, &cfg)[0].contains("similar keys exist: vela:docs"),
        "{:?}",
        warnings(&r, &l, &cfg)
    );
    // mixed depths share nothing through the parent clause either
    r.key = Some("vela:other:x".into());
    assert!(warnings(&r, &l, &cfg).is_empty());
    // three-segment siblings still match by parent
    l.rows.push(row(
        "C0000000000000000000000017",
        "decision",
        &["x"],
        Some("docs:plan:chunk-1"),
    ));
    r.key = Some("docs:plan:chunk-2".into());
    assert!(
        warnings(&r, &l, &cfg)[0].contains("similar keys exist: docs:plan:chunk-1"),
        "{:?}",
        warnings(&r, &l, &cfg)
    );
}

#[test]
fn decision_without_key_and_multi_topic_text_warn() {
    let l = log();
    let cfg = Config::default();
    // the plan's done criterion: decision + no key + "a; b; c; d" → two warnings
    let mut r = row("D0000000000000000000000017", "decision", &["x"], None);
    r.text = "a; b; c; d".into();
    let w = warnings(&r, &l, &cfg);
    assert_eq!(w.len(), 2, "{w:?}");
    assert!(w[0].contains("no --key"), "{w:?}");
    assert!(w[1].contains("topic separators"), "{w:?}");
    // two `·` already list topics; one separator is still a single topic
    r.text = "a · b · c".into();
    assert!(
        warnings(&r, &l, &cfg).len() == 2,
        "{:?}",
        warnings(&r, &l, &cfg)
    );
    r.text = "a; b".into();
    let w = warnings(&r, &l, &cfg);
    assert_eq!(w.len(), 1, "{w:?}");
    // two `;` is ordinary English prose inside one topic — no split nudge
    r.text = "Key-match wins over files-match; files only apply to notes; \
branch is ignored for keys."
        .into();
    let w = warnings(&r, &l, &cfg);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("no --key"), "{w:?}");
    // mixed separators count together: three in all is several topics
    r.text = "a; b; c · d".into();
    assert_eq!(warnings(&r, &l, &cfg).len(), 2);
    // `—` joins clauses, not topics — it cuts the auto title but never warns
    r.text = "a — b — c".into();
    let w = warnings(&r, &l, &cfg);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("no --key"), "{w:?}");
    // notes stay keyless without complaint; a key quiets the key warning
    let mut n = row("N0000000000000000000000018", "note", &["x"], None);
    n.text = "a; b".into();
    assert!(warnings(&n, &l, &cfg).is_empty());
    r.key = Some("plugh:xyzzy".into());
    r.text = "single topic".into();
    assert!(warnings(&r, &l, &cfg).is_empty());
}

#[test]
fn fat_reasons_shared_by_warnings_and_doctor() {
    let l = log();
    let cfg = Config::default();
    // the plan's done criterion: open decision, no key, three separators
    let mut r = row("D0000000000000000000000017", "decision", &["x"], None);
    r.text = "a; b; c; d".into();
    let f = fat_reasons(&r, &cfg);
    assert_eq!(f.len(), 2, "{f:?}");
    assert!(f[0].contains("no --key"), "{f:?}");
    assert!(f[1].contains("topic separators"), "{f:?}");
    // warnings renders the same reasons with the prefix, nothing more
    let w = warnings(&r, &l, &cfg);
    assert_eq!(w.len(), 2, "{w:?}");
    assert!(w[0].ends_with(&f[0]) && w[1].ends_with(&f[1]), "{w:?}");
    // a keyed single-topic decision is lean; notes stay lean too
    r.key = Some("a:b".into());
    r.text = "single topic".into();
    assert!(fat_reasons(&r, &cfg).is_empty());
}

#[test]
fn separators_warn_by_density_not_count() {
    let cfg = Config::default();
    let mut r = row(
        "D0000000000000000000000017",
        "decision",
        &["x"],
        Some("k:v"),
    );
    // a long single-topic row carries its `;` as prose — each clause is a
    // sentence of its own, no split nudge
    let clause = "word ".repeat(50).trim_end().to_string();
    r.text = format!("{clause}; {clause}; {clause}; {clause}");
    assert!(
        fat_reasons(&r, &cfg).is_empty(),
        "{:?}",
        fat_reasons(&r, &cfg)
    );
    // the same three separators in a short row read as a list of topics
    r.text = "a; b; c; d".into();
    assert!(
        fat_reasons(&r, &cfg)[0].contains("topic separators"),
        "{:?}",
        fat_reasons(&r, &cfg)
    );
}

#[test]
fn docs_only_without_anchor_warns() {
    let l = log();
    let cfg = Config::default();
    let mk = |files: &[&str]| {
        let mut r = row("D0000000000000000000000017", "note", files, Some("k:v"));
        r.text = "about a doc".into();
        r
    };
    let w = warnings(&mk(&["spec/x.md", "notes/y.md"]), &l, &cfg);
    assert!(w[0].contains("only *.md docs"), "{w:?}");
    // a code file beside the docs is a lasting foothold — silent
    assert!(warnings(&mk(&["src/a.rs", "spec/x.md"]), &l, &cfg).is_empty());
    // an anchor never goes, so it anchors the row by itself too
    assert!(warnings(&mk(&["spec/x.md", "doc:pricing"]), &l, &cfg).is_empty());
    assert!(warnings(&mk(&["doc:pricing"]), &l, &cfg).is_empty());
    // nothing to judge on an empty file list
    assert!(warnings(&mk(&[]), &l, &cfg).is_empty());
}

#[test]
fn fat_warning_names_the_limit_that_tripped() {
    let cfg = Config::default(); // tokens 400 · chars 1200
    let mut r = row("D0000000000000000000000017", "note", &["x"], Some("a:b"));
    // 1299 chars of plain English: ~325 tokens (under 400) but over 1200 chars
    r.text = "word ".repeat(260).trim_end().into();
    assert_eq!(r.text.chars().count(), 1299);
    let f = fat_reasons(&r, &cfg);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].contains("chars (warn at 1200)"), "{f:?}");
    assert!(!f[0].contains("tokens (warn at"), "{f:?}");
    // over both limits names both
    let tight = Config {
        warn_row_tokens: 3,
        warn_row_chars: 10,
        ..Config::default()
    };
    let f = fat_reasons(&r, &tight);
    assert!(f[0].contains("tokens (warn at 3) and"), "{f:?}");
    assert!(f[0].contains("chars (warn at 10)"), "{f:?}");
    // under both limits: silent
    r.text = "short one".into();
    assert!(fat_reasons(&r, &cfg).is_empty());
}
