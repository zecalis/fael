//! `plan edit --replace/--with`: one occurrence swaps through the bill; 0 or 2 reject.

use super::super::Store;
use super::super::bill::Bill;
use super::super::chunk::tests::{NOW, add};
use super::super::export_json;
use serde_json::json;

fn store(body: &str) -> Store {
    let mut s = Store::open_in_memory().unwrap();
    add(&mut s, "a", None);
    let id = s.plans().unwrap()[0].id;
    let rev = export_json(&s, id).unwrap()["rev"].clone();
    let b = json!({"v": 1, "plan": "inbox", "rev": rev,
        "sections": [{"key": "c", "title": "Constraints", "body": body}]});
    s.bill(serde_json::from_value::<Bill>(b).unwrap(), NOW, false)
        .unwrap();
    s
}

fn state(s: &Store) -> (String, i64) {
    let j = export_json(s, s.plans().unwrap()[0].id).unwrap();
    (
        j["sections"][0]["body"].as_str().unwrap().to_string(),
        j["rev"].as_i64().unwrap(),
    )
}

#[test]
fn one_occurrence_is_replaced_with_one_rev_bump() {
    let mut s = store("keep a\nold line (x)\nkeep b");
    let (_, r0) = state(&s);
    let p = s.plans().unwrap().remove(0);
    let (lines, r1) = s
        .plan_edit(&p, "c", "old line (x)", "new line", NOW)
        .unwrap();
    assert_eq!(state(&s).0, "keep a\nnew line\nkeep b");
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(r1 > r0);
    assert_eq!(state(&s).1, r1);
}

#[test]
fn zero_and_two_occurrences_reject_and_change_nothing() {
    let mut s = store("dup\ndup\nonce");
    let before = state(&s);
    let p = s.plans().unwrap().remove(0);
    let e = s.plan_edit(&p, "c", "absent", "x", NOW).unwrap_err();
    assert!(e.contains("0 matches"), "{e}");
    let e = s.plan_edit(&p, "c", "dup", "x", NOW).unwrap_err();
    assert!(e.contains("occurs 2 times"), "{e}");
    let e = s.plan_edit(&p, "zz", "once", "x", NOW).unwrap_err();
    assert!(e.contains("no section zz"), "{e}");
    assert_eq!(state(&s), before);
}
