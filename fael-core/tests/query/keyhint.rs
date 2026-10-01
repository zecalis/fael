//! key_hints — a prompt word equal to one key segment points at the key; no fuzz.

use super::row;
use fael_core::*;

fn keys_of(log: &Log, prompt: &str) -> Vec<String> {
    key_hints(log, prompt).into_iter().map(|k| k.key).collect()
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
    assert_eq!(h[0].count, 2);
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
    assert!(keys_of(&l, "token").is_empty());
}

#[test]
fn at_most_three_keys() {
    let l = ledger();
    assert_eq!(keys_of(&l, "credit token billing plan").len(), 3);
}
