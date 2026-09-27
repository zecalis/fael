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
fn decision_without_key_and_multi_topic_text_warn() {
    let l = log();
    let cfg = Config::default();
    // the plan's done criterion: decision + no key + "a; b; c" → two warnings
    let mut r = row("D0000000000000000000000017", "decision", &["x"], None);
    r.text = "a; b; c".into();
    let w = warnings(&r, &l, &cfg);
    assert_eq!(w.len(), 2, "{w:?}");
    assert!(w[0].contains("no --key"), "{w:?}");
    assert!(w[1].contains("topic separators"), "{w:?}");
    // `·` counts too; one separator is still a single topic
    r.text = "a · b · c".into();
    assert!(
        warnings(&r, &l, &cfg).len() == 2,
        "{:?}",
        warnings(&r, &l, &cfg)
    );
    r.text = "a; b".into();
    let w = warnings(&r, &l, &cfg);
    assert_eq!(w.len(), 1, "{w:?}");
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
    // the plan's done criterion: open decision, no key, two separators
    let mut r = row("D0000000000000000000000017", "decision", &["x"], None);
    r.text = "a; b; c".into();
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
