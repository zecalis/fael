//! key_hints — a prompt word equal to the key's head (first segment after the
//! namespace) points at the key; no fuzz. Rank by match strength: more prompt
//! words met wins (a key typed whole over head-only), then most used, then name.

use super::row;
use fael_core::*;

fn keys_of(log: &Log, prompt: &str) -> Vec<String> {
    key_hints(log, prompt)
        .into_iter()
        .map(|(k, _)| k.key)
        .collect()
}

fn ledger() -> Log {
    let k = |id: &str, key: &str| row(id, "decision", &["src/a.rs"], Some(key));
    Log {
        rows: vec![
            k("A0000000000000000000000010", "vela:credit-ledger"),
            k("A0000000000000000000000011", "vela:credit-ledger"),
            k("A0000000000000000000000012", "vela:credit-layer"),
            k("A0000000000000000000000013", "vela:auth-token"),
            k("A0000000000000000000000014", "vela:billing-run"),
            k("A0000000000000000000000015", "vela:plan-3"),
            k("A0000000000000000000000016", "vela:gone-topic"),
        ],
        closes: vec![Row::close("t-0000", "A0000000000000000000000016", "done")],
        ..Log::default()
    }
}

#[test]
fn exact_segment_points_most_used_first() {
    let l = ledger();
    // Thai around the word still splits it out; case does not matter
    assert_eq!(
        keys_of(&l, "ยังไม่มีโค้ด Credit เลยใช่ไหม"),
        ["vela:credit-ledger", "vela:credit-layer"]
    );
    let h = key_hints(&l, "credit");
    assert_eq!(h[0].0.count, 2);
    // 01M3XKB7G: the hint names the word that matched
    assert_eq!(h[0].1, ["credit"]);
}

#[test]
fn never_fuzzy() {
    let l = ledger();
    // prefix, plural, typo, substring: nothing
    for p in ["cred", "credits", "credt", "ledge", "creditledger"] {
        assert!(keys_of(&l, p).is_empty(), "{p}");
    }
}

#[test]
fn area_words_short_and_numeric_segments_stay_quiet() {
    let l = ledger();
    // `vela` names every key — an area, not a topic
    assert!(keys_of(&l, "vela").is_empty());
    // under 4 chars, all digits
    assert!(keys_of(&l, "run 3").is_empty());
}

#[test]
fn closed_or_superseded_keys_are_not_open() {
    let mut l = ledger();
    assert!(keys_of(&l, "gone").is_empty());
    let mut sup = row(
        "A0000000000000000000000017",
        "decision",
        &["src/a.rs"],
        Some("vela:other"),
    );
    sup.supersedes = Some("A0000000000000000000000013".into());
    l.rows.push(sup);
    assert!(keys_of(&l, "auth").is_empty());
}

#[test]
fn at_most_three_keys() {
    let l = ledger();
    assert_eq!(keys_of(&l, "credit billing plan").len(), 3);
}

/// 01M3XKB7E: a key typed whole ranks no lower than keys met by one common
/// word, and the area-word rule drops only the word, never a stronger match.
#[test]
fn key_typed_whole_outranks_head_only_hits() {
    let k = |id: &str, key: &str| row(id, "decision", &["src/a.rs"], Some(key));
    let l = Log {
        rows: vec![
            k("A0000000000000000000000030", "cli:close-multi"),
            k("A0000000000000000000000031", "close:chain"),
            k("A0000000000000000000000032", "close:loop"),
            // extra open rows so the close:* keys win the old count-only sort
            k("A0000000000000000000000033", "close:chain"),
            k("A0000000000000000000000034", "close:loop"),
            k("A0000000000000000000000035", "plan:x-y:handoff"),
        ],
        ..Log::default()
    };
    // the typed key survives, ranked first despite fewer rows
    assert_eq!(
        keys_of(&l, "close out plan:x-y:handoff")[0],
        "plan:x-y:handoff"
    );
}

/// 01M3XKB7E: a topic does not switch off as it grows — with 4 open keys
/// sharing the head, plain 'credit' still names the most used, and a key the
/// prompt names beyond its head ranks first.
#[test]
fn growing_topic_keeps_hinting() {
    let k = |id: &str, key: &str| row(id, "decision", &["src/a.rs"], Some(key));
    let l = Log {
        rows: vec![
            k("A0000000000000000000000040", "vela:credit-ledger"),
            k("A0000000000000000000000041", "vela:credit-ledger"),
            k("A0000000000000000000000042", "vela:credit-layer"),
            k("A0000000000000000000000043", "vela:credit-notes"),
            k("A0000000000000000000000044", "vela:ledger-run"),
        ],
        ..Log::default()
    };
    // a 4th credit-* key used to silence 'credit' entirely — now the top 3
    // most-used hint instead
    let got = keys_of(&l, "credit");
    assert_eq!(got.len(), 3);
    assert_eq!(got[0], "vela:credit-ledger");
    // a prompt naming a trailing segment too lifts that key over head-only
    // matches of the same strength ('ledger' hits credit-ledger AND ledger-run,
    // but credit-ledger met one more word) — the ties below sort by usage, name
    let h = key_hints(&l, "credit ledger merge");
    assert_eq!(h[0].0.key, "vela:credit-ledger");
    assert_eq!(h[0].1, ["credit", "ledger"]);
    assert_eq!(h[1].0.key, "vela:credit-layer");
    assert_eq!(h[1].1, ["credit"]);
    assert_eq!(h[2].0.key, "vela:credit-notes");
    assert_eq!(h[2].1, ["credit"]);
}

#[test]
fn namespace_and_trailing_segments_never_match() {
    // the vela report: "data scope" named every `*-scope` key, "fael" named
    // fael:store — only the head after the namespace is a topic word
    let k = |id: &str, key: &str| row(id, "decision", &["src/a.rs"], Some(key));
    let l = Log {
        rows: vec![
            k("A0000000000000000000000020", "vela:adjustment-scope"),
            k("A0000000000000000000000021", "vela:line-scope"),
            k("A0000000000000000000000022", "vela:tax-scope"),
            k("A0000000000000000000000023", "fael:store"),
        ],
        ..Log::default()
    };
    assert!(keys_of(&l, "data scope").is_empty());
    assert!(keys_of(&l, "the fael log").is_empty());
    assert_eq!(keys_of(&l, "store"), ["fael:store"]);
    assert_eq!(keys_of(&l, "adjustment"), ["vela:adjustment-scope"]);
}
